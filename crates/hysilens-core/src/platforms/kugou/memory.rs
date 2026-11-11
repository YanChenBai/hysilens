// 播放地址与指针链改编自 AF Media Bar；MIT 版权声明见 THIRD_PARTY_NOTICES.md。
use anyhow::{Context, ensure};
use std::mem::size_of;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        Diagnostics::{Debug::ReadProcessMemory, ToolHelp::*},
        Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
    },
};

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
        loop {
            let name = text(&entry.szExeFile);
            if (name.eq_ignore_ascii_case("KuGou.exe")
                || name.eq_ignore_ascii_case("KuGouMusic.exe"))
                && let Ok(reader) = Self::open(entry.th32ProcessID)
                && reader.timeline().is_ok()
            {
                return Ok(reader);
            }
            if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }
        anyhow::bail!("无法读取酷狗播放进度：权限不足或客户端版本不支持")
    }

    fn open(pid: u32) -> anyhow::Result<Self> {
        let snapshot = Handle(unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)?
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
                "kgplayer.dll" => position_base = Some(entry.modBaseAddr as usize),
                "kugou.dll" => duration_base = Some(entry.modBaseAddr as usize),
                _ => {}
            }
            if unsafe { Module32NextW(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }
        Ok(Self {
            handle: Handle(unsafe {
                OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, false, pid)?
            }),
            position_base: position_base.context("缺少 kgplayer.dll")?,
            duration_base: duration_base.context("缺少 kugou.dll")?,
        })
    }

    fn read<const N: usize>(&self, address: usize) -> anyhow::Result<[u8; N]> {
        ensure!(address >= 0x10000, "酷狗内存地址无效");
        let mut bytes = [0; N];
        let mut read = 0;
        unsafe {
            ReadProcessMemory(
                self.handle.0,
                address as _,
                bytes.as_mut_ptr() as _,
                N,
                Some(&mut read),
            )?;
        }
        ensure!(read == N, "酷狗内存读取不完整");
        Ok(bytes)
    }

    pub fn timeline(&self) -> anyhow::Result<(f64, f64)> {
        // 此组偏移已在 20.1.51.27967 实测；模块升级后必须重新验证。
        let position = f64::from_le_bytes(self.read(self.position_base + 0x3929d8)?);
        let mut address = self.duration_base + 0x023c9b68;
        for offset in [0x70, 0x30, 0, 0x24] {
            let pointer = u64::from_le_bytes(self.read(address)?);
            address = usize::try_from(pointer)?
                .checked_add(offset)
                .context("酷狗指针溢出")?;
        }
        let duration = f64::from(i32::from_le_bytes(self.read(address)?));
        ensure!(
            position.is_finite()
                && position >= 0.0
                && duration > 0.0
                && duration <= 86400.0
                && position <= duration + 2.0,
            "酷狗时间轴数值无效"
        );
        Ok((position * 1000.0, duration * 1000.0))
    }
}

fn text(value: &[u16]) -> String {
    String::from_utf16_lossy(
        &value[..value
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(value.len())],
    )
}
