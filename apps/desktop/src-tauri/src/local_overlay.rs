//! 无边框歌词 overlay 窗口以及驱动它的 IPC 接口
//!
//! overlay 是透明、置顶、支持 click-through 的 WebView，label 为 `local-lyrics`，
//! 渲染本地 server 的歌词视图（`/`）；它首次使用时创建，之后复用：禁用时只是隐藏
//! 窗口，页面因而保留自己的 DOM 状态；`close` 是唯一的拆除路径，在主窗口消失时运行
//!
//! 单位：跨越 WebView 边界的每个值（`Geometry`、`local_overlay_drag` 坐标、
//! `local_overlay_size` 尺寸）都是 **logical pixel**，与 CSS screen 坐标一致；
//! drag offset 是相对按键按下位置的增量；physical pixel 只出现在 `native_drag`
//! 内部，并由它用窗口 scale factor 换算一次
//!
//! threading：Tauri command 是 `async` 的，起始于 async runtime，因此任何创建、
//! 移动、缩放、显示或隐藏窗口的操作都通过 `run_on_main_thread` 转到主线程，
//! 这是 Winit 事件循环的要求；[`DesktopState`] 以 [`SharedDesktop`] 共享，且只在
//! 该线程被修改，所以锁从不跨 `await` 持有

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

/// overlay WebView 的窗口 label，所有 command 与 guard 都以它为键
const LABEL: &str = "local-lyrics";

#[cfg(windows)]
mod native_drag;

/// 启动 overlay 窗口的 OS 级右键 drag watcher
///
/// 仅在 Windows 使用，因为透明 WebView 无法可靠投递 DOM pointer event；其他平台
/// 的拖动只走 `local_overlay_drag`，轮询循环见 [`native_drag`]
#[cfg(windows)]
pub fn watch_drag(app: AppHandle) {
    native_drag::watch(app);
}

/// `main` 窗口的注入脚本
///
/// 它暴露冻结的 `window.__HYSILENS_DESKTOP__`，声明核心 IPC 能力并提供 `getOverlay` 与 `setOverlay`，
/// 二者是 `local_overlay_get` 和 `local_overlay_set` 的薄封装；随后派发
/// `hysilens:desktop-ready`，让页面在初始化时就能发现桌面 host；只有 `/control`
/// 或 `/settings` 的顶层 frame 会获得该 API，歌词 webview 和 plugin 文档 URL 没有
/// 任何窗口管理能力
pub const CONTROL_BRIDGE: &str = r#"
if (window === window.top && ['/control', '/settings'].includes(location.pathname)) {
  Object.defineProperty(window, '__HYSILENS_DESKTOP__', {
    value: Object.freeze({
      controlIpc: true,
      getOverlay: () => window.__TAURI_INTERNALS__.invoke('local_overlay_get'),
      setOverlay: settings => window.__TAURI_INTERNALS__.invoke('local_overlay_set', { settings }),
    }),
  });
  window.dispatchEvent(new Event('hysilens:desktop-ready'));
}
"#;

/// overlay 窗口的位置与尺寸，单位为 logical pixel
///
/// `x`/`y` 是窗口外框左上角在虚拟桌面坐标中的位置，在主显示器左侧或上方的
/// 显示器上可以为负；`width`/`height` 是 logical 内尺寸；[`Geometry::validate`]
/// 强制的范围是宽 160–8192、高 80–8192
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Geometry {
    /// 窗口左边缘，logical pixel
    pub x: i32,

    /// 窗口上边缘，logical pixel
    pub y: i32,

    /// 内宽，logical pixel
    pub width: u32,

    /// 内高，logical pixel
    pub height: u32,
}

/// 全新安装时的默认位置：位于 (80, 80) 的 960×320 窗口
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
    /// 拒绝 160–8192 × 80–8192 之外的尺寸以及超过 ±100000 的坐标
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

/// overlay 的用户偏好，持久化在 `<data_root>/local-overlay.json`
///
/// 保存是 atomic 的：JSON 先写入 `local-overlay.tmp` 再重命名覆盖目标，因此崩溃
/// 不会留下截断的文件
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    /// overlay 是否让鼠标输入穿透到下方桌面
    pub click_through: bool,

    /// 上一次的位置，下次显示 overlay 时恢复
    pub geometry: Geometry,
}

