//! 托盘保留后台服务与歌词窗口，主界面关闭时只隐藏；退出由菜单显式触发

use tauri::{
    AppHandle, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

/// 托盘必须先创建成功，之后才能把主窗口关闭动作改为隐藏
///
/// # Errors
/// 图标缺失、菜单或系统托盘创建失败时返回错误
pub fn setup(app: &AppHandle) -> anyhow::Result<()> {
    let show = MenuItem::with_id(app, "show-main", "显示主界面", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide-main", "隐藏主界面", true, None::<&str>)?;
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
                    .map_or(Ok(()), |window| window.hide()),
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

fn show_main(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.unminimize()?;
        window.show()?;
        window.set_focus()?;
    }

    Ok(())
}
