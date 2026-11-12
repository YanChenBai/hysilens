//! Hysilens 的 Tauri 桌面 host
//!
//! 该二进制掌管整个进程：应用命令行覆盖项、加载
//! [`hysilens_core::config::Settings`]、绑定 loopback listener，并在多线程 Tokio
//! runtime 上运行核心服务；主窗口通过 IPC 控制，HTTP/WebSocket 只服务歌词渲染
//! [`runtime::supervise`] 让这个 server 在设置变更期间保持存活，本模块则持有它
//! 外面的 Tauri application
//!
//! 承载两个窗口：
//!
//! - `main` 渲染控制 UI（`/control` 或 Vite dev server），是唯一接收
//!   [`local_overlay::CONTROL_BRIDGE`] 的 frame
//! - `local-lyrics` 是无边框歌词 overlay，按需创建，完全由 [`local_overlay`] 管理
//!
//! 关闭 `main` 窗口时释放其 WebView，overlay 与 server 继续运行；从托盘退出应用时
//! 销毁 overlay、abort server task，并给 runtime 一小段宽限期

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ipc;
mod local_overlay;
mod runtime;
mod tray;

use clap::Parser;
use hysilens_core::config::{Settings, data_root};
use tauri::Manager;
use tokio::sync::{mpsc, watch};

/// 叠加在设置文件之上的命令行覆盖项
#[derive(Parser)]
struct Cli {
    /// 覆盖本地 server 配置中的 TCP port
    #[arg(long)]
    port: Option<u16>,

    /// 不创建任何窗口直接运行 HTTP server，并等待 Ctrl-C
    #[arg(long)]
    headless: bool,

    /// 从 5173 端口的 Vite dev server 加载控制 UI，而不是 server 自己的
    /// `/control` 页面
    #[arg(long)]
    dev: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mut settings = Settings::load()?;

    if let Some(port) = cli.port {
        settings.port = port;
    }

    settings.validate()?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let listener = runtime.block_on(tokio::net::TcpListener::bind((
        std::net::Ipv4Addr::LOCALHOST,
        settings.port,
    )))?;

    let url = format!("http://127.0.0.1:{}/control", settings.port);

    // 设置变更会回传给 supervisor，每次 server 重启成功后都会发布新的
    // control URL，供窗口跟随
    let (restart, requests) = mpsc::channel(4);
    let (urls, mut navigation) = watch::channel(url.clone());
    let (control, services) = watch::channel(None);
    let worker = runtime.spawn(runtime::supervise(
        listener, settings, restart, requests, urls, control,
    ));

    println!(
        "Hysilens: {url}\nData: {}\nPID: {}",
        data_root().display(),
        std::process::id()
    );

    if cli.headless {
        runtime.block_on(tokio::signal::ctrl_c())?;
        worker.abort();

        return Ok(());
    }

    // 生命周期长于 setup 闭包的工作句柄
    let handle = runtime.handle().clone();

    tauri::Builder::default()
        .manage(ipc::ControlIpc::new(services))
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::Destroyed)
                && let Err(error) = window.state::<ipc::ControlIpc>().cancel(None)
            {
                eprintln!("释放主窗口 IPC 订阅失败: {error}");
            }
        })
        .invoke_handler(tauri::generate_handler![
            local_overlay::local_overlay_get,
            local_overlay::local_overlay_set,
            local_overlay::local_overlay_drag,
            local_overlay::local_overlay_size,
            ipc::control_request,
            ipc::control_install_plugin,
            ipc::control_subscribe,
            ipc::control_unsubscribe,
        ])
        .setup(move |app| {
            let initial = local_overlay::control_url(&url, cli.dev)?;
            let desktop = local_overlay::DesktopState::new(initial.clone());
            app.manage(std::sync::Mutex::new(desktop));

            #[cfg(windows)]
            local_overlay::watch_drag(app.handle().clone());

            tray::setup(app.handle())?;
            tray::show_main(app.handle())?;

            // 跟随 server 重启：supervisor 每次发布新 URL 时重新指向两个窗口
            let app_handle = app.handle().clone();
            handle.spawn(async move {
                while navigation.changed().await.is_ok() {
                    let url = navigation.borrow_and_update().clone();

                    if let Ok(url) = local_overlay::control_url(&url, cli.dev) {
                        let navigation_app = app_handle.clone();
                        let _ = app_handle.run_on_main_thread(move || {
                            if let Err(error) = local_overlay::navigate(&navigation_app, url) {
                                eprintln!("更新桌面窗口地址失败: {error}");
                            }
                        });
                    }
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())?
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    // 最后一个 WebView 关闭后保留托盘与服务，只有显式退出才清理 overlay
                    api.prevent_exit();
                } else {
                    local_overlay::close(app);
                }
            }
        });

    worker.abort();
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));

    Ok(())
}
