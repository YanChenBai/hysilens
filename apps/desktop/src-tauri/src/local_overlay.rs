use hysilens_core::config::data_root;
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU16, Ordering},
};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

const LABEL: &str = "local-lyrics";

#[cfg(windows)]
mod native_drag;

#[cfg(windows)]
pub fn watch_drag(app: AppHandle) {
    native_drag::watch(app);
}

// Only the main frame receives the small API. No window-management permissions
// are granted to the lyric webview or to plugin document URLs.
pub const CONTROL_BRIDGE: &str = r#"
if (window === window.top && ['/control', '/settings'].includes(location.pathname)) {
  Object.defineProperty(window, '__HYSILENS_DESKTOP__', {
    value: Object.freeze({
      getOverlay: () => window.__TAURI_INTERNALS__.invoke('local_overlay_get'),
      setOverlay: settings => window.__TAURI_INTERNALS__.invoke('local_overlay_set', { settings }),
    }),
  });
  window.dispatchEvent(new Event('hysilens:desktop-ready'));
}
"#;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            x: 80,
            y: 80,
            width: 960,
            height: 320,
        }
    }
}

impl Geometry {
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (160..=8192).contains(&self.width) && (80..=8192).contains(&self.height),
            "宽度须为 160–8192，高度须为 80–8192"
        );
        anyhow::ensure!(
            (-100_000..=100_000).contains(&self.x) && (-100_000..=100_000).contains(&self.y),
            "窗口坐标超出范围"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub click_through: bool,
    pub geometry: Geometry,
}

