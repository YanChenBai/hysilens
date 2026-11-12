//! 以增强模式启动酷狗，或在桌面创建带调试参数的快捷方式
//!
//! 不会结束或重启正在运行的酷狗：单实例客户端已运行时会忽略启动参数，此时只返回
//! [`Launch::NormalMode`]，由用户自行完全退出后再启动

use super::cdp;
use anyhow::{Context, ensure};
use std::{
    mem::size_of,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::CloseHandle,
        Storage::EnhancedStorage::PKEY_AppUserModel_ID,
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize, IPersistFile,
                StructuredStorage::{
                    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
                },
            },
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Registry::{HKEY_CLASSES_ROOT, RRF_RT_REG_SZ, RegGetValueW},
            Variant::VT_LPWSTR,
        },
        UI::Shell::{
            FOLDERID_Desktop, IShellLinkW, KF_FLAG_DEFAULT, PropertiesSystem::IPropertyStore,
            SHGetKnownFolderPath, SHStrDupW, ShellLink,
        },
    },
    core::{HSTRING, Interface, PCWSTR, w},
};

/// 桌面快捷方式的文件名
pub const SHORTCUT_NAME: &str = "酷狗音乐（Hysilens 增强）.lnk";

/// 酷狗官方快捷方式声明的 AppUserModelID
///
/// 快捷方式缺少它时，Windows 会为带参数启动的酷狗生成另一个 ID，系统媒体会话就无法识别为酷狗
const APP_USER_MODEL_ID: &str = "kugou";

const PROCESS_NAMES: [&str; 2] = ["KuGou.exe", "KuGouMusic.exe"];

/// [`launch`] 的结果
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Launch {
    /// 调试端口已开启，无需启动
    Ready,

    /// 已按增强模式启动酷狗
    Started,

    /// 酷狗正以普通模式运行，需完全退出后再启动
    NormalMode,
}

/// 是否有酷狗进程在运行
pub fn running() -> bool {
    process_running().unwrap_or(false)
}

fn process_running() -> anyhow::Result<bool> {
    // SAFETY: 无 pointer 参数；返回的 snapshot 在函数末尾关闭
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = false;

    // SAFETY: entry 尺寸正确且已初始化 dwSize，snapshot 在枚举期间保持打开
    let mut next = unsafe { Process32FirstW(snapshot, &mut entry) };

    while next.is_ok() {
        let end = entry
            .szExeFile
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);

        if PROCESS_NAMES
            .iter()
            .any(|expected| name.eq_ignore_ascii_case(expected))
        {
            found = true;
            break;
        }

        // SAFETY: 同上，继续遍历同一个 snapshot
        next = unsafe { Process32NextW(snapshot, &mut entry) };
    }

    // SAFETY: snapshot 由本函数创建，只关闭一次
    unsafe {
        let _ = CloseHandle(snapshot);
    }

    Ok(found)
}

/// 从 `kugou://` 协议的打开命令中取出可执行文件路径
fn command_executable(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    let path = match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => {
            let end = command.to_ascii_lowercase().find(".exe")? + 4;
            &command[..end]
        }
    };

    Some(PathBuf::from(path))
}

fn registry_executable() -> Option<PathBuf> {
    let mut buffer = vec![0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;

    // SAFETY: buffer 与 size 描述同一块可写内存，键名为静态以 null 结尾的宽字符串
    let status = unsafe {
        RegGetValueW(
            HKEY_CLASSES_ROOT,
            w!("kugou\\shell\\open\\command"),
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };

    if status.is_err() {
        return None;
    }

    let length = (size as usize / 2).min(buffer.len());
    let end = buffer[..length]
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(length);

    command_executable(&String::from_utf16_lossy(&buffer[..end]))
}

/// 查找酷狗启动器：优先 `kugou://` 协议登记的路径，其次常见安装目录
///
/// 使用安装根目录下的启动器而不是带版本号的目录，升级后路径仍然有效
pub fn executable() -> anyhow::Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = registry_executable().into_iter().collect();

    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(variable) {
            let root = PathBuf::from(root);
            candidates.push(root.join("KuGou/KGMusic/KuGou.exe"));
            candidates.push(root.join("KuGou2012/KuGou.exe"));
        }
    }

    candidates
        .into_iter()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("KuGou.exe"))
                && path.is_file()
        })
        .context("未找到已安装的酷狗客户端")
}

