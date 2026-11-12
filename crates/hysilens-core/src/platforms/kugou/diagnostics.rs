//! 按需触发的只读诊断；身份 AOB 只扫描可执行 module section，不会执行完整进程 dump

use super::*;
use crate::platforms::qq_music;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{BufReader, Read},
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows::Win32::System::{
    Memory::{MEMORY_BASIC_INFORMATION, VirtualQueryEx},
    Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW},
    SystemInformation::{
        GetNativeSystemInfo, GlobalMemoryStatusEx, IMAGE_FILE_MACHINE, MEMORYSTATUSEX, SYSTEM_INFO,
    },
    Threading::{IsWow64Process2, PROCESS_QUERY_INFORMATION},
};
use windows::core::{PCWSTR, w};

const SAMPLE_COUNT: usize = 13;
const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);
const CONTEXT_BEFORE: usize = 64;
const CONTEXT_AFTER: usize = 64;

struct DiagnosticMedia(Option<qq_music::Probe>);

impl Drop for DiagnosticMedia {
    fn drop(&mut self) {
        if let Some(probe) = self.0.take() {
            drop(probe);

            // SAFETY: 配平 `Probe::new` 在同一 worker thread 上成功调用的 `CoInitializeEx`，
            // 后者递增了线程的 COM 初始化计数（线程此前已初始化时为 `S_FALSE`）；probe 及其
            // WinRT manager 已在上方 drop，因此没有 WinRT 对象存活到这次调用之后
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

fn application_info() -> anyhow::Result<Value> {
    let path = std::env::current_exe()?;
    let metadata = std::fs::metadata(&path)?;

    Ok(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "executablePath": path,
        "sha256": result_value(sha256(&path)),
        "fileSize": metadata.len(),
        "modifiedUnixMs": metadata.modified().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map(|time| time.as_millis()),
        "debugBuild": cfg!(debug_assertions),
    }))
}

fn media_snapshot(probe: Option<&qq_music::Probe>) -> Value {
    probe.map_or(Value::Null, |probe| {
        let mut playback = probe.sample();
        playback
            .sessions
            .retain(|s| s.source_id.eq_ignore_ascii_case("kugou"));
        json!(playback)
    })
}

fn registry_string(key: PCWSTR, name: PCWSTR) -> anyhow::Result<String> {
    // 取到的值是有长度上限的 Windows/CPU 描述，不是任意 registry 内容
    let mut buffer = [0u16; 1024];
    let mut length = size_of::<[u16; 1024]>() as u32;

    // SAFETY: 两个 registry 名称都是静态 NUL 结尾字符串，buffer 可写
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key,
            name,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut length),
        )
        .ok()?;
    }

    Ok(text(&buffer))
}

fn system_info() -> Value {
    let windows_key = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    let cpu_key = w!("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0");
    let mut info = SYSTEM_INFO::default();
    let mut memory = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };

    // SAFETY: `info` 是已完全初始化、可写的 SYSTEM_INFO 输出 buffer，
    // GetNativeSystemInfo 不需要调用方提供大小
    unsafe { GetNativeSystemInfo(&mut info) };

    // SAFETY: `memory` 是存活的 MEMORYSTATUSEX，`dwLength` 已设为自身大小，
    // 调用只写入该 buffer
    let memory = unsafe { GlobalMemoryStatusEx(&mut memory) }
        .map(|()| {
            json!({
                "totalPhysicalBytes": memory.ullTotalPhys,
                "availablePhysicalBytes": memory.ullAvailPhys,
                "loadPercent": memory.dwMemoryLoad,
            })
        })
        .map_err(Into::into);

    let revision = (|| -> anyhow::Result<u32> {
        let mut revision = 0u32;
        let mut length = size_of::<u32>() as u32;

        // SAFETY: DWORD 值被读入大小正确且存活的输出 buffer
        unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                windows_key,
                w!("UBR"),
                RRF_RT_REG_DWORD,
                None,
                Some((&mut revision as *mut u32).cast()),
                Some(&mut length),
            )
            .ok()?;
        }

        Ok(revision)
    })();
    json!({
        "windows": {
            "productName": result_value(registry_string(windows_key, w!("ProductName"))),
            "displayVersion": result_value(registry_string(windows_key, w!("DisplayVersion"))),
            "editionId": result_value(registry_string(windows_key, w!("EditionID"))),
            "buildNumber": result_value(registry_string(windows_key, w!("CurrentBuildNumber"))),
            "buildRevision": result_value(revision),
            "buildLabEx": result_value(registry_string(windows_key, w!("BuildLabEx"))),
        },
        "cpu": {
            "name": result_value(registry_string(cpu_key, w!("ProcessorNameString"))),
            "identifier": result_value(registry_string(cpu_key, w!("Identifier"))),
            "vendor": result_value(registry_string(cpu_key, w!("VendorIdentifier"))),
            "logicalProcessorCount": std::thread::available_parallelism().map(|n| n.get()).ok(),
            "systemProcessorCount": info.dwNumberOfProcessors,
        },
        // SAFETY: GetNativeSystemInfo 已初始化该 union 的 architecture 成员
        "nativeArchitectureCode": unsafe { info.Anonymous.Anonymous.wProcessorArchitecture.0 },
        "pageSize": info.dwPageSize,
        "memory": result_value(memory),
    })
}

fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn result_value<T: serde::Serialize>(result: anyhow::Result<T>) -> Value {
    match result {
        Ok(value) => json!({ "value": value }),
        Err(error) => json!({ "error": format!("{error:#}") }),
    }
}

fn sha256(path: &std::path::Path) -> anyhow::Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];

    loop {
        let count = reader.read(&mut buffer)?;

        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }

    Ok(format!("{:x}", hash.finalize()))
}

struct Module {
    name: String,

    base: usize,

    size: u32,

    version: Option<[u16; 4]>,

    report: Value,
}

fn modules(pid: u32) -> anyhow::Result<Vec<Module>> {
    // SAFETY: CreateToolhelp32Snapshot 不接受 pointer，返回的 HANDLE 归 `Handle` 所有，
    // 由其在 drop 时关闭
    let snapshot =
        Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)? });

    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };

    // SAFETY: `entry` 是大小正确、`dwSize` 已初始化的 MODULEENTRY32W，snapshot 在整个枚举
    // 期间保持打开
    unsafe { Module32FirstW(snapshot.0, &mut entry)? };

    let mut modules = Vec::new();

    loop {
        let name = text(&entry.szModule);

        if matches!(
            name.to_ascii_lowercase().as_str(),
            "kgplayer.dll" | "kugou.dll" | "kugou.exe" | "kugoumusic.exe"
        ) {
            let length = entry
                .szExePath
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExePath.len());

            let path = PathBuf::from(std::ffi::OsString::from_wide(&entry.szExePath[..length]));
            let version = file_version(&entry.szExePath);
            let report = json!({
                "name": name,
                "path": path,
                "base": format!("{:#x}", entry.modBaseAddr as usize),
                "size": entry.modBaseSize,
                "fileVersion": result_value(version.as_ref().copied().map_err(|e| anyhow::anyhow!("{e:#}"))),
                "fileVersionString": version.as_ref().ok().map(|v| v.iter().map(u16::to_string).collect::<Vec<_>>().join(".")),
                "sha256": result_value(sha256(&path)),
            });
            modules.push(Module {
                name,
                base: entry.modBaseAddr as usize,
                size: entry.modBaseSize,
                version: version.ok(),
                report,
            });
        }

        // SAFETY: snapshot 与输出 entry 在整个枚举期间均有效
        if let Err(error) = unsafe { Module32NextW(snapshot.0, &mut entry) } {
            ensure!(
                error.code().0 as u32 == 0x80070012,
                "枚举酷狗模块中断: {error}"
            );
            break;
        }
    }

    Ok(modules)
}

