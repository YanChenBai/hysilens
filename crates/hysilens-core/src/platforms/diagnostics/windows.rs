//! Windows 诊断共用的进程与文件版本信息，仅枚举目标进程的主模块

use anyhow::ensure;
use serde_json::{Value, json};
use std::mem::size_of;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        Storage::FileSystem::{
            GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
        },
        System::Diagnostics::ToolHelp::*,
    },
    core::{PCWSTR, w},
};

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: snapshot handle 归本对象唯一所有，且只关闭一次
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn text(value: &[u16]) -> String {
    let end = value
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(value.len());

    String::from_utf16_lossy(&value[..end])
}

pub(crate) fn processes(names: &[&str]) -> Value {
    match process_list(names) {
        Ok(processes) => json!({ "value": processes }),
        Err(error) => json!({ "error": format!("{error:#}") }),
    }
}

fn process_list(names: &[&str]) -> anyhow::Result<Vec<Value>> {
    // SAFETY: 无 pointer 参数；成功返回的 snapshot 由 Handle 负责关闭
    let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? });
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    // SAFETY: entry 长度正确且 dwSize 已初始化，snapshot 在枚举期间存活
    unsafe {
        Process32FirstW(snapshot.0, &mut entry)?;
    }

    let mut result = Vec::new();

    loop {
        let name = text(&entry.szExeFile);

        if names
            .iter()
            .any(|expected| name.eq_ignore_ascii_case(expected))
        {
            let image = match image(entry.th32ProcessID) {
                Ok(image) => image,
                Err(error) => json!({ "error": format!("{error:#}") }),
            };

            result.push(json!({ "pid": entry.th32ProcessID, "name": name, "image": image }));
        }

        // SAFETY: entry 与 snapshot 在整个枚举期间保持有效
        if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
            break;
        }
    }

    Ok(result)
}

fn image(pid: u32) -> anyhow::Result<Value> {
    // SAFETY: pid 来自进程枚举；成功的 snapshot 归 Handle 所有
    let snapshot =
        Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)? });
    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };

    // SAFETY: entry 的大小字段已初始化，snapshot 在调用期间存活
    unsafe {
        Module32FirstW(snapshot.0, &mut entry)?;
    }

    let version = match file_version(&entry.szExePath) {
        Ok(version) => json!({ "value": version }),
        Err(error) => json!({ "error": format!("{error:#}") }),
    };

    Ok(json!({ "path": text(&entry.szExePath), "version": version }))
}

pub(crate) fn modules(pid: u32, names: &[&str]) -> Value {
    result_value(module_list(pid, names))
}

fn result_value<T: serde::Serialize>(result: anyhow::Result<T>) -> Value {
    match result {
        Ok(value) => json!({ "value": value }),
        Err(error) => json!({ "error": format!("{error:#}") }),
    }
}

fn module_list(pid: u32, names: &[&str]) -> anyhow::Result<Vec<Value>> {
    // SAFETY: pid 来自已附加进程；成功的 snapshot 归 Handle 唯一所有
    let snapshot =
        Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)? });
    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };

    // SAFETY: entry 的大小字段已初始化，snapshot 在遍历期间存活
    unsafe {
        Module32FirstW(snapshot.0, &mut entry)?;
    }

    let mut result = Vec::new();

    loop {
        let name = text(&entry.szModule);

        if names
            .iter()
            .any(|expected| name.eq_ignore_ascii_case(expected))
        {
            let path = text(&entry.szExePath);

            result.push(json!({
                "name": name,
                "base": entry.modBaseAddr as usize,
                "size": entry.modBaseSize,
                "path": path,
                "version": result_value(file_version(&entry.szExePath)),
                "sha256": result_value(file_hash(std::path::Path::new(&path))),
            }));
        }

        // SAFETY: entry 和 snapshot 在遍历期间持续有效
        if unsafe { Module32NextW(snapshot.0, &mut entry) }.is_err() {
            break;
        }
    }

    Ok(result)
}

fn file_hash(path: &std::path::Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let file = std::fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= 64 * 1024 * 1024,
        "模块哈希超过 64 MiB 预算"
    );

    let mut hash = Sha256::new();
    let mut reader = file.take(64 * 1024 * 1024 + 1);
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0;

    loop {
        let count = reader.read(&mut buffer)?;

        if count == 0 {
            break;
        }

        total += count;
        ensure!(total <= 64 * 1024 * 1024, "模块哈希超过 64 MiB 预算");
        hash.update(&buffer[..count]);
    }

    Ok(format!("{:x}", hash.finalize()))
}

pub(crate) fn file_version(path: &[u16]) -> anyhow::Result<[u16; 4]> {
    ensure!(path.last() == Some(&0), "文件路径未以 NUL 结尾");

    // SAFETY: Toolhelp 提供以 NUL 结尾的模块路径，缓冲区在整个调用期间有效
    let length = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
    ensure!(length > 0, "无法获取文件版本信息大小");

    let mut buffer = vec![0u8; length as usize];
    let mut value = std::ptr::null_mut();
    let mut value_length = 0;

    // SAFETY: buffer 分配了 API 要求的字节数，查询结果指向仍存活的 buffer
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
        "文件版本信息不完整"
    );

    // SAFETY: 已验证结果长度；read_unaligned 不要求字节缓冲区具备结构体对齐
    let version = unsafe { value.cast::<VS_FIXEDFILEINFO>().read_unaligned() };
    ensure!(version.dwSignature == 0xfeef04bd, "文件版本信息无效");

    Ok([
        (version.dwFileVersionMS >> 16) as u16,
        version.dwFileVersionMS as u16,
        (version.dwFileVersionLS >> 16) as u16,
        version.dwFileVersionLS as u16,
    ])
}
