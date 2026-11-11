#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod local_overlay;
mod runtime;

use clap::Parser;
use hysilens_core::config::{Settings, data_root};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::{mpsc, watch};

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    port: Option<u16>,
    #[arg(long)]
    headless: bool,
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
    let (restart, requests) = mpsc::channel(4);
    let (urls, mut navigation) = watch::channel(url.clone());
    let worker = runtime.spawn(runtime::supervise(
        listener, settings, restart, requests, urls,
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
    let handle = runtime.handle().clone();
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            local_overlay::local_overlay_get,
            local_overlay::local_overlay_set,
            local_overlay::local_overlay_drag,
            local_overlay::local_overlay_size,
        ])
        .setup(move |app| {
            let initial = local_overlay::control_url(&url, cli.dev)?;
            let desktop = local_overlay::DesktopState::new(initial.clone());
            let navigation_guard = desktop.control_navigation();
            app.manage(std::sync::Mutex::new(desktop));
            #[cfg(windows)]
            local_overlay::watch_drag(app.handle().clone());
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(initial))
                .initialization_script(local_overlay::CONTROL_BRIDGE)
                .on_navigation(navigation_guard)
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                .title("Hysilens")
                .inner_size(1100., 640.)
                .min_inner_size(900., 640.)
                .build()?;
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
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(
                    event,
                    tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
                )
            {
                local_overlay::close(window.app_handle());
            }
        })
        .run(tauri::generate_context!())?;
    worker.abort();
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    Ok(())
}