fn region(handle: HANDLE, address: usize) -> Value {
    let mut info = MEMORY_BASIC_INFORMATION::default();

    // SAFETY: info 是有效的输出 buffer，远程地址由操作系统校验
    let count = unsafe {
        VirtualQueryEx(
            handle,
            Some(address as _),
            &mut info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };

    if count == 0 {
        return json!({ "error": windows::core::Error::from_thread().to_string() });
    }
    json!({
        "base": format!("{:#x}", info.BaseAddress as usize),
        "allocationBase": format!("{:#x}", info.AllocationBase as usize),
        "size": info.RegionSize,
        "state": format!("{:#x}", info.State.0),
        "protect": format!("{:#x}", info.Protect.0),
        "allocationProtect": format!("{:#x}", info.AllocationProtect.0),
        "type": format!("{:#x}", info.Type.0),
        "nearby": nearby(handle, address, &info),
    })
}

fn nearby(handle: HANDLE, address: usize, info: &MEMORY_BASIC_INFORMATION) -> Value {
    // 停留在已查询区域内并避开 guard/no-access page；这是围绕已知地址的有界上下文
    // 读取，从不是全进程扫描
    if info.State.0 != 0x1000 || info.Protect.0 & (0x100 | 0x01) != 0 {
        return json!({ "skipped": "内存未提交或受保护" });
    }

    let start = address
        .saturating_sub(CONTEXT_BEFORE)
        .max(info.BaseAddress as usize);

    let end = address
        .saturating_add(CONTEXT_AFTER)
        .min((info.BaseAddress as usize).saturating_add(info.RegionSize));

    let length = end.saturating_sub(start);

    if length == 0 || length > CONTEXT_BEFORE + CONTEXT_AFTER {
        return json!({ "skipped": "内存区域范围无效" });
    }

    let mut bytes = vec![0u8; length];
    let mut count = 0;

    // SAFETY: bytes 有 length 个可写字节，远程地址由 Windows 校验
    let result = unsafe {
        ReadProcessMemory(
            handle,
            start as _,
            bytes.as_mut_ptr().cast(),
            length,
            Some(&mut count),
        )
    };
    bytes.truncate(count.min(length));

    let mut report = json!({
        "address": format!("{start:#x}"),
        "targetOffset": address.saturating_sub(start),
        "requestedBytes": length,
        "readBytes": bytes.len(),
        "bytesHex": bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
    });

    if let Err(error) = result {
        report["error"] = json!(error.to_string());
    }
    report
}

fn read_value<const N: usize>(reader: &Reader, address: usize) -> (Value, anyhow::Result<[u8; N]>) {
    let result = reader.read(address);
    let mut report = json!({ "address": format!("{address:#x}"), "byteCount": N, "region": region(reader.handle.0, address) });

    match &result {
        Ok(bytes) => {
            report["bytesHex"] = json!(bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
        }
        Err(error) => {
            report["error"] = json!(format!("{error:#}"));

            if let Some(code) = error.chain().find_map(|cause| {
                cause
                    .downcast_ref::<windows::core::Error>()
                    .map(|error| error.code())
            }) {
                report["hresult"] = json!(format!("0x{:08x}", code.0 as u32));
            }
        }
    }
    (report, result)
}

fn sample(reader: &Reader) -> Value {
    let started = timestamp();
    let layout = reader.layout;
    let (mut position_report, position) =
        read_value(reader, reader.position_base + layout.position_offset);

    let position = position.map(f64::from_le_bytes);
    let progress_usable = position
        .as_ref()
        .is_ok_and(|position| validate_position(*position).is_ok());

    if let Ok(value) = &position {
        // JSON 无法表示 NaN/Infinity，因此除原始字节外还保留其文本形式
        position_report["rawSeconds"] = json!(value.to_string());
        position_report["seconds"] = json!(value);
    }

    let mut steps = Vec::new();
    let mut leaf_report = Value::Null;
    let duration = (|| -> anyhow::Result<f64> {
        let mut address = reader
            .duration_base
            .checked_add(layout.duration_offset)
            .context("时长入口溢出")?;

        for (index, offset) in layout.duration_chain.iter().enumerate() {
            let (mut step, pointer) = read_value(reader, address);
            step["index"] = json!(index);
            step["offset"] = json!(format!("{offset:#x}"));

            let next = pointer.and_then(|bytes| {
                let pointer = u64::from_le_bytes(bytes);
                step["pointer"] = json!(format!("{pointer:#x}"));
                usize::try_from(pointer)?
                    .checked_add(*offset)
                    .context("酷狗指针溢出")
            });

            match &next {
                Ok(next) => step["nextAddress"] = json!(format!("{next:#x}")),
                Err(error) => step["error"] = json!(format!("{error:#}")),
            }
            steps.push(step);
            address = next?;
        }

        let (mut leaf, duration) = read_value(reader, address);
        let duration = duration.map(i32::from_le_bytes);

        if let Ok(raw) = &duration {
            leaf["rawValue"] = json!(raw);
            leaf["seconds"] = json!(f64::from(*raw) / layout.duration_units_per_second);
        }
        leaf_report = leaf;
        duration.map(|raw| f64::from(raw) / layout.duration_units_per_second)
    })();

    let mut report = json!({
        "capturedUnixMs": timestamp(),
        "startedUnixMs": started,
        "progressUsable": progress_usable,
        "position": position_report,
        "durationSteps": steps,
        "duration": leaf_report,
    });

    match duration {
        Ok(seconds) => {
            report["timeline"] =
                result_value(position.and_then(|position| validate_timeline(position, seconds)));
        }
        Err(error) => report["durationError"] = json!(format!("{error:#}")),
    }
    report
}

fn process(pid: u32, media: Option<&qq_music::Probe>) -> anyhow::Result<Value> {
    let modules = modules(pid)?;
    let mut report = json!({ "modules": modules.iter().map(|m| &m.report).collect::<Vec<_>>() });

    // 区域/架构诊断才需要 Query 权限，即使拿不到该额外权限，也要保留可读采样
    // SAFETY: `pid` 来自 Toolhelp 枚举，该调用只返回一个拥有所有权的 handle，
    // 不解引用任何 pointer
    let handle = unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, false, pid) };
    let handle = match handle {
        Ok(handle) => Handle(handle),
        Err(error) => {
            report["queryPermissionError"] = json!(error.to_string());

            // SAFETY: 对已枚举的 PID 做只读访问，返回的 handle 拥有所有权
            match unsafe { OpenProcess(PROCESS_VM_READ, false, pid) } {
                Ok(handle) => Handle(handle),
                Err(error) => {
                    report["error"] = json!(format!("无法打开酷狗进程: {error}"));
                    return Ok(report);
                }
            }
        }
    };

    let mut machine = IMAGE_FILE_MACHINE::default();
    let mut native = IMAGE_FILE_MACHINE::default();

    // SAFETY: handle 存活，两个 machine 值都是有效的输出 buffer
    report["architecture"] = result_value(unsafe {
        IsWow64Process2(handle.0, &mut machine, Some(&mut native))
    }.map(|()| json!({ "processMachine": format!("{:#x}", machine.0), "nativeMachine": format!("{:#x}", native.0) })).map_err(Into::into));

    let find = |name: &str| modules.iter().find(|m| m.name.eq_ignore_ascii_case(name));
    let prepare = || -> anyhow::Result<_> {
        let position = find("kgplayer.dll").context("缺少 kgplayer.dll")?;
        let duration = find("kugou.dll").context("缺少 kugou.dll")?;
        let layouts = layouts(
            position.version.context("无法获取 kgplayer.dll 版本")?,
            duration.version.context("无法获取 kugou.dll 版本")?,
        )?;

        Ok((position, duration, layouts))
    };

    match prepare() {
        Ok((position, duration, layouts)) => {
            let mut reader = Reader {
                identity_entry: Err("not initialized".into()),
                handle,
                position_base: position.base,
                duration_base: duration.base,
                layouts,
                layout: layouts[0],
            };
            reader.prepare_identity(duration.version.unwrap_or_default(), duration.size as usize);
            report["identityEntry"] = match &reader.identity_entry {
                Ok(entry) => json!({"value":entry}),
                Err(error) => json!({"error":error}),
            };

            let mut captures = Vec::new();
            let origin = Instant::now();

            for index in 0..SAMPLE_COUNT {
                if index > 0 {
                    std::thread::sleep(SAMPLE_INTERVAL);
                }

                let started = timestamp();
                let media_before = media_snapshot(media);
                let mut attempts = Vec::new();

                for (priority, layout) in layouts.iter().enumerate() {
                    reader.layout = *layout;

                    let mut attempt = sample(&reader);
                    attempt["layout"] = json!({
                        "priority": priority,
                        "positionOffset": format!("{:#x}", layout.position_offset),
                        "durationOffset": format!("{:#x}", layout.duration_offset),
                        "durationChain": layout.duration_chain.iter().map(|o| format!("{o:#x}")).collect::<Vec<_>>(),
                        "durationUnitsPerSecond": layout.duration_units_per_second,
                        "positionEntryWithinModule": layout.position_offset.checked_add(8).is_some_and(|end| end <= position.size as usize),
                        "durationEntryWithinModule": layout.duration_offset.checked_add(8).is_some_and(|end| end <= duration.size as usize),
                    });
                    attempts.push(attempt);
                }

                let selected = attempts
                    .iter()
                    .position(|attempt| attempt["timeline"]["value"].is_array());
                captures.push(json!({
                    "index": index,
                    "startedUnixMs": started,
                    "finishedUnixMs": timestamp(),
                    "elapsedMs": origin.elapsed().as_millis(),
                    "preferredUsableLayoutIndex": selected,
                    "identity": result_value(reader.identity()),
                    "mediaBefore": media_before,
                    "mediaAfter": media_snapshot(media),
                    "layouts": attempts,
                }));
            }
            report["samples"] = json!(captures);
        }
        Err(error) => report["error"] = json!(format!("{error:#}")),
    }

    Ok(report)
}

