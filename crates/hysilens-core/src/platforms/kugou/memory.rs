//! 只读的酷狗进程内存读取器
//!
//! 通过版本相关的 pointer chain 定位播放位置和时长，并用 `ReadProcessMemory` 读取；只会打开
//! 由 Toolhelp snapshot 找到的进程，且每次读取都按其 buffer 做边界检查
//!
//! 播放地址和 pointer chain 改编自 AF Media Bar；MIT 声明见 `THIRD_PARTY_NOTICES.md`

use super::identity;
use crate::platforms::diagnostics::windows::file_version;
use anyhow::{Context, ensure};
use std::mem::size_of;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        Diagnostics::{Debug::ReadProcessMemory, ToolHelp::*},
        Threading::{OpenProcess, PROCESS_VM_READ},
    },
};

#[path = "diagnostics.rs"]
mod diagnostics;
pub use diagnostics::report as diagnostic_report;

/// 一个受支持播放器构建的字节 layout
///
/// 两个偏移都是模块相对的：位置从 `kgplayer.dll` 读取，时长从 `kugou.dll` 读取；
/// `duration_chain` 的遍历方式是读取当前地址处的 64 位 pointer 再加上下一个偏移，
/// `duration_units_per_second` 把最终 `i32` 叶子值换算成秒（计秒时为 `1.0`，计毫秒时为
/// `1000.0`）
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

// kgplayer.dll 1.2.0.523 / kugou.dll 20.1.61.28014：进度为 f64 秒，时长末端为 i32 毫秒
// 短链来自 AF Media Bar 对该模块版本的适配，因此优先；并非遍历更短就一定可靠
// 来源：https://github.com/Fervent-Tempo/AF-Media-Bar/blob/8e0564f91e10541eed321681ddf72da024e3fa6f/src/AFMediaBar/Classes/Services/Media/Sources/KuGou/KuGou.cs
const V20161_LAYOUT: Layout = Layout {
    position_offset: 0x3949f8,
    duration_offset: 0x02434b30,
    duration_chain: &[0x108, 0x8c],
    duration_units_per_second: 1000.0,
};

const V20161_LAYOUTS: &[Layout] = &[
    V20161_LAYOUT,
    Layout {
        // 长链曾在本机读到正确值，保留以兼容短链读失败或校验失败的运行状态；
        // 不把它当作第二份独立确认，也不因一次成功就切换为永久首选
        // 异常报告中两条链都曾落到堆指针的高 32 位，读出 528/563 等假毫秒值，
        // 即使 DLL 版本和 SHA-256 相同也会发生；所有链都必须逐次校验
        duration_chain: &[0x130, 0x130, 0x2a4],
        ..V20161_LAYOUT
    },
    Layout {
        // 本机切歌和完整重启后验证的兜底入口，末端为 i32 毫秒
        // 仅在前两条链读取或校验失败时尝试，不缓存堆地址或上一首时长
        duration_offset: 0x024363d8,
        duration_chain: &[0x30, 0x118],
        ..V20161_LAYOUT
    },
];

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

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: `Handle` 持有该 HANDLE 的唯一副本，因此它只会被关闭一次，
        // 之后任何调用方都无法再使用它
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// 存活酷狗进程的只读视图
///
/// 持有 process handle（在 drop 时关闭），以及在 attach 时捕获的 module base；这些 base
/// 属于单个进程生命周期，因此重启后的播放器需要重新 [`Reader::attach`]
pub struct Reader {
    handle: Handle,

    position_base: usize,

    duration_base: usize,

    layouts: &'static [Layout],

    layout: Layout,

    identity_entry: Result<identity::Entry, String>,
}