impl Preferences {
    fn load() -> anyhow::Result<Self> {
        let path = data_root().join("local-overlay.json");
        match std::fs::read(path) {
            Ok(bytes) => {
                let value: Self = serde_json::from_slice(&bytes)?;
                value.geometry.validate()?;
                Ok(value)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    fn save(&self) -> anyhow::Result<()> {
        self.geometry.validate()?;
        let directory = data_root();
        std::fs::create_dir_all(&directory)?;
        let temporary = directory.join("local-overlay.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, directory.join("local-overlay.json"))?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlaySettings {
    pub enabled: bool,
    #[serde(flatten)]
    pub preferences: Preferences,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayUpdate {
    enabled: bool,
    click_through: bool,
}

#[derive(Clone, Copy)]
struct DragOrigin {
    cursor_x: f64,
    cursor_y: f64,
    x: i32,
    y: i32,
}

impl DragOrigin {
    fn position(self, x: f64, y: f64) -> LogicalPosition<i32> {
        LogicalPosition::new(
            (f64::from(self.x) + x - self.cursor_x)
                .round()
                .clamp(-100_000., 100_000.) as i32,
            (f64::from(self.y) + y - self.cursor_y)
                .round()
                .clamp(-100_000., 100_000.) as i32,
        )
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DragPhase {
    Start,
    Move,
    End,
}

pub struct DesktopState {
    pub control_url: Url,
    navigation_port: Arc<AtomicU16>,
    settings: OverlaySettings,
    closing: bool,
    drag: Option<DragOrigin>,
    #[cfg(windows)]
    native_drag: native_drag::RightDrag,
}

pub type SharedDesktop = Mutex<DesktopState>;

impl DesktopState {
    pub fn control_navigation(&self) -> impl Fn(&Url) -> bool + Send + 'static {
        let port = self.navigation_port.clone();
        move |url| {
            url.scheme() == "http"
                && url.host_str() == Some("127.0.0.1")
                && url.port_or_known_default() == Some(port.load(Ordering::Relaxed))
                && url.username().is_empty()
                && url.password().is_none()
                && matches!(url.path(), "/control" | "/settings")
        }
    }

    pub fn new(control_url: Url) -> Self {
        let preferences = Preferences::load().unwrap_or_else(|error| {
            eprintln!("读取本地悬浮窗设置失败，使用默认位置: {error}");
            Preferences::default()
        });
        Self {
            navigation_port: Arc::new(AtomicU16::new(
                control_url.port_or_known_default().unwrap_or(80),
            )),
            control_url,
            // Opening is always an explicit action; saved geometry does not auto-start a window.
            settings: OverlaySettings {
                enabled: false,
                preferences,
            },
            closing: false,
            drag: None,
            #[cfg(windows)]
            native_drag: native_drag::RightDrag::default(),
        }
    }
}

pub fn control_url(server_url: &str, dev: bool) -> anyhow::Result<Url> {
    Ok(if dev {
        "http://127.0.0.1:5173/control"
    } else {
        server_url
    }
    .parse()?)
}

fn allowed_control(label: &str, url: &Url, current: &Url) -> bool {
    label == "main"
        && url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.origin() == current.origin()
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(url.path(), "/control" | "/settings")
}

fn authorize(label: &str, url: &Url, state: &DesktopState) -> anyhow::Result<()> {
    anyhow::ensure!(
        !state.closing && allowed_control(label, url, &state.control_url),
        "本地悬浮窗只能从主控制窗口操作"
    );
    Ok(())
}

#[tauri::command]
pub async fn local_overlay_get(window: WebviewWindow) -> Result<OverlaySettings, String> {
    // Query the UI thread before taking the state lock (close/navigation also use it).
    let url = window.url().map_err(|error| error.to_string())?;
    let state = window.state::<SharedDesktop>();
    let state = state.lock().map_err(|error| error.to_string())?;
    authorize(window.label(), &url, &state).map_err(|error| error.to_string())?;
    Ok(state.settings.clone())
}

#[tauri::command]
pub async fn local_overlay_set(
    window: WebviewWindow,
    settings: OverlayUpdate,
) -> Result<OverlaySettings, String> {
    // Window creation must run on the event loop, never block the IPC thread.
    let (reply, result) = tokio::sync::oneshot::channel();
    let app = window.app_handle().clone();
    app.clone()
        .run_on_main_thread(move || {
            let result = (|| -> anyhow::Result<OverlaySettings> {
                let shared = app.state::<SharedDesktop>();
                let mut state = shared
                    .lock()
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                authorize(window.label(), &window.url()?, &state)?;
                let previous = state.settings.clone();
                // Position and recommended size belong to the lyric window, not a stale control form.
                let settings = OverlaySettings {
                    enabled: settings.enabled,
                    preferences: Preferences {
                        click_through: settings.click_through,
                        geometry: previous.preferences.geometry.clone(),
                    },
                };
                if let Err(error) =
                    apply(&app, &state, &settings).and_then(|()| settings.preferences.save())
                {
                    // Preserve the visible state if persistence fails, including a newly opened window.
                    if let Err(rollback) = apply(&app, &state, &previous) {
                        eprintln!("恢复悬浮窗失败: {rollback}");
                    }
                    return Err(error);
                }
                state.settings = settings;
                state.drag = None;
                #[cfg(windows)]
                state.native_drag.cancel();
                Ok(state.settings.clone())
            })();
            let _ = reply.send(result.map_err(|error| error.to_string()));
        })
        .map_err(|error| error.to_string())?;
    result.await.map_err(|error| error.to_string())?
}

fn allowed_lyrics(label: &str, url: &Url, current: &Url) -> bool {
    label == LABEL && url.origin() == current.origin() && url.path() == "/" && url.query().is_none()
}

async fn update_lyrics(
    window: WebviewWindow,
    update: impl FnOnce(&WebviewWindow, &mut DesktopState) -> anyhow::Result<()> + Send + 'static,
) -> Result<(), String> {
    let app = window.app_handle().clone();
    let (reply, result) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = (|| -> anyhow::Result<()> {
            let shared = window.state::<SharedDesktop>();
            let mut state = shared
                .lock()
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            anyhow::ensure!(
                !state.closing
                    && state.settings.enabled
                    && allowed_lyrics(window.label(), &window.url()?, &state.control_url),
                "悬浮窗已关闭或来源无效"
            );
            update(&window, &mut state)
        })();
        let _ = reply.send(result.map_err(|error| error.to_string()));
    })
    .map_err(|error| error.to_string())?;
    result.await.map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn local_overlay_drag(
    window: WebviewWindow,
    phase: DragPhase,
    x: f64,
    y: f64,
) -> Result<(), String> {
    if !x.is_finite() || !y.is_finite() || x.abs() > 100_000. || y.abs() > 100_000. {
        return Err("拖动坐标无效".into());
    }
    update_lyrics(window, move |window, state| {
        anyhow::ensure!(
            !state.settings.preferences.click_through,
            "鼠标穿透时不能拖动"
        );
        let geometry = &mut state.settings.preferences.geometry;
        if matches!(phase, DragPhase::Start) {
            state.drag = Some(DragOrigin {
                cursor_x: x,
                cursor_y: y,
                x: geometry.x,
                y: geometry.y,
            });
            return Ok(());
        }
        let Some(origin) = state.drag else {
            return Ok(());
        };
        let position = origin.position(x, y);
        window.set_position(position)?;
        geometry.x = position.x;
        geometry.y = position.y;
        if matches!(phase, DragPhase::End) {
            state.settings.preferences.save()?;
            state.drag = None;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn local_overlay_size(
    window: WebviewWindow,
    width: u32,
    height: u32,
) -> Result<(), String> {
    update_lyrics(window, move |window, state| {
        let geometry = &state.settings.preferences.geometry;
        if geometry.width == width && geometry.height == height {
            return Ok(());
        }
        let next = Geometry {
            width,
            height,
            ..geometry.clone()
        };
        next.validate()?;
        window.set_size(LogicalSize::new(width, height))?;
        state.settings.preferences.geometry = next;
        state.settings.preferences.save()
    })
    .await
}

fn lyric_url(control: &Url) -> Url {
    let mut url = control.clone();
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    url
}

fn apply(app: &AppHandle, state: &DesktopState, settings: &OverlaySettings) -> anyhow::Result<()> {
    let existing = app.get_webview_window(LABEL);
    if !settings.enabled {
        if let Some(window) = existing {
            window.hide()?;
        }
        return Ok(());
    }
    let geometry = &settings.preferences.geometry;
    let window = match existing {
        Some(window) => window,
        None => {
            let navigation_port = state.navigation_port.clone();
            WebviewWindowBuilder::new(
                app,
                LABEL,
                WebviewUrl::External(lyric_url(&state.control_url)),
            )
            .title("Hysilens Lyrics")
            .decorations(false)
            .transparent(true)
            .background_color(tauri::webview::Color(0, 0, 0, 0))
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false)
            .shadow(false)
            .resizable(false)
            .visible(false)
            .initialization_script_for_all_frames(format!(
                "window.__HYSILENS_NATIVE_DRAG__ = {};\n{}",
                cfg!(windows),
                include_str!("overlay-drag.js")
            ))
            .inner_size(f64::from(geometry.width), f64::from(geometry.height))
            .position(f64::from(geometry.x), f64::from(geometry.y))
            .on_navigation(move |url| {
                url.scheme() == "http"
                    && url.host_str() == Some("127.0.0.1")
                    && url.port_or_known_default() == Some(navigation_port.load(Ordering::Relaxed))
                    && url.path() == "/"
                    && url.query().is_none()
                    && url.username().is_empty()
                    && url.password().is_none()
            })
            .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
            .build()?
        }
    };
    window.set_size(LogicalSize::new(geometry.width, geometry.height))?;
    window.set_position(LogicalPosition::new(geometry.x, geometry.y))?;
    window.set_ignore_cursor_events(settings.preferences.click_through)?;
    window.set_always_on_top(true)?;
    window.show()?;
    // Tao rewrites extended styles when visibility changes; apply tool-window
    // flags after show, and after every subsequent click-through change.
    #[cfg(windows)]
    make_tool_window(&window)?;
    Ok(())
}

#[cfg(windows)]
fn tool_window_style(style: u32) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };
    (style | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) & !WS_EX_APPWINDOW.0
}

#[cfg(windows)]
fn make_tool_window(window: &WebviewWindow) -> anyhow::Result<()> {
    use windows::Win32::Foundation::{GetLastError, SetLastError, WIN32_ERROR};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SetWindowLongPtrW, SetWindowPos,
    };
    let hwnd = window.hwnd()?;
    // SAFETY: this is our live lyric window HWND, used only on the UI thread.
    // Preserve all WebView/layered flags and only alter task switching/activation.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        SetLastError(WIN32_ERROR(0));
        if SetWindowLongPtrW(hwnd, GWL_EXSTYLE, tool_window_style(style) as isize) == 0 {
            let error = GetLastError();
            anyhow::ensure!(error.0 == 0, "设置悬浮窗样式失败: {}", error.0);
        }
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
        )?;
    }
    Ok(())
}

pub fn navigate(app: &AppHandle, url: Url) -> anyhow::Result<()> {
    {
        let shared = app.state::<SharedDesktop>();
        let mut state = shared
            .lock()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        if state.closing {
            return Ok(());
        }
        state.control_url = url.clone();
        state
            .navigation_port
            .store(url.port_or_known_default().unwrap_or(80), Ordering::Relaxed);
    }
    if let Some(window) = app.get_webview_window("main") {
        window.navigate(url.clone())?;
    }
    if let Some(window) = app.get_webview_window(LABEL) {
        window.navigate(lyric_url(&url))?;
    }
    Ok(())
}

pub fn close(app: &AppHandle) {
    if let Ok(mut state) = app.state::<SharedDesktop>().lock() {
        if !state.closing
            && let Err(error) = state.settings.preferences.save()
        {
            eprintln!("保存悬浮窗位置失败: {error}");
        }
        state.closing = true;
        state.settings.enabled = false;
    }
    if let Some(window) = app.get_webview_window(LABEL)
        && let Err(error) = window.destroy()
    {
        eprintln!("关闭悬浮窗失败: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_uses_cursor_delta_without_losing_negative_monitor_positions() {
        let origin = DragOrigin {
            cursor_x: 120.,
            cursor_y: 80.,
            x: -1900,
            y: -500,
        };
        assert_eq!(
            origin.position(145., 60.),
            LogicalPosition::new(-1875, -520)
        );
        assert_eq!(
            origin.position(160., 100.),
            LogicalPosition::new(-1860, -480)
        );
    }

    #[test]
    fn control_updates_cannot_overwrite_window_geometry() {
        assert!(serde_json::from_str::<OverlayUpdate>(
            r#"{"enabled":true,"clickThrough":false,"geometry":{"x":0,"y":0,"width":960,"height":320}}"#
        ).is_err());
        assert!(
            serde_json::from_str::<OverlayUpdate>(r#"{"enabled":true,"clickThrough":false}"#)
                .is_ok()
        );
    }

    #[test]
    fn only_local_lyrics_root_can_move_or_resize() {
        let current: Url = "http://127.0.0.1:38473/control".parse().unwrap();
        for (label, url, allowed) in [
            (LABEL, "http://127.0.0.1:38473/", true),
            ("main", "http://127.0.0.1:38473/", false),
            (LABEL, "http://127.0.0.1:38474/", false),
            (LABEL, "http://127.0.0.1:38473/?preview=dark", false),
            (
                LABEL,
                "http://127.0.0.1:38473/api/plugins/builtin/view",
                false,
            ),
        ] {
            assert_eq!(
                allowed_lyrics(label, &url.parse().unwrap(), &current),
                allowed
            );
        }
    }

    #[test]
    fn accepts_negative_monitor_coordinates() {
        assert!(
            Geometry {
                x: -1920,
                y: -1080,
                ..Geometry::default()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn rejects_invalid_geometry() {
        for geometry in [
            Geometry {
                width: 0,
                ..Geometry::default()
            },
            Geometry {
                height: 8193,
                ..Geometry::default()
            },
            Geometry {
                x: i32::MIN,
                ..Geometry::default()
            },
        ] {
            assert!(geometry.validate().is_err(), "{geometry:?}");
        }
    }

    #[test]
    fn rejects_fractional_dimensions_and_extra_properties() {
        for value in [
            r#"{"x":0,"y":0,"width":160.5,"height":80}"#,
            r#"{"x":0,"y":0,"width":160,"height":80,"url":"https://example.com"}"#,
        ] {
            assert!(serde_json::from_str::<Geometry>(value).is_err());
        }
    }

    #[test]
    fn only_current_main_control_origin_is_authorized() {
        let current: Url = "http://127.0.0.1:38473/control".parse().unwrap();
        for (label, url, allowed) in [
            ("main", "http://127.0.0.1:38473/control", true),
            ("main", "http://127.0.0.1:38473/settings", true),
            (LABEL, "http://127.0.0.1:38473/control", false),
            ("main", "http://127.0.0.1:38474/control", false),
            ("main", "http://127.0.0.1:38473/", false),
            ("main", "http://127.0.0.1:38473/api/plugins/foo/view", false),
            ("main", "http://localhost:38473/control", false),
            ("main", "https://127.0.0.1:38473/control", false),
            ("main", "http://127.0.0.1.evil.test:38473/control", false),
        ] {
            assert_eq!(
                allowed_control(label, &url.parse().unwrap(), &current),
                allowed,
                "{label}: {url}"
            );
        }
    }

    #[test]
    fn navigation_preserves_dev_origin_and_removes_preview_queries() {
        assert_eq!(
            control_url("http://127.0.0.1:40000/control", true)
                .unwrap()
                .as_str(),
            "http://127.0.0.1:5173/control"
        );
        assert_eq!(
            lyric_url(
                &"http://127.0.0.1:40000/control?preview=dark#test"
                    .parse()
                    .unwrap()
            )
            .as_str(),
            "http://127.0.0.1:40000/"
        );
    }

    #[test]
    fn settings_round_trip() {
        let value = OverlaySettings {
            enabled: true,
            preferences: Preferences::default(),
        };
        let restored: OverlaySettings =
            serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(restored.enabled);
        assert_eq!(restored.preferences, value.preferences);
    }

    #[cfg(windows)]
    #[test]
    fn tool_style_preserves_transparency_and_topmost_without_appwindow() {
        use windows::Win32::UI::WindowsAndMessaging::{
            WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
            WS_EX_TRANSPARENT,
        };
        let retained = WS_EX_LAYERED.0 | WS_EX_TOPMOST.0 | WS_EX_TRANSPARENT.0;
        assert_eq!(
            tool_window_style(retained | WS_EX_APPWINDOW.0),
            retained | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0
        );
    }
}
