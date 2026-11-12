//! 托盘保留后台服务与歌词窗口；收起主界面时释放 WebView，点击托盘时按需重建

use tauri::{
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

/// 托盘必须先创建成功，之后才能允许主窗口关闭而继续驻留
///
/// # Errors
/// 图标缺失、菜单或系统托盘创建失败时返回错误
pub fn setup(app: &AppHandle) -> anyhow::Result<()> {
    let show = MenuItem::with_id(app, "show-main", "显示主界面", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide-main", "收起主界面", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Hysilens", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &separator, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("应用缺少托盘图标"))?;

    TrayIconBuilder::with_id("hysilens")
        .icon(icon)
        .tooltip("Hysilens · 点击显示主界面")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let result = match event.id.as_ref() {
                "show-main" => show_main(app),
                "hide-main" => app
                    .get_webview_window("main")
                    .map_or(Ok(()), |window| window.close()),
                "quit" => {
                    app.exit(0);

                    Ok(())
                }
                _ => Ok(()),
            };

            if let Err(error) = result {
                eprintln!("托盘操作失败: {error}");
            }
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) && let Err(error) = show_main(tray.app_handle())
            {
                eprintln!("从托盘恢复主界面失败: {error}");
            }
        })
        .build(app)?;

    Ok(())
}

/// 使用最新的 server origin 重建控制页；不持有状态锁执行 WebView 创建
///
/// # Errors
/// 状态锁已 poison、窗口创建或恢复失败时返回错误
pub fn show_main(app: &AppHandle) -> tauri::Result<()> {
    let window = if let Some(window) = app.get_webview_window("main") {
        window
    } else {
        let (url, navigation_guard) = {
            let shared = app.state::<crate::local_overlay::SharedDesktop>();
            let state = shared
                .lock()
                .map_err(|_| std::io::Error::other("桌面状态锁已损坏"))?;

            (state.control_url.clone(), state.control_navigation())
        };

        WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
            .initialization_script(crate::local_overlay::CONTROL_BRIDGE)
            .on_navigation(navigation_guard)
            .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
            .title("Hysilens")
            .inner_size(1100., 640.)
            .min_inner_size(900., 640.)
            .build()?
    };

    window.unminimize()?;
    window.show()?;
    window.set_focus()?;

    Ok(())
}