/// 以增强模式启动酷狗；已开启或已在普通模式运行时不做任何操作
pub fn launch() -> anyhow::Result<Launch> {
    if cdp::port_open(cdp::PORT) {
        ensure!(
            cdp::endpoint_available(cdp::PORT).unwrap_or(false),
            "调试端口 9225 已被占用或酷狗页面不可用，请检查后重试"
        );

        return Ok(Launch::Ready);
    }

    if running() {
        return Ok(Launch::NormalMode);
    }

    let executable = executable()?;
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    std::process::Command::new(&executable)
        .args(cdp::LAUNCH_ARGS)
        .current_dir(executable.parent().unwrap_or(Path::new(".")))
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .with_context(|| format!("无法启动酷狗 ({})", executable.display()))?;

    Ok(Launch::Started)
}

/// 在桌面创建以增强模式启动酷狗的快捷方式，已存在时覆盖这一个文件；返回快捷方式路径
///
/// 不修改用户原有的酷狗快捷方式
pub fn create_shortcut() -> anyhow::Result<PathBuf> {
    let executable = executable()?;

    // SAFETY: 在当前线程初始化 COM，成功时在函数结束前配对调用 CoUninitialize
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .context("无法初始化快捷方式所需的 COM apartment")?;
    let result = write_shortcut(&executable);

    // SAFETY: 与上面成功的 CoInitializeEx 配对，write_shortcut 中的 COM 对象均已释放
    unsafe { CoUninitialize() };

    result
}

fn app_user_model_id() -> windows::core::Result<PROPVARIANT> {
    // SAFETY: 输入字符串在调用期间有效；SHStrDupW 分配的内存交由 PROPVARIANT 析构时释放
    let id = unsafe { SHStrDupW(&HSTRING::from(APP_USER_MODEL_ID))? };

    Ok(PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_LPWSTR,
                Anonymous: PROPVARIANT_0_0_0 { pwszVal: id },
                ..Default::default()
            }),
        },
    })
}

fn write_shortcut(executable: &Path) -> anyhow::Result<PathBuf> {
    // SAFETY: 返回的字符串由 COM 分配，转换后立即用 CoTaskMemFree 释放
    let desktop = unsafe {
        let raw = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None)
            .context("无法定位桌面目录")?;
        let path = raw.to_string();
        CoTaskMemFree(Some(raw.0 as _));
        PathBuf::from(path.context("桌面路径编码无效")?)
    };
    ensure!(desktop.is_dir(), "桌面目录不存在");

    let path = desktop.join(SHORTCUT_NAME);
    let directory = executable.parent().unwrap_or(Path::new("."));

    // SAFETY: COM 已在当前线程初始化；所有字符串参数在调用期间有效
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .context("无法创建快捷方式对象")?;

        link.SetPath(&HSTRING::from(executable.as_os_str()))?;
        link.SetArguments(&HSTRING::from(cdp::LAUNCH_ARGS.join(" ")))?;
        link.SetWorkingDirectory(&HSTRING::from(directory.as_os_str()))?;
        link.SetIconLocation(&HSTRING::from(executable.as_os_str()), 0)?;
        link.SetDescription(&HSTRING::from("以 Hysilens 增强模式启动酷狗音乐"))?;

        let value = app_user_model_id().context("无法分配快捷方式的应用标识")?;

        let store: IPropertyStore = link.cast()?;
        store
            .SetValue(&PKEY_AppUserModel_ID, &value)
            .context("无法设置快捷方式的应用标识")?;
        store.Commit()?;

        link.cast::<IPersistFile>()?
            .Save(&HSTRING::from(path.as_os_str()), true)
            .context("无法保存快捷方式")?;
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_variant_owns_its_string() {
        for _ in 0..100 {
            let value = app_user_model_id().unwrap();

            assert_eq!(value.vt(), VT_LPWSTR);

            // SAFETY: 上面已检查类型，字符串由 value 持有且以 null 结尾
            let id = unsafe { value.Anonymous.Anonymous.Anonymous.pwszVal.to_string() }.unwrap();

            assert_eq!(id, APP_USER_MODEL_ID);
            drop(value);
        }
    }

    #[test]
    #[ignore = "会在桌面创建或覆盖增强模式快捷方式"]
    fn creates_desktop_shortcut() {
        let path = create_shortcut().unwrap();

        println!("{}", path.display());
        assert!(path.is_file());
    }

    #[test]
    fn parses_protocol_commands() {
        assert_eq!(
            command_executable(r#""C:\Program Files\KuGou\KGMusic\KuGou.exe" "%1""#),
            Some(PathBuf::from(r"C:\Program Files\KuGou\KGMusic\KuGou.exe"))
        );
        assert_eq!(
            command_executable(r"D:\KuGou\KuGou.exe %1"),
            Some(PathBuf::from(r"D:\KuGou\KuGou.exe"))
        );
        assert_eq!(command_executable("   "), None);
    }
}