fn processes(media: Option<&qq_music::Probe>) -> anyhow::Result<Vec<Value>> {
    // SAFETY: CreateToolhelp32Snapshot 不接受 pointer，返回的 HANDLE 归 `Handle` 所有，
    // 由其在 drop 时关闭
    let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? });
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    // SAFETY: `entry` 是大小正确、`dwSize` 已初始化的 PROCESSENTRY32W，snapshot 在整个
    // 枚举期间保持打开
    unsafe { Process32FirstW(snapshot.0, &mut entry)? };

    // 先枚举再采样，避免延迟期间的进程变动隐藏候选
    let mut candidates = Vec::new();

    loop {
        let name = text(&entry.szExeFile);

        if name.eq_ignore_ascii_case("KuGou.exe") || name.eq_ignore_ascii_case("KuGouMusic.exe") {
            candidates.push((entry.th32ProcessID, name));
        }

        // SAFETY: 在同一个存活的 snapshot 上继续遍历，`entry` 保持其 `dwSize`
        if let Err(error) = unsafe { Process32NextW(snapshot.0, &mut entry) } {
            ensure!(
                error.code().0 as u32 == 0x80070012,
                "枚举酷狗进程中断: {error}"
            );
            break;
        }
    }

    Ok(candidates
        .into_iter()
        .map(|(pid, name)| {
            let mut report = process(pid, media)
                .unwrap_or_else(|error| json!({ "error": format!("{error:#}") }));
            report["pid"] = json!(pid);
            report["name"] = json!(name);
            report
        })
        .collect())
}