impl Preferences {
    /// 读取偏好文件，文件不存在时回退到默认值
    ///
    /// # Errors
    ///
    /// JSON 无效、geometry 越界，或 `NotFound` 之外的 I/O 失败
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

    /// 校验并以 atomic 方式写入偏好
    ///
    /// # Errors
    ///
    /// geometry 越界，或创建目录 / 写入 / 重命名失败
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

/// 交给 WebView 的完整 overlay 状态
///
/// 以 `camelCase` 序列化并展开 preferences，所以线上结构是
/// `{ enabled, clickThrough, geometry: { x, y, width, height } }`
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlaySettings {
    /// overlay 窗口当前是否显示
    pub enabled: bool,

    #[serde(flatten)]
    pub preferences: Preferences,
}

/// `local_overlay_set` 的请求体
///
/// 刻意比 [`OverlaySettings`] 窄：控制页面可以切换开关，但不得覆盖歌词窗口上报
/// 的 geometry，这也是 `deny_unknown_fields` 会拒绝携带它的 payload 的原因
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayUpdate {
    /// 请求的 overlay 可见性
    enabled: bool,

    /// 请求的 click-through 模式
    click_through: bool,
}

/// pointer drag 开始时记录的锚点
///
/// 保存那一刻的 cursor 与窗口原点，单位均为 logical pixel，这样每次移动只应用
/// 增量，窗口保持抓取偏移而不是把角吸附到 cursor 上
#[derive(Clone, Copy)]
struct DragOrigin {
    /// drag 开始时的 pointer X，logical pixel
    cursor_x: f64,

    /// drag 开始时的 pointer Y，logical pixel
    cursor_y: f64,

    /// drag 开始时的窗口 X，logical pixel
    x: i32,

    /// drag 开始时的窗口 Y，logical pixel
    y: i32,
}

impl DragOrigin {
    /// 把 cursor 位置映射为窗口原点，并 clamp 到 [`Geometry::validate`] 接受的
    /// 坐标范围
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

/// overlay 脚本上报的一次 pointer drag 的某个 phase
///
/// `start` 记录锚点，`move` 重新定位窗口并更新保存的 geometry，`end` 再定位一次
/// 并持久化；没有前置 `start` 的 `move`/`end`，以及 click-through 启用期间的任何
/// phase，都是 no-op
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DragPhase {
    /// 按键在 overlay 上按下
    Start,

    /// 按键保持期间 pointer 移动
    Move,

    /// 按键释放，最终位置被保存
    End,
}

/// 与 Tauri command 共享的 host 侧状态
///
/// 在任何窗口存在之前创建，并作为 [`SharedDesktop`] 由 Tauri app 持有；只有主线程
/// 修改它，且只有 `main` 窗口获得授权
pub struct DesktopState {
    /// 运行中 server 的 `/control` URL：既是被允许驱动 overlay 的 origin，也是
    /// 推导歌词 URL 的基地址
    pub control_url: Url,

    /// 运行中 server 的 port，与那些生命周期长于单次状态锁的 navigation guard
    /// 共享
    navigation_port: Arc<AtomicU16>,

    /// 当前 overlay 状态，即 `local_overlay_get` 的返回值
    settings: OverlaySettings,

    /// 应用开始退出后置位，此后所有 command 都会拒绝
    closing: bool,

    /// 正在进行的 pointer drag
    drag: Option<DragOrigin>,

    /// 从 OS 采样得到的右键 drag，见 [`native_drag`]
    #[cfg(windows)]
    native_drag: native_drag::RightDrag,
}

/// [`DesktopState`] 的 managed-state handle
///
/// guard 只在主线程执行同步窗口操作期间持有，从不跨 `await`
pub type SharedDesktop = Mutex<DesktopState>;

impl DesktopState {
    /// 返回安装在 `main` 窗口上的 navigation guard
    ///
    /// 它只接受 `/control` 或 `/settings` 下的 `http://127.0.0.1:<server port>`
    /// 页面；port 在每次导航时都通过共享 atomic 读取，因此 server 重启后 guard
    /// 依然有效
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