impl Reader {
    pub fn attach() -> anyhow::Result<Self> {
        // SAFETY: 该调用不接收 pointer，返回的新 snapshot HANDLE 由 `Handle` 持有；
        // 后续失败会 drop 该包装并关闭它
        let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? });
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        // SAFETY: `entry` 是尺寸正确且已初始化 `dwSize` 的 PROCESSENTRY32W，
        // snapshot 在整个枚举期间保持打开
        unsafe {
            Process32FirstW(snapshot.0, &mut entry)?;
        }

        let mut failures = Vec::new();

        loop {
            let name = text(&entry.szExeFile);

            if name.eq_ignore_ascii_case("KuGou.exe") || name.eq_ignore_ascii_case("KuGouMusic.exe")
            {
                let candidate = Self::open(entry.th32ProcessID).and_then(|mut reader| {
                    reader.select_layout()?;

                    Ok(reader)
                });

                match candidate {
                    Ok(reader) => return Ok(reader),
                    Err(error) => {
                        failures.push(format!("{name} (PID {}): {error:#}", entry.th32ProcessID))
                    }
                }
            }

            // SAFETY: 在同一个存活的 snapshot 上继续遍历，`entry` 保持其 `dwSize`
            if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }
        ensure!(!failures.is_empty(), "未找到酷狗进程，请启动酷狗并播放歌曲");
        anyhow::bail!("无法读取酷狗播放进度：{}", failures.join("；"))
    }

    fn select_layout(&mut self) -> anyhow::Result<()> {
        // 进度与时长位于不同 DLL，时长链失败不能证明进度地址失效
        // 连接只要求进度有效；保留读句柄，允许后续采样在时长恢复后自行读到有效值
        let position = self.position()?;

        for layout in self.layouts {
            if self
                .duration(*layout)
                .and_then(|duration| validate_timeline(position, duration))
                .is_ok()
            {
                self.layout = *layout;
                break;
            }
        }

        Ok(())
    }

    fn open(pid: u32) -> anyhow::Result<Self> {
        // SAFETY: CreateToolhelp32Snapshot 不接收 pointer；返回的 HANDLE 由 `Handle` 持有，
        // 该函数返回或失败时它会被关闭
        let snapshot = Handle(unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)
                .context("无法枚举酷狗模块")?
        });

        let mut entry = MODULEENTRY32W {
            dwSize: size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };

        // SAFETY: `entry` 是尺寸正确且已初始化 `dwSize` 的 MODULEENTRY32W，
        // snapshot 在整个枚举期间保持打开
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
                    duration_base = Some((
                        entry.modBaseAddr as usize,
                        file_version(&entry.szExePath)?,
                        entry.modBaseSize as usize,
                    ))
                }
                _ => {}
            }

            // SAFETY: 在存活的 snapshot 上继续模块遍历，`entry` 仍带有其 `dwSize`
            if unsafe { Module32NextW(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }

        let (position_base, position_version) = position_base.context("缺少 kgplayer.dll")?;
        let (duration_base, duration_version, duration_size) =
            duration_base.context("缺少 kugou.dll")?;

        let layouts = layouts(position_version, duration_version)?;
        let mut reader = Self {
            // SAFETY: OpenProcess 不接收 pointer；它校验 `pid` 并返回自有 HANDLE 或错误，
            // 成功时的值由 `Handle` 在 drop 时关闭
            handle: Handle(unsafe {
                // 模块已由 Toolhelp 枚举；读取内存不需要额外申请查询权限
                OpenProcess(PROCESS_VM_READ, false, pid).context("无法打开酷狗进程进行只读访问")?
            }),
            position_base,
            duration_base,
            layouts,
            layout: layouts[0],
            identity_entry: Err("酷狗身份入口尚未定位".into()),
        };
        reader.prepare_identity(duration_version, duration_size);

        Ok(reader)
    }

    fn prepare_identity(&mut self, version: [u16; 4], size: usize) {
        // 身份失败不能让进度/时长或更早的受支持构建失效
        self.identity_entry = if version == [20, 1, 61, 28014] {
            identity::Entry::discover(self, self.duration_base, size)
                .map_err(|error| format!("{error:#}"))
        } else {
            Err(format!("酷狗内存歌曲身份尚未适配 ({version:?})"))
        };
    }

    pub fn identity(&self) -> anyhow::Result<super::KugouIdentity> {
        let entry = self
            .identity_entry
            .as_ref()
            .map_err(|message| anyhow::anyhow!("{message}"))?;
        entry.read(self)
    }

    fn read<const N: usize>(&self, address: usize) -> anyhow::Result<[u8; N]> {
        let mut bytes = [0; N];
        self.read_into(address, &mut bytes)?;

        Ok(bytes)
    }

    pub(super) fn read_into(&self, address: usize, bytes: &mut [u8]) -> anyhow::Result<()> {
        let length = bytes.len();
        ensure!(
            address >= 0x10000
                && address
                    .checked_add(length)
                    .is_some_and(|end| (end as u64) <= 0x0000_8000_0000_0000),
            "酷狗内存地址无效"
        );

        let mut read = 0;

        // SAFETY: bytes 是 length 字节的有效输出缓冲区，目标进程地址由系统校验
        unsafe {
            ReadProcessMemory(
                self.handle.0,
                address as _,
                bytes.as_mut_ptr() as _,
                length,
                Some(&mut read),
            )
            .with_context(|| format!("读取酷狗内存失败 (地址 {address:#x}，{length} 字节)"))?;
        }
        ensure!(read == length, "酷狗内存读取不完整");

        Ok(())
    }

    fn position(&self) -> anyhow::Result<f64> {
        let position = f64::from_le_bytes(
            self.read(
                self.position_base
                    .checked_add(self.layout.position_offset)
                    .context("酷狗进度地址溢出")?,
            )
            .context("无法读取 kgplayer.dll 播放进度")?,
        );
        validate_position(position)?;

        Ok(position)
    }

    fn duration(&self, layout: Layout) -> anyhow::Result<f64> {
        // 从 kugou.dll 的模块相对入口开始，每一级先读 64 位指针再加偏移；
        // 最后地址保存的是 i32 数值，不再解引用；这里的偏移顺序与 CE XML 的存储顺序相反
        let mut address = self
            .duration_base
            .checked_add(layout.duration_offset)
            .context("酷狗时长地址溢出")?;

        for offset in layout.duration_chain {
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

        Ok(duration / layout.duration_units_per_second)
    }

    /// 返回毫秒进度和可选毫秒时长；仅进度读取失败会使整个采样失败
    pub fn timeline(&self) -> anyhow::Result<(f64, Option<f64>)> {
        let position = self.position()?;
        let layouts = if self.layouts.is_empty() {
            std::slice::from_ref(&self.layout)
        } else {
            self.layouts
        };

        // 每次重新从短链开始，失败再按顺序尝试兜底链；不缓存上次时长或永久选中成功的链，
        // 避免切歌后沿用旧歌时长；全部失败返回 None，不能用假时长裁剪有效进度
        // 原始数值、逐级读取错误与各条链的采样仍由按需诊断报告完整保留
        let duration = layouts.iter().find_map(|layout| {
            self.duration(*layout)
                .and_then(|duration| validate_timeline(position, duration))
                .ok()
                .map(|(_, duration)| duration)
                .filter(|duration| *duration > 0.0 && *duration >= position * 1000.0)
        });

        Ok((position * 1000.0, duration))
    }
}

