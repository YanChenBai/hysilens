//! 只读的 WeSing 进程内存读取器
//!
//! 通过扫描 `WeSing.exe`（仅 x86）已加载的模块来定位歌词与 playback 结构，并用
//! `ReadProcessMemory` 读取；每次访问都按查询到的区域做边界检查

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
        // SAFETY: 构造上 `Handle` 是唯一所有者（没有 `Copy`/`Clone`），原始字段从未被复制，
        // 因此该 HANDLE 只会被关闭一次
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

    let mut found = None;

    loop {
        if utf16(&entry.szExeFile).eq_ignore_ascii_case("WeSing.exe") {
            ensure!(
                found.is_none(),
                "Multiple WeSing processes; close extra instances"
            );

            found = Some(entry.th32ProcessID);
        }

        // SAFETY: 在同一个存活的 snapshot 上继续遍历，`entry` 保持其 `dwSize`
        if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
            break;
        }
    }

    Ok(found)
}

impl Process {
    pub fn open(pid: u32) -> anyhow::Result<Self> {
        // 只申请查询和读取权限，不写入、不注入，也不启用调试特权
        // SAFETY: OpenProcess 不接收 pointer；无效或无权访问的 `pid` 会报告为错误，
        // 成功时的值由 `Handle` 持有，并在 drop 时关闭
        let handle = Handle(
            unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, false, pid) }
                .context("Open WeSing for read-only access")?,
        );

        #[cfg(target_pointer_width = "64")]
        {
            let mut wow64 = BOOL::default();

            // SAFETY: `handle` 存活并由正在构建的 `Process` 持有，`wow64` 是有效的 BOOL
            // 输出缓冲区
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

        // SAFETY: `self.handle` 在 `self` 的整个生命周期内保持打开，`code` 是有效的 DWORD
        // 输出缓冲区
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

        // SAFETY: 范围已在上文做过边界检查，`bytes` 恰好有 `size` 个可写字节；
        // Windows 会校验该远端地址可读
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

        // 逐个读取，避免字符串临近页尾时跨到不可读页，并正确保留代理对
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
        // SAFETY: CreateToolhelp32Snapshot 不接收 pointer；返回的 HANDLE 由 `Handle` 持有，
        // 该函数返回或失败时它会被关闭
        let snapshot = Handle(unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, self.pid)?
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

        loop {
            if utf16(&entry.szModule).eq_ignore_ascii_case("KSongsLyric.dll") {
                return Ok((
                    u32::try_from(entry.modBaseAddr as usize)?,
                    entry.modBaseSize,
                ));
            }

            // SAFETY: 在存活的 snapshot 上继续模块遍历，`entry` 仍带有其 `dwSize`
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

            // SAFETY: `info` 是有效的 MEMORY_BASIC_INFORMATION 输出缓冲区，
            // 查询的地址由操作系统校验
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

        // SAFETY: 回调通过 `LPARAM` 收到 `&mut state`，且 `EnumWindows` 只在本次调用期间
        // 同步调用它
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
    // SAFETY: `param` 是本模块中 `EnumWindows` 构造的 `LPARAM`，因此它指向一个比回调
    // 存活更久的 `Windows`
    let state = unsafe { &mut *(param.0 as *mut Windows) };
    let mut pid = 0;

    // SAFETY: `hwnd` 来自 `EnumWindows`，`pid` 是有效的 DWORD 输出缓冲区
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }

    if pid == state.pid {
        let mut text = [0u16; 512];

        // SAFETY: `text` 是 512 单元的缓冲区，`hwnd` 是当前正在枚举的窗口
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
