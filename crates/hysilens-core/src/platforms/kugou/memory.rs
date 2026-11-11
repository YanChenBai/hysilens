// 播放地址与指针链改编自 AF Media Bar；MIT 版权声明见 THIRD_PARTY_NOTICES.md。
use anyhow::{Context, ensure};
use std::mem::size_of;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    },
    System::{
        Diagnostics::{Debug::ReadProcessMemory, ToolHelp::*},
        Threading::{OpenProcess, PROCESS_VM_READ},
    },
};
use windows::core::{PCWSTR, w};

#[derive(Clone, Copy)]
struct Layout {
    position_offset: usize,
    duration_offset: usize,
    duration_chain: &'static [usize],
    duration_units_per_second: f64,
}

const LEGACY_LAYOUT: Layout = Layout {
    position_offset: 0x3929d8,
    duration_offset: 0x023c7b68,
    duration_chain: &[0x70, 0x30, 0, 0x24],
    duration_units_per_second: 1.0,
};

const LEGACY_LAYOUTS: &[Layout] = &[
    LEGACY_LAYOUT,
    Layout {
        duration_offset: 0x023c9b68,
        ..LEGACY_LAYOUT
    },
];

// 20.1.61.28014：已用两首不同歌曲核对时长，并确认进度连续递增。
const V20161_LAYOUTS: &[Layout] = &[Layout {
    position_offset: 0x3949f8,
    duration_offset: 0x02434b30,
    duration_chain: &[0x130, 0x130, 0x2a4],
    duration_units_per_second: 1000.0,
}];

fn layouts(
    position_version: [u16; 4],
    duration_version: [u16; 4],
) -> anyhow::Result<&'static [Layout]> {
    match (position_version, duration_version) {
        ([1, 2, 0, 519], [20, 1, 51, _]) => Ok(LEGACY_LAYOUTS),
        ([1, 2, 0, 523], [20, 1, 61, 28014]) => Ok(V20161_LAYOUTS),
        _ => anyhow::bail!(
            "酷狗模块版本尚未适配 (kgplayer.dll: {position_version:?}, kugou.dll: {duration_version:?})"
        ),
    }
}

