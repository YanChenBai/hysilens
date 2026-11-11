use anyhow::{Context, ensure};
use std::mem::size_of;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND, LPARAM},
        System::{
            Diagnostics::{Debug::ReadProcessMemory, ToolHelp::*},
            Memory::*,
            Threading::*,
        },
        UI::WindowsAndMessaging::*,
    },
    core::BOOL,
};

pub struct Handle(pub HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub struct Process {
    handle: Handle,
    pub pid: u32,
}

pub fn find_process() -> anyhow::Result<Option<u32>> {
    let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? });

    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    unsafe {
        Process32FirstW(snapshot.0, &mut entry)?;
    }

    let mut found = None;

    loop {
        if utf16(&entry.szExeFile).eq_ignore_ascii_case("WeSing.exe") {
            ensure!(
                found.is_none(),
                "Multiple WeSing processes; close extra instances"
            );

            found = Some(entry.th32ProcessID);
        }

        if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
            break;
        }
    }

    Ok(found)
}

impl Process {
    pub fn open(pid: u32) -> anyhow::Result<Self> {
        // 只申请查询和读取权限，不写入、不注入，也不启用调试特权。
        let handle = Handle(
            unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, false, pid) }
                .context("Open WeSing for read-only access")?,
        );

        #[cfg(target_pointer_width = "64")]
        {
            let mut wow64 = BOOL::default();

            unsafe {
                IsWow64Process(handle.0, &mut wow64)?;
            }

            ensure!(
                wow64.as_bool(),
                "Only x86 WeSing is supported by this memory layout"
            );
        }

        Ok(Self { handle, pid })
    }

    pub fn alive(&self) -> bool {
        let mut code = 0;

        unsafe { GetExitCodeProcess(self.handle.0, &mut code).is_ok() && code == 259 }
    }

    pub fn read(&self, address: u32, size: usize) -> anyhow::Result<Vec<u8>> {
        ensure!(
            address >= 0x10000
                && size <= 1024 * 1024 + 128
                && (address as u64 + size as u64) <= 0x1_0000_0000,
            "Invalid x86 memory range"
        );

        let mut bytes = vec![0; size];

        let mut read = 0;

        unsafe {
            ReadProcessMemory(
                self.handle.0,
                address as usize as _,
                bytes.as_mut_ptr() as _,
                size,
                Some(&mut read),
            )
        }
        .with_context(|| format!("Read WeSing memory at {address:#x}"))?;

        ensure!(read == size, "Incomplete memory read");

        Ok(bytes)
    }

    pub fn u32(&self, address: u32) -> anyhow::Result<u32> {
        Ok(u32::from_le_bytes(
            self.read(address, 4)?.try_into().unwrap(),
        ))
    }

    pub fn float(&self, address: u32) -> anyhow::Result<f64> {
        let value = f32::from_bits(self.u32(address)?) as f64;

        ensure!(
            value.is_finite() && (0.0..86400.0).contains(&value),
            "Invalid playback time"
        );

        Ok(value)
    }

    pub fn string(&self, address: u32) -> anyhow::Result<String> {
        let mut units = Vec::new();

        // 逐个读取，避免字符串临近页尾时跨到不可读页，并正确保留代理对。
        for offset in 0..128 {
            let bytes = self.read(address + offset * 2, 2)?;

            let unit = u16::from_le_bytes(bytes.try_into().unwrap());

            if unit == 0 {
                return String::from_utf16(&units).context("Invalid lyric UTF-16");
            }

            units.push(unit);
        }

        anyhow::bail!("Unterminated lyric string")
    }

    pub fn module(&self) -> anyhow::Result<(u32, u32)> {
        let snapshot = Handle(unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, self.pid)?
        });

        let mut entry = MODULEENTRY32W {
            dwSize: size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };

        unsafe {
            Module32FirstW(snapshot.0, &mut entry)?;
        }

        loop {
            if utf16(&entry.szModule).eq_ignore_ascii_case("KSongsLyric.dll") {
                return Ok((
                    u32::try_from(entry.modBaseAddr as usize)?,
                    entry.modBaseSize,
                ));
            }

            if unsafe { Module32NextW(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }

        anyhow::bail!("KSongsLyric.dll is not loaded; open a song with lyrics")
    }

    pub fn scan(&self, pattern: &[Option<u8>]) -> anyhow::Result<Vec<u32>> {
        ensure!(
            !pattern.is_empty() && pattern.len() <= 128,
            "Invalid scan pattern"
        );

        let mut address = 0x10000usize;

        let mut total = 0usize;

        let mut hits = Vec::new();

        while address < 0x1_0000_0000usize {
            let mut info = MEMORY_BASIC_INFORMATION::default();

            if unsafe {
                VirtualQueryEx(
                    self.handle.0,
                    Some(address as _),
                    &mut info,
                    size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            } == 0
            {
                break;
            }

            let base = info.BaseAddress as usize;

            let end = base.saturating_add(info.RegionSize).min(0x1_0000_0000);

            if end <= address {
                break;
            }

            if info.State == MEM_COMMIT
                && (info.Protect == PAGE_READWRITE || info.Protect == PAGE_EXECUTE_READWRITE)
            {
                let mut cursor = base.max(0x10000);

                while cursor < end {
                    let size = (end - cursor).min(1024 * 1024);

                    total += size;

                    ensure!(
                        total <= 512 * 1024 * 1024,
                        "WeSing scan exceeds 512 MiB budget"
                    );

                    if let Ok(bytes) = self.read(cursor as u32, size) {
                        for offset in super::matches(&bytes, pattern) {
                            hits.push((cursor + offset) as u32);

                            ensure!(
                                hits.len() <= 256,
                                "Too many memory candidates; unsupported layout"
                            );
                        }
                    }

                    if cursor + size == end {
                        break;
                    }

                    cursor += size - (pattern.len() - 1);
                }
            }

            address = end;
        }

        Ok(hits)
    }

    pub fn window_title(&self) -> anyhow::Result<String> {
        let mut state = Windows {
            pid: self.pid,
            titles: vec![],
        };

        unsafe {
            EnumWindows(Some(window), LPARAM(&mut state as *mut Windows as isize))?;
        }

        state.titles.sort();

        state.titles.dedup();

        ensure!(
            state.titles.len() <= 1,
            "Multiple singing windows; cannot determine current song"
        );

        Ok(state.titles.pop().unwrap_or_default())
    }
}

struct Windows {
    pid: u32,
    titles: Vec<String>,
}

unsafe extern "system" fn window(hwnd: HWND, param: LPARAM) -> BOOL {
    let state = unsafe { &mut *(param.0 as *mut Windows) };

    let mut pid = 0;

    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }

    if pid == state.pid {
        let mut text = [0u16; 512];

        let len = unsafe { GetWindowTextW(hwnd, &mut text) };

        let title = String::from_utf16_lossy(&text[..len.max(0) as usize]);

        if let Some(song) = title.strip_prefix("全民K歌 - ") {
            state.titles.push(song.trim().to_string());
        }
    }

    BOOL(1)
}

fn utf16(units: &[u16]) -> String {
    String::from_utf16_lossy(
        &units[..units
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units.len())],
    )
}