    /// 在任何窗口存在之前构建初始状态
    ///
    /// 会读取已保存的 geometry，但 overlay 初始为隐藏：打开它总是显式的用户操作
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

/// 解析窗口应加载的控制页面 URL
///
/// 开启 `dev` 时无论 `server_url` 是什么都使用 5173 端口的 Vite dev server，
/// 否则直接使用 `server_url`
///
/// # Errors
///
/// 生成的 URL 解析失败
pub fn control_url(server_url: &str, dev: bool) -> anyhow::Result<Url> {
    Ok(if dev {
        "http://127.0.0.1:5173/control"
    } else {
        server_url
    }
    .parse()?)
}

/// `url` 是否可以加载进给定 label 的窗口
///
/// 只有 `main` 可以导航，且只能停留在运行中 server 的 origin 内、只到 `/control`
/// 或 `/settings`；携带凭据的 URL 会被拒绝
fn allowed_control(label: &str, url: &Url, current: &Url) -> bool {
    label == "main"
        && url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.origin() == current.origin()
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(url.path(), "/control" | "/settings")
}

/// 检查 command 是否由存活的 `main` 窗口调用
///
/// # Errors
///
/// 应用正在关闭，或调用窗口/URL 不是获得授权的控制页面
fn authorize(label: &str, url: &Url, state: &DesktopState) -> anyhow::Result<()> {
    anyhow::ensure!(
        !state.closing && allowed_control(label, url, &state.control_url),
        "本地悬浮窗只能从主控制窗口操作"
    );

    Ok(())
}

/// 校验核心控制 IPC 的调用窗口，复用当前控制页面的导航边界
///
/// # Errors
/// 窗口未授权、应用正在关闭或状态锁损坏时返回错误
pub fn authorize_control(window: &WebviewWindow) -> anyhow::Result<()> {
    let url = window.url()?;
    let shared = window.state::<SharedDesktop>();
    let state = shared
        .lock()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    authorize(window.label(), &url, &state)
}

/// 把当前 [`OverlaySettings`] 返回给控制页面
///
/// 在 async runtime 上运行且只读取状态：先解析窗口 URL，再短暂持有锁完成鉴权与克隆
///
/// # Errors
///
/// 调用方不是当前 control origin 的 `main` 窗口、应用正在关闭，或状态锁已 poison
#[tauri::command]
pub async fn local_overlay_get(window: WebviewWindow) -> Result<OverlaySettings, String> {
    // 先查询 UI thread，再获取状态锁（close 和 navigation 也会用到它）
    let url = window.url().map_err(|error| error.to_string())?;
    let state = window.state::<SharedDesktop>();
    let state = state.lock().map_err(|error| error.to_string())?;

    authorize(window.label(), &url, &state).map_err(|error| error.to_string())?;

    Ok(state.settings.clone())
}

/// 应用控制页面发来的可见性 / click-through 变更并返回结果状态
///
/// 在 async runtime 上运行，但所有工作都在 `run_on_main_thread` 内完成，因为创建、
/// 显示和隐藏窗口只允许在那里进行；geometry 从当前状态沿用，控制表单从不决定窗口
/// 位置；应用或保存失败时会重新应用上一次的状态，窗口不会停留在半更新状态
///
/// # Errors
///
/// 调用方未授权、锁已 poison，或应用 / 持久化变更时窗口 / 文件系统失败
#[tauri::command]
pub async fn local_overlay_set(
    window: WebviewWindow,
    settings: OverlayUpdate,
) -> Result<OverlaySettings, String> {
    // 创建窗口必须走事件循环，绝不能阻塞 IPC thread
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

                // 位置和建议尺寸归歌词窗口所有，不取自过期的控制表单
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
                    // 持久化失败时保留可见状态，包括刚刚打开的窗口
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

/// 窗口本身是否为 overlay：与 server 同 origin，路径为根 `/`，且没有 query
fn allowed_lyrics(label: &str, url: &Url, current: &Url) -> bool {
    label == LABEL && url.origin() == current.origin() && url.path() == "/" && url.query().is_none()
}

/// 在持有状态锁的情况下于主线程运行 `update`
///
/// overlay 启用期间，调用方必须是当前 origin 上的存活 overlay 窗口，否则更新被
/// 拒绝；因此 `update` 内部的窗口改动是安全的，结果通过 oneshot channel 回传
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

/// 处理 overlay 脚本驱动的一次 pointer drag 的某个 phase
///
/// `x`/`y` 是 pointer 位置的 logical pixel（CSS screen 坐标），必须有限且位于
/// ±100000 之内；`start` 保存锚点，`move` 重新定位窗口并更新保存的 geometry，
/// `end` 还会持久化
///
/// # Errors
///
/// 坐标非有限或越界、调用方不是存活 overlay、启用了 click-through，或窗口移动 /
/// 保存失败
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

/// 应用歌词内容请求的 logical 内尺寸
///
/// 窗口缩放前，`width`/`height` 会按与 [`Geometry`] 相同的范围（160–8192 ×
/// 80–8192）校验；尺寸没有变化时直接忽略，不触碰窗口
///
/// # Errors
///
/// 尺寸越界、调用方不是存活 overlay，或缩放 / 保存失败
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

/// 从 control URL 推导歌词 URL：同 origin、路径 `/`，且没有 query 或 fragment
fn lyric_url(control: &Url) -> Url {
    let mut url = control.clone();
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);

    url
}

/// 把 `settings` 应用到 overlay 窗口，首次打开时创建窗口
///
/// 关闭 overlay 时释放其 WebView，再次打开时按已保存的 geometry 重建；必须在主线程运行
/// 创建时配置窗口
/// （透明、无边框、置顶、不显示在任务栏、不可聚焦），每次调用都会在显示前重新
/// 应用尺寸、位置、click-through 与置顶状态
fn apply(app: &AppHandle, state: &DesktopState, settings: &OverlaySettings) -> anyhow::Result<()> {
    let existing = app.get_webview_window(LABEL);

    if !settings.enabled {
        if let Some(window) = existing {
            window.destroy()?;
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

    // Tao 在可见性变化时会重写 extended style，所以要在 show 之后、以及之后每次
    // click-through 变化后重新应用 tool-window 标志
    #[cfg(windows)]
    make_tool_window(&window)?;

    Ok(())
}

/// 把窗口标记为 tool window：不出现在任务栏和 Alt-Tab，且不会被激活，同时保留
/// layered、topmost 与 transparent 位
#[cfg(windows)]
fn tool_window_style(style: u32) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    (style | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) & !WS_EX_APPWINDOW.0
}

/// 把 [`tool_window_style`] 应用到 overlay 当前窗口
///
/// 必须在主线程运行，因为主线程才是窗口的所有者
#[cfg(windows)]
fn make_tool_window(window: &WebviewWindow) -> anyhow::Result<()> {
    use windows::Win32::Foundation::{GetLastError, SetLastError, WIN32_ERROR};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SetWindowLongPtrW, SetWindowPos,
    };

    let hwnd = window.hwnd()?;

    // SAFETY: `hwnd` 是我们存活的 overlay 窗口句柄，调用期间有效，并在拥有它的
    // UI thread 上使用；两个调用只修改该窗口的 extended style 位 / z-order，没有
    // 指针或借用 buffer 跨过 FFI 边界；`SetWindowLongPtrW` 返回 0 只有在先前
    // `SetLastError(0)` 仍为 0 时才算错误，下面的检查依赖这一点
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

/// 让两个窗口指向（重启后）server 的 URL
///
/// 先更新保存的 control URL 与 navigation port，再导航 `main` 窗口并重新推导歌词
/// URL；必须在主线程调用，应用正在关闭时什么也不做
///
/// # Errors
///
/// 导航任一窗口失败
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

/// 作为关闭应用的一部分拆除 overlay
///
/// 持久化最后的 geometry，把状态标记为 closing 以拒绝后续 command，然后销毁窗口；
/// 该操作幂等，失败只上报而不会中断关闭流程
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
    fn control_capability_matches_ipc_origins_without_page_paths() {
        use tauri::utils::acl::{RemoteUrlPattern, capability::Capability};

        let capability: Capability =
            serde_json::from_str(include_str!("../capabilities/local-overlay.json")).unwrap();

        let patterns: Vec<RemoteUrlPattern> = capability
            .remote
            .unwrap()
            .urls
            .iter()
            .map(|pattern| pattern.parse().unwrap())
            .collect();

        // 基于 fetch 的 IPC transport 发送的是 Origin，其中没有页面路径
        for (origin, allowed) in [
            ("http://127.0.0.1:38473", true),
            ("http://127.0.0.1:5173", true),
            ("http://127.0.0.1:40000", true),
            ("http://localhost:38473", false),
            ("https://127.0.0.1:38473", false),
            ("http://127.0.0.1.evil.test:38473", false),
        ] {
            let url = origin.parse().unwrap();
            assert_eq!(
                patterns.iter().any(|pattern| pattern.test(&url)),
                allowed,
                "{origin}"
            );
        }
    }

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