fn file_version(path: &[u16]) -> anyhow::Result<[u16; 4]> {
    ensure!(path.last() == Some(&0), "酷狗模块路径未以 NUL 结尾");
    // SAFETY: Toolhelp 提供以 NUL 结尾的模块路径，缓冲区在整个调用期间有效。
    let length = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
    ensure!(length > 0, "无法获取酷狗模块版本信息大小");
    let mut buffer = vec![0u8; length as usize];
    let mut value = std::ptr::null_mut();
    let mut value_length = 0;
    // SAFETY: buffer 分配了 API 要求的字节数，查询结果指向仍存活的 buffer。
    unsafe {
        GetFileVersionInfoW(
            PCWSTR(path.as_ptr()),
            None,
            length,
            buffer.as_mut_ptr().cast(),
        )?;
        VerQueryValueW(
            buffer.as_ptr().cast(),
            w!("\\"),
            &mut value,
            &mut value_length,
        )
        .ok()?;
    }
    ensure!(
        !value.is_null() && value_length as usize >= size_of::<VS_FIXEDFILEINFO>(),
        "酷狗模块版本信息不完整"
    );
    // SAFETY: 已验证结果长度；read_unaligned 不要求字节缓冲区具备结构体对齐。
    let version = unsafe { value.cast::<VS_FIXEDFILEINFO>().read_unaligned() };
    ensure!(version.dwSignature == 0xfeef04bd, "酷狗模块版本信息无效");
    Ok([
        (version.dwFileVersionMS >> 16) as u16,
        version.dwFileVersionMS as u16,
        (version.dwFileVersionLS >> 16) as u16,
        version.dwFileVersionLS as u16,
    ])
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub struct Reader {
    handle: Handle,
    position_base: usize,
    duration_base: usize,
    layouts: &'static [Layout],
    layout: Layout,
}

impl Reader {
    pub fn attach() -> anyhow::Result<Self> {
        let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? });
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        unsafe {
            Process32FirstW(snapshot.0, &mut entry)?;
        }
        let mut failures = Vec::new();
        loop {
            let name = text(&entry.szExeFile);
            if name.eq_ignore_ascii_case("KuGou.exe") || name.eq_ignore_ascii_case("KuGouMusic.exe")
            {
                let candidate = Self::open(entry.th32ProcessID).and_then(|mut reader| {
                    // 同一版本可能有多个已知布局；实际走通时间轴后才采用。
                    let mut errors = Vec::new();
                    for layout in reader.layouts {
                        reader.layout = *layout;
                        match reader.timeline() {
                            Ok(_) => return Ok(reader),
                            Err(error) => errors
                                .push(format!("时长入口 {:#x}: {error:#}", layout.duration_offset)),
                        }
                    }
                    anyhow::bail!("{}", errors.join("；"))
                });
                match candidate {
                    Ok(reader) => return Ok(reader),
                    Err(error) => {
                        failures.push(format!("{name} (PID {}): {error:#}", entry.th32ProcessID))
                    }
                }
            }
            if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }
        ensure!(!failures.is_empty(), "未找到酷狗进程，请启动酷狗并播放歌曲");
        anyhow::bail!("无法读取酷狗播放进度：{}", failures.join("；"))
    }

    fn open(pid: u32) -> anyhow::Result<Self> {
        let snapshot = Handle(unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)
                .context("无法枚举酷狗模块")?
        });
        let mut entry = MODULEENTRY32W {
            dwSize: size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };
        unsafe {
            Module32FirstW(snapshot.0, &mut entry)?;
        }
        let mut position_base = None;
        let mut duration_base = None;
        loop {
            match text(&entry.szModule).to_ascii_lowercase().as_str() {
                "kgplayer.dll" => {
                    position_base =
                        Some((entry.modBaseAddr as usize, file_version(&entry.szExePath)?))
                }
                "kugou.dll" => {
                    duration_base =
                        Some((entry.modBaseAddr as usize, file_version(&entry.szExePath)?))
                }
                _ => {}
            }
            if unsafe { Module32NextW(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }
        let (position_base, position_version) = position_base.context("缺少 kgplayer.dll")?;
        let (duration_base, duration_version) = duration_base.context("缺少 kugou.dll")?;
        let layouts = layouts(position_version, duration_version)?;
        Ok(Self {
            handle: Handle(unsafe {
                // 模块已由 Toolhelp 枚举；读取内存不需要额外申请查询权限。
                OpenProcess(PROCESS_VM_READ, false, pid).context("无法打开酷狗进程进行只读访问")?
            }),
            position_base,
            duration_base,
            layouts,
            layout: layouts[0],
        })
    }

    fn read<const N: usize>(&self, address: usize) -> anyhow::Result<[u8; N]> {
        ensure!(address >= 0x10000, "酷狗内存地址无效");
        let mut bytes = [0; N];
        let mut read = 0;
        // SAFETY: bytes 是 N 字节的有效输出缓冲区，目标进程地址由系统校验。
        unsafe {
            ReadProcessMemory(
                self.handle.0,
                address as _,
                bytes.as_mut_ptr() as _,
                N,
                Some(&mut read),
            )
            .with_context(|| format!("读取酷狗内存失败 (地址 {address:#x}，{N} 字节)"))?;
        }
        ensure!(read == N, "酷狗内存读取不完整");
        Ok(bytes)
    }

    pub fn timeline(&self) -> anyhow::Result<(f64, f64)> {
        let position = f64::from_le_bytes(
            self.read(self.position_base + self.layout.position_offset)
                .context("无法读取 kgplayer.dll 播放进度")?,
        );
        let mut address = self.duration_base + self.layout.duration_offset;
        for offset in self.layout.duration_chain {
            let pointer = u64::from_le_bytes(
                self.read(address)
                    .context("无法读取 kugou.dll 时长指针链")?,
            );
            address = usize::try_from(pointer)?
                .checked_add(*offset)
                .context("酷狗指针溢出")?;
        }
        let duration = f64::from(i32::from_le_bytes(
            self.read(address).context("无法读取酷狗歌曲时长")?,
        ));
        validate_timeline(position, duration / self.layout.duration_units_per_second)
    }
}