/// 构建运行中 KuGou 进程的诊断 snapshot
///
/// 在调用方的 blocking thread 上运行：WinRT probe 在该线程创建并释放，报告包含应用、系统、
/// module 与多次时间轴采样；缺失的 module、权限失败和读取错误会写入返回的 JSON，
/// 而不是中止报告
pub fn report() -> Value {
    let started = timestamp();

    // 在同一个 blocking worker thread 上创建并使用 WinRT 对象
    let media = qq_music::Probe::new(Instant::now());
    let media_error = media.as_ref().err().map(|error| format!("{error:#}"));
    let media = DiagnosticMedia(media.ok());
    let mut report = json!({
        "schemaVersion": 2,
        "hysilensVersion": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "hostArchitecture": std::env::consts::ARCH,
        "startedUnixMs": started,
        "sampleCount": SAMPLE_COUNT,
        "sampleIntervalMs": SAMPLE_INTERVAL.as_millis(),
        "pointerBytes": size_of::<u64>(),
        "system": system_info(),
        "application": result_value(application_info()),
        "mediaBefore": media_snapshot(media.0.as_ref()),
        "contextBytesBefore": CONTEXT_BEFORE,
        "contextBytesAfter": CONTEXT_AFTER,
    });

    if let Some(error) = media_error {
        report["mediaError"] = json!(error);
    }

    match processes(media.0.as_ref()) {
        Ok(processes) => {
            if processes.is_empty() {
                report["error"] = json!("未找到酷狗进程，请启动酷狗并播放歌曲");
            }
            report["processes"] = json!(processes);
        }
        Err(error) => report["error"] = json!(format!("{error:#}")),
    }
    report["mediaAfter"] = media_snapshot(media.0.as_ref());
    report["finishedUnixMs"] = json!(timestamp());
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(position: &f64, root: &u64, chain: &'static [usize]) -> Reader {
        Reader {
            identity_entry: Err("test".into()),
            // SAFETY: 测试只用只读/查询权限的 handle 读取自身存活的 buffer
            handle: Handle(unsafe {
                OpenProcess(
                    PROCESS_VM_READ | PROCESS_QUERY_INFORMATION,
                    false,
                    std::process::id(),
                )
                .unwrap()
            }),
            position_base: position as *const f64 as usize,
            duration_base: root as *const u64 as usize,
            layouts: &[],
            layout: Layout {
                position_offset: 0,
                duration_offset: 0,
                duration_chain: chain,
                duration_units_per_second: 1000.0,
            },
        }
    }

    #[test]
    fn preserves_readable_but_invalid_duration_and_pointer_steps() {
        let position = 136.2340794f64;
        let duration = 528i32;
        let root = &duration as *const i32 as u64;
        let report = sample(&reader(&position, &root, &[0]));
        assert_eq!(report["position"]["seconds"], position);
        assert_eq!(report["duration"]["rawValue"], 528);
        assert_eq!(report["duration"]["seconds"], 0.528);
        assert_eq!(report["progressUsable"], true);
        assert_eq!(report["durationSteps"][0]["pointer"], format!("{root:#x}"));
        assert!(
            report["timeline"]["error"]
                .as_str()
                .unwrap()
                .contains("时间轴数值无效")
        );
    }

    #[test]
    fn keeps_position_and_failed_pointer_region() {
        let position = 21.5f64;
        let root = 0u64;
        let report = sample(&reader(&position, &root, &[0, 0]));
        assert_eq!(report["position"]["seconds"], position);
        assert_eq!(report["durationSteps"].as_array().unwrap().len(), 2);

        let failed = &report["durationSteps"][1];
        assert_eq!(failed["address"], "0x0");
        assert!(failed["error"].is_string());
        assert!(failed["region"].is_object());
        assert!(report["durationError"].is_string());
    }

    #[test]
    fn keeps_failed_leaf_read_and_nonfinite_position_in_serializable_report() {
        let position = f64::NAN;
        let root = 0u64;
        let report = sample(&reader(&position, &root, &[0]));
        assert_eq!(report["position"]["rawSeconds"], "NaN");
        assert_eq!(report["progressUsable"], false);
        assert_eq!(report["duration"]["address"], "0x0");
        assert!(report["duration"]["region"].is_object());
        assert!(report["duration"]["error"].is_string());
        assert!(serde_json::to_string_pretty(&report).is_ok());
    }

    #[test]
    fn reports_valid_timeline_without_losing_raw_values() {
        let position = 21.5f64;
        let duration = 209000i32;
        let root = &duration as *const i32 as u64;
        let report = sample(&reader(&position, &root, &[0]));
        assert_eq!(report["timeline"]["value"], json!([21500.0, 209000.0]));
        assert_eq!(report["duration"]["bytesHex"], "68300300");

        let nearby = &report["duration"]["region"]["nearby"];
        let count = nearby["readBytes"].as_u64().unwrap() as usize;
        let offset = nearby["targetOffset"].as_u64().unwrap() as usize;
        let bytes = nearby["bytesHex"].as_str().unwrap();
        assert!(count <= CONTEXT_BEFORE + CONTEXT_AFTER);
        assert_eq!(&bytes[offset * 2..offset * 2 + 8], "68300300");
    }

    #[test]
    fn context_read_stays_within_region_boundary() {
        let bytes = [0xabu8; 32];
        let start = bytes.as_ptr() as usize;

        // 刻意收窄的 region 用于证明上下文读取遵守其边界
        let info = MEMORY_BASIC_INFORMATION {
            BaseAddress: start as _,
            RegionSize: bytes.len(),
            State: windows::Win32::System::Memory::MEM_COMMIT,
            Protect: windows::Win32::System::Memory::PAGE_READWRITE,
            ..Default::default()
        };

        let position = 0.0;
        let root = 0u64;
        let reader = reader(&position, &root, &[0]);
        let report = nearby(reader.handle.0, start + 4, &info);
        assert_eq!(report["address"], format!("{start:#x}"));
        assert_eq!(report["targetOffset"], 4);
        assert_eq!(report["readBytes"], bytes.len());
        assert_eq!(report["bytesHex"], "ab".repeat(bytes.len()));

        let protected = MEMORY_BASIC_INFORMATION {
            Protect: windows::Win32::System::Memory::PAGE_NOACCESS,
            ..info
        };
        assert!(nearby(reader.handle.0, start, &protected)["skipped"].is_string());
    }

    #[test]
    fn hashes_file_contents_for_comparing_same_version_modules() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"abc").unwrap();
        assert_eq!(
            sha256(file.path()).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn collects_windows_build_cpu_and_memory_without_starting_processes() {
        let report = system_info();
        assert!(
            report["windows"]["buildNumber"]["value"]
                .as_str()
                .is_some_and(|v| !v.is_empty())
        );
        assert!(
            report["cpu"]["name"]["value"]
                .as_str()
                .is_some_and(|v| !v.is_empty())
        );
        assert!(
            report["memory"]["value"]["totalPhysicalBytes"]
                .as_u64()
                .unwrap()
                > 0
        );
    }
}