fn validate_position(position: f64) -> anyhow::Result<()> {
    ensure!(
        position.is_finite() && (0.0..=86402.0).contains(&position),
        "酷狗播放进度数值无效 (进度 {position} 秒)"
    );

    Ok(())
}

fn validate_timeline(position: f64, duration: f64) -> anyhow::Result<(f64, f64)> {
    // 这只是数值合理性检查，不是歌曲身份或地址正确性的证明；比如 563 毫秒的误读
    // 在歌曲刚开始时仍可能暂时通过；不能据此认定两条链在同版本的所有电脑上都可靠
    // 这里允许切换状态下的两秒边界偏差；timeline() 发布时长时还要求它不小于实际进度
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

        // 未声明 Windows 版本兼容性的测试程序可能读到系统兼容版本 6.x
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
                identity_entry: Err("test".into()),
                // SAFETY: 只读打开当前测试进程，所有目标缓冲区存活到读取结束
                handle: Handle(unsafe {
                    OpenProcess(PROCESS_VM_READ, false, std::process::id()).unwrap()
                }),
                position_base: (&*position as *const f64 as usize) - layout.position_offset,
                duration_base: (&*root as *const u64 as usize) - layout.duration_offset,
                layouts: &[],
                layout: *layout,
            };
            assert_eq!(reader.timeline().unwrap(), (21500.0, Some(209000.0)));
        }
    }

    #[test]
    fn prefers_short_chain_and_falls_back_after_read_or_validation_failure() {
        use super::{Handle, OpenProcess, PROCESS_VM_READ, Reader, V20161_LAYOUTS};

        for (preferred, fallback, expected) in [
            (Some(210000i32), Some(209000i32), Some((0, 210000.0))),
            (Some(528), Some(209000), Some((1, 209000.0))),
            (None, Some(209000), Some((1, 209000.0))),
            (Some(528), Some(563), None),
            (None, None, None),
        ] {
            let position = Box::new(21.5f64);
            let mut short_leaf = [0u8; 0x90];
            short_leaf[0x8c..].copy_from_slice(&preferred.unwrap_or_default().to_le_bytes());

            let mut long_leaf = [0u8; 0x2a8];
            long_leaf[0x2a4..].copy_from_slice(&fallback.unwrap_or_default().to_le_bytes());

            let mut middle = [0u8; 0x138];
            middle[0x130..].copy_from_slice(&(long_leaf.as_ptr() as u64).to_le_bytes());

            let mut shared = [0u8; 0x138];

            if preferred.is_some() {
                shared[0x108..0x110].copy_from_slice(&(short_leaf.as_ptr() as u64).to_le_bytes());
            }

            if fallback.is_some() {
                shared[0x130..].copy_from_slice(&(middle.as_ptr() as u64).to_le_bytes());
            }

            let root = Box::new(shared.as_ptr() as u64);
            let mut reader = Reader {
                identity_entry: Err("test".into()),
                // SAFETY: 所有目标分配都属于本测试，且比这些读取存活更久
                handle: Handle(unsafe {
                    OpenProcess(PROCESS_VM_READ, false, std::process::id()).unwrap()
                }),
                position_base: (&*position as *const f64 as usize)
                    - V20161_LAYOUTS[0].position_offset,
                duration_base: (&*root as *const u64 as usize) - V20161_LAYOUTS[0].duration_offset,
                layouts: &V20161_LAYOUTS[..2],
                layout: V20161_LAYOUTS[0],
            };

            let result = reader.select_layout();

            if let Some((index, duration)) = expected {
                result.unwrap();
                assert_eq!(
                    reader.layout.duration_chain,
                    V20161_LAYOUTS[index].duration_chain
                );
                assert_eq!(reader.timeline().unwrap(), (21500.0, Some(duration)));
            } else {
                result.unwrap();
                assert_eq!(reader.timeline().unwrap(), (21500.0, None));
            }

            // 切歌后不重新 attach 也可能重新出现有效时长
            if preferred.is_some() {
                short_leaf[0x8c..].copy_from_slice(&211000i32.to_le_bytes());
                assert_eq!(reader.timeline().unwrap(), (21500.0, Some(211000.0)));
            }
        }
    }

    #[test]
    fn uses_third_chain_without_caching_duration_or_overriding_valid_preferred_chain() {
        use super::{Handle, OpenProcess, PROCESS_VM_READ, Reader, V20161_LAYOUTS};

        let position = Box::new(21.5f64);
        let fallback = V20161_LAYOUTS[2];
        let root_offset = fallback.duration_offset - V20161_LAYOUTS[0].duration_offset;
        let mut roots = vec![0u8; root_offset + 8];
        let mut fallback_leaf = [0u8; 0x11c];
        let mut fallback_parent = [0u8; 0x38];
        fallback_parent[0x30..].copy_from_slice(&(fallback_leaf.as_ptr() as u64).to_le_bytes());
        roots[root_offset..].copy_from_slice(&(fallback_parent.as_ptr() as u64).to_le_bytes());

        let mut reader = Reader {
            identity_entry: Err("test".into()),
            // SAFETY: 所有指针均指向当前测试进程内仍存活的缓冲区，只申请读取权限
            handle: Handle(unsafe {
                OpenProcess(PROCESS_VM_READ, false, std::process::id()).unwrap()
            }),
            position_base: (&*position as *const f64 as usize) - V20161_LAYOUTS[0].position_offset,
            duration_base: roots.as_ptr() as usize - V20161_LAYOUTS[0].duration_offset,
            layouts: V20161_LAYOUTS,
            layout: V20161_LAYOUTS[0],
        };

        for (raw, expected) in [
            (308427i32, Some(308427.0)),
            (312946, Some(312946.0)),
            (0, None),
            (1, None),
            (-1, None),
            (21400, None),
            (86400001, None),
            (261276, Some(261276.0)),
        ] {
            fallback_leaf[0x118..].copy_from_slice(&raw.to_le_bytes());

            assert_eq!(reader.timeline().unwrap(), (21500.0, expected));
        }

        reader.select_layout().unwrap();
        assert_eq!(reader.layout.duration_offset, fallback.duration_offset);

        let mut short_leaf = [0u8; 0x90];
        short_leaf[0x8c..].copy_from_slice(&210000i32.to_le_bytes());
        let mut short_parent = [0u8; 0x110];
        short_parent[0x108..].copy_from_slice(&(short_leaf.as_ptr() as u64).to_le_bytes());
        roots[..8].copy_from_slice(&(short_parent.as_ptr() as u64).to_le_bytes());

        assert_eq!(reader.timeline().unwrap(), (21500.0, Some(210000.0)));

        short_leaf[0x8c..].copy_from_slice(&1i32.to_le_bytes());

        assert_eq!(reader.timeline().unwrap(), (21500.0, Some(261276.0)));
    }

    #[test]
    fn keeps_reading_position_without_duration_but_rejects_invalid_position() {
        use super::{Handle, OpenProcess, PROCESS_VM_READ, Reader, V20161_LAYOUTS};

        let mut position = Box::new(123.548812f64);
        let root = Box::new(0u64);
        let mut reader = Reader {
            identity_entry: Err("test".into()),
            // SAFETY: position/root 分配在所有只读探测期间保持存活
            handle: Handle(unsafe {
                OpenProcess(PROCESS_VM_READ, false, std::process::id()).unwrap()
            }),
            position_base: (&*position as *const f64 as usize) - V20161_LAYOUTS[0].position_offset,
            duration_base: (&*root as *const u64 as usize) - V20161_LAYOUTS[0].duration_offset,
            layouts: &V20161_LAYOUTS[..2],
            layout: V20161_LAYOUTS[0],
        };
        reader.select_layout().unwrap();
        assert_eq!(reader.timeline().unwrap(), (123548.812, None));
        *position = 126.548812;
        assert_eq!(reader.timeline().unwrap(), (126548.812, None));

        for invalid in [f64::NAN, f64::INFINITY, -1.0, 86403.0] {
            *position = invalid;
            assert!(reader.select_layout().is_err());
            assert!(reader.timeline().is_err());
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