fn validate_timeline(position: f64, duration: f64) -> anyhow::Result<(f64, f64)> {
    ensure!(
        position.is_finite()
            && position >= 0.0
            && (0.0..=86400.0).contains(&duration)
            && position <= duration + 2.0,
        "酷狗时间轴数值无效 (进度 {position} 秒，时长 {duration} 秒)"
    );
    Ok((position * 1000.0, duration * 1000.0))
}

fn text(value: &[u16]) -> String {
    String::from_utf16_lossy(
        &value[..value
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(value.len())],
    )
}

#[cfg(test)]
mod tests {
    use super::{file_version, layouts, validate_timeline};
    use std::os::windows::ffi::OsStrExt;

    #[test]
    fn selects_matching_module_versions_and_duration_units() {
        let legacy = layouts([1, 2, 0, 519], [20, 1, 51, 27974]).unwrap();
        let current = layouts([1, 2, 0, 523], [20, 1, 61, 28014]).unwrap();
        for (layout, raw_duration) in [(legacy[0], 209.0), (current[0], 209000.0)] {
            assert_eq!(
                validate_timeline(21.5, raw_duration / layout.duration_units_per_second).unwrap(),
                (21500.0, 209000.0)
            );
        }
        assert!(layouts([1, 2, 0, 519], [20, 1, 61, 28014]).is_err());
        assert!(layouts([1, 2, 0, 523], [20, 1, 62, 28015]).is_err());
    }

    #[test]
    fn reads_fixed_version_from_a_windows_system_module() {
        let path: Vec<u16> = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/kernel32.dll")
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // 未声明 Windows 版本兼容性的测试程序可能读到系统兼容版本 6.x。
        let version = file_version(&path).unwrap();
        assert!(
            version[0] >= 6,
            "unexpected system module version: {version:?}"
        );
    }

    #[test]
    fn reads_legacy_and_current_pointer_chains_from_process_memory() {
        use super::{Handle, LEGACY_LAYOUTS, OpenProcess, PROCESS_VM_READ, Reader, V20161_LAYOUTS};

        for layout in LEGACY_LAYOUTS.iter().chain(V20161_LAYOUTS) {
            let position = Box::new(21.5f64);
            let (&last_offset, offsets) = layout.duration_chain.split_last().unwrap();
            let mut leaf = vec![0u8; last_offset + 4];
            let raw_duration = (209.0 * layout.duration_units_per_second) as i32;
            leaf[last_offset..].copy_from_slice(&raw_duration.to_le_bytes());
            let mut buffers = vec![leaf];
            for offset in offsets.iter().rev() {
                let pointer = buffers.last().unwrap().as_ptr() as u64;
                let mut parent = vec![0u8; offset + 8];
                parent[*offset..].copy_from_slice(&pointer.to_le_bytes());
                buffers.push(parent);
            }
            let root = Box::new(buffers.last().unwrap().as_ptr() as u64);
            let reader = Reader {
                // SAFETY: 只读打开当前测试进程，所有目标缓冲区存活到读取结束。
                handle: Handle(unsafe {
                    OpenProcess(PROCESS_VM_READ, false, std::process::id()).unwrap()
                }),
                position_base: (&*position as *const f64 as usize) - layout.position_offset,
                duration_base: (&*root as *const u64 as usize) - layout.duration_offset,
                layouts: &[],
                layout: *layout,
            };
            assert_eq!(reader.timeline().unwrap(), (21500.0, 209000.0));
        }
    }

    #[test]
    fn accepts_idle_timeline_before_playback_starts() {
        assert_eq!(validate_timeline(0.0, 0.0).unwrap(), (0.0, 0.0));
    }

    #[test]
    fn preserves_subsecond_position_and_track_end_tolerance() {
        assert_eq!(
            validate_timeline(180.5, 180.0).unwrap(),
            (180500.0, 180000.0)
        );
    }

    #[test]
    fn rejects_invalid_memory_values() {
        for (position, duration) in [
            (f64::NAN, 180.0),
            (f64::INFINITY, 180.0),
            (-1.0, 180.0),
            (0.0, -1.0),
            (0.0, 86401.0),
            (190.0, 180.0),
        ] {
            assert!(validate_timeline(position, duration).is_err());
        }
    }
}
