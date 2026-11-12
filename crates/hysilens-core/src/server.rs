//! 核心服务运行时与只读的歌词 HTTP/WebSocket 通道，桌面控制通过进程内句柄访问

pub mod control;

use crate::plugin_registry::{PluginCatalog, PluginEvent, PluginRegistry};
use crate::protocol::{ClientMessage, DocumentCursor, ServerMessage, Snapshot, SnapshotUpdate};
use crate::{
    capture::{self, Event as CaptureEvent},
    config::Settings,
    coordinator::Coordinator,
    model::Source,
};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
};
use std::{
    borrow::Cow,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{RwLock, mpsc, oneshot, watch};
use tower_http::services::ServeDir;

#[derive(rust_embed::RustEmbed)]
#[folder = "../../apps/desktop/dist/"]
struct EmbeddedWeb;

/// 浏览器 UI 的提供位置
#[derive(Debug)]
pub enum WebRoot {
    /// 编译进二进制的资源
    Embedded,

    /// 从磁盘提供的开发构建目录
    Directory(PathBuf),
}

struct RuntimeGuard {
    stop: Arc<AtomicBool>,

    reducer: tokio::task::JoinHandle<()>,
}

impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.reducer.abort();
    }
}

enum Action {
    Overlay(crate::settings::OverlaySettings),

    PluginRemoved(String),

    Capture(CaptureEvent),

    Source(Source),

    Select {
        generation: u64,

        key: Option<String>,

        reply: oneshot::Sender<Result<(), String>>,
    },

    Offset(i32),

    Display {
        romanization: bool,

        translation: bool,
    },
}

#[derive(Clone)]
struct AppState {
    origin: Instant,

    port: u16,

    actions: mpsc::Sender<Action>,

    snapshots: watch::Receiver<Arc<Snapshot>>,

    settings: Arc<RwLock<Settings>>,

    restart: mpsc::Sender<Settings>,

    plugins: Arc<PluginRegistry>,

    diagnostics: Arc<
        Vec<(
            crate::platforms::Descriptor,
            crate::platforms::diagnostics::Diagnostics,
        )>,
    >,

    diagnostic_export: Arc<tokio::sync::Mutex<()>>,

    /// 酷狗增强模式的运行时开关，与酷狗 adapter 共享
    kugou_enhanced: Arc<AtomicBool>,
}

/// [`serve_on`] 的启动输入
#[derive(Debug)]
pub struct RuntimeOptions {
    /// 启动时加载的 settings；server 之后会自行持久化变更
    pub settings: Settings,

    /// settings 变更请求重启进程时使用的 channel
    pub restart: mpsc::Sender<Settings>,

    /// 桌面 host 接收当前服务控制句柄；headless 可不订阅
    pub control: watch::Sender<Option<control::ControlHandle>>,
}

/// 提供 overlay UI、snapshot 流与 plugin API，直到 `shutdown` 完成
///
/// # Errors
/// listener 没有本地地址、web 构建缺失、adapter 以 `auto` 注册或重复注册，
/// 或 `axum` 带错误停止时失败
pub async fn serve_on(
    listener: tokio::net::TcpListener,
    options: RuntimeOptions,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let source = options.settings.source;
    let kugou_enhanced = Arc::new(AtomicBool::new(options.settings.kugou_cdp));
    let registrations = vec![
        crate::platforms::wesing::registration(),
        crate::platforms::qq_music::registration(options.settings.cache_dir.clone()),
        crate::platforms::kugou::registration(
            options.settings.kugou_cache_dir.clone(),
            kugou_enhanced.clone(),
        ),
    ];

    let web = WebRoot::Embedded;
    let port = listener.local_addr()?.port();

    match &web {
        WebRoot::Embedded => anyhow::ensure!(
            EmbeddedWeb::get("index.html").is_some(),
            "Build the overlay before Rust: vp run @hysilens/desktop#build:web"
        ),
        WebRoot::Directory(path) => anyhow::ensure!(
            path.join("index.html").is_file(),
            "Build the overlay before Rust: vp run @hysilens/desktop#build:web"
        ),
    }

    let origin = Instant::now();
    let epoch = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );

    let descriptors: Vec<_> = registrations
        .iter()
        .map(|registration| registration.descriptor)
        .collect();

    let diagnostics = Arc::new(
        registrations
            .iter()
            .map(|registration| (registration.descriptor, registration.diagnostics.clone()))
            .collect(),
    );

    anyhow::ensure!(
        descriptors
            .iter()
            .all(|descriptor| descriptor.source != Source::Auto),
        "An adapter cannot register as auto"
    );

    for (index, descriptor) in descriptors.iter().enumerate() {
        anyhow::ensure!(
            !descriptors[..index]
                .iter()
                .any(|old| old.source == descriptor.source
                    || old.session_id.eq_ignore_ascii_case(descriptor.session_id)),
            "Duplicate platform registration"
        );
    }

    let mut coordinator = Coordinator::new(epoch, descriptors);

    coordinator.source(source);
    coordinator.state.overlay = options.settings.overlay.clone();
    coordinator.state.offset_ms = options.settings.overlay.offset.round() as i32;
    coordinator.state.show_romanization = options.settings.overlay.romanization;
    coordinator.state.show_translation = options.settings.overlay.translation;

    let settings = Arc::new(RwLock::new(options.settings));
    let reducer_settings = settings.clone();
    let (snapshots_tx, snapshots) = watch::channel(Arc::new(coordinator.state.clone()));
    let (actions, mut receiver) = mpsc::channel(32);
    let stop = Arc::new(AtomicBool::new(false));
    let mut captures = capture::spawn(origin, registrations, stop.clone());
    let reducer = tokio::spawn(async move {
        loop {
            let action = tokio::select! {
                Some(action) = receiver.recv() => action,
                Some(event) = captures.recv() => Action::Capture(event),
                else => break,
            };

            match action {
                Action::Overlay(settings) => {
                    coordinator.state.offset_ms = settings.offset.round() as i32;
                    coordinator.state.show_romanization = settings.romanization;
                    coordinator.state.show_translation = settings.translation;
                    coordinator.state.overlay = settings;
                }
                Action::PluginRemoved(id) => {
                    if coordinator.state.overlay.renderer == id {
                        coordinator.state.overlay.renderer =
                            crate::settings::OverlaySettings::default().renderer;
                    }
                }
                Action::Capture(CaptureEvent::Playback(playback, document)) => {
                    coordinator.sample(playback, document)
                }
                Action::Source(source) => coordinator.source(source),
                Action::Capture(CaptureEvent::Catalog(update)) => {
                    coordinator.catalog(update.source, update.catalog);
                }
                Action::Select {
                    generation,
                    key,
                    reply,
                } => {
                    let result = coordinator
                        .select(generation, key)
                        .map_err(|error| error.to_string());

                    let _ = reply.send(result);
                }
                Action::Offset(value) => {
                    coordinator.state.offset_ms = value.clamp(-10_000, 10_000);
                    coordinator.state.overlay.offset = coordinator.state.offset_ms as f64;
                }
                Action::Display {
                    romanization,
                    translation,
                } => {
                    coordinator.state.show_romanization = romanization;

                    coordinator.state.show_translation = translation;
                    coordinator.state.overlay.romanization = romanization;
                    coordinator.state.overlay.translation = translation;
                }
            }

            {
                let mut saved = reducer_settings.write().await;

                if saved.overlay != coordinator.state.overlay
                    || saved.source != coordinator.state.source
                {
                    saved.overlay = coordinator.state.overlay.clone();
                    saved.source = coordinator.state.source;

                    if let Err(error) = saved.save() {
                        eprintln!("保存设置失败: {error}");
                    }
                }
            }

            coordinator.state.seq += 1;

            snapshots_tx.send_replace(Arc::new(coordinator.state.clone()));
        }
    });

    let embedded_plugins = EmbeddedWeb::get("plugins/index.json")
        .and_then(|asset| serde_json::from_slice(&asset.data).ok())
        .unwrap_or_default();

    let plugins = PluginRegistry::new(
        embedded_plugins,
        crate::plugins::roots(),
        crate::plugins::config_root(),
    );

    // shutdown 置位 `stop` 后，会在一个 debounce 周期内退出
    plugins.spawn_watcher(stop.clone());

    let state = AppState {
        origin,
        port,
        actions,
        snapshots,
        settings,
        restart: options.restart,
        plugins,
        diagnostics,
        diagnostic_export: Arc::default(),
        kugou_enhanced,
    };

    options
        .control
        .send_replace(Some(control::ControlHandle::new(state.clone())));

    let router = renderer_routes();

    let router = match web {
        WebRoot::Embedded => router.fallback(embedded),
        WebRoot::Directory(path) => {
            router.fallback_service(ServeDir::new(&path).not_found_service(
                tower_http::services::ServeFile::new(path.join("index.html")),
            ))
        }
    }
    .with_state(state);

    let abort = reducer.abort_handle();
    let guard = RuntimeGuard {
        stop: stop.clone(),
        reducer,
    };

    println!("Overlay: http://127.0.0.1:{port}/");

    let result = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown.await;
            stop.store(true, Ordering::Relaxed);
            abort.abort();
        })
        .await;

    drop(guard);

    result?;

    Ok(())
}

fn renderer_routes() -> Router<AppState> {
    Router::new()
        .route("/api/events", get(events))
        .route("/api/plugins", get(plugin_list))
        .route("/api/plugins/{id}/view", get(plugin_view))
        .route("/api/plugins/{id}/config", get(plugin_config))
        .route("/plugins/{id}/{*path}", get(plugin_asset))
        .layer(DefaultBodyLimit::max(64 * 1024))
}

async fn embedded(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() || path == "playground" || path == "settings" || path == "control"
    {
        "index.html"
    } else {
        path
    };

    let Some(asset) = EmbeddedWeb::get(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let mime = mime_guess::from_path(path).first_or_octet_stream();
    (
        [(axum::http::header::CONTENT_TYPE, mime.to_string())],
        asset_bytes(asset.data),
    )
        .into_response()
}

fn catalog_json(catalog: &PluginCatalog) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "plugins": catalog.plugins,
        "errors": catalog.errors,
        "revision": catalog.revision,
    }))
}

async fn plugin_list(State(state): State<AppState>) -> Json<serde_json::Value> {
    catalog_json(&state.plugins.catalog())
}

#[derive(serde::Deserialize)]
struct PluginViewQuery {
    preview: Option<String>,
}

async fn plugin_view(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PluginViewQuery>,
) -> Response {
    let Some(plugin) = state.plugins.find(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    // URL 来自已校验的本地 manifest 路径；HTML 属性需要单独转义
    let escape = |value: &str| {
        value
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
    };

    let entry =
        match crate::plugins::resource_url(&id, plugin["entry"].as_str().unwrap_or_default()) {
            Ok(url) => url,
            Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
        };

    let mut styles = String::new();

    for path in plugin["styles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|path| path.as_str())
    {
        let url = match crate::plugins::resource_url(&id, path) {
            Ok(url) => url,
            Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
        };
        styles.push_str(&format!(
            "<link rel=\"stylesheet\" crossorigin href=\"{}\">",
            escape(&url)
        ));
    }

    // 同时设置最内层 document 的画布；透明的 iframe 会露出 WebView 默认的白色画布，
    // 而不是它后面的预览
    let (background, color_scheme) = match query.preview.as_deref() {
        Some("1" | "dark") => ("#000", "dark"),
        Some("light") => ("#fafafa", "light"),
        Some("system") => ("light-dark(#fafafa,#000)", "light dark"),
        _ => ("transparent", "light"),
    };

    // 页面运行在 sandboxed iframe 中（不透明 origin），因此它的 module 与样式都是
    // 跨域获取的；SDK runtime 由 `hysilens build` 打包进入口文件
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><base href=\"/plugins/{id}/\"><style>html,body{{margin:0;width:100%;height:100%;background:{background};color-scheme:{color_scheme}}}*{{box-sizing:border-box;-webkit-user-select:none!important;user-select:none!important}}</style>{styles}</head><body><script type=\"module\" crossorigin src=\"{}\"></script></body></html>",
        escape(&entry)
    );
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "no-store"),
        ],
        html,
    )
        .into_response()
}

/// plugin 页面在沙箱中运行且不带 `allow-same-origin`，因此它发出的每个请求都是
/// 跨域的；资源是公开的只读文件，会修改状态的 API 仍然使用 `allowed_origin`
const PLUGIN_CORS: (axum::http::HeaderName, &str) =
    (axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");

fn asset_bytes(data: Cow<'static, [u8]>) -> axum::body::Bytes {
    match data {
        Cow::Borrowed(bytes) => axum::body::Bytes::from_static(bytes),
        Cow::Owned(bytes) => bytes.into(),
    }
}

fn plugin_bytes(id: &str, path: &str) -> anyhow::Result<axum::body::Bytes> {
    anyhow::ensure!(crate::plugins::valid_id(id), "插件标识无效");

    // 已安装的包是权威来源；不要把旧的嵌入式资源混进其中
    if let Some(directory) = crate::plugins::directory(id)? {
        return Ok(std::fs::read(crate::plugins::local_path(&directory, path)?)?.into());
    }

    anyhow::ensure!(
        !path.contains(['\\', ':'])
            && std::path::Path::new(path)
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_))),
        "插件路径无效"
    );

    EmbeddedWeb::get(&format!(
        "plugins/{}/{path}",
        crate::plugins::local_name(id)
    ))
    .map(|asset| asset_bytes(asset.data))
    .ok_or_else(|| anyhow::anyhow!("插件文件不存在"))
}

async fn plugin_asset(Path((id, path)): Path<(String, String)>) -> Response {
    match plugin_bytes(&id, &path) {
        Ok(bytes) => (
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    mime_guess::from_path(&path)
                        .first_or_octet_stream()
                        .to_string(),
                ),
                (axum::http::header::CACHE_CONTROL, "no-cache".into()),
                (PLUGIN_CORS.0, PLUGIN_CORS.1.into()),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn config_schema(registry: &PluginRegistry, id: &str) -> anyhow::Result<serde_json::Value> {
    let plugin = registry
        .find(id)
        .ok_or_else(|| anyhow::anyhow!("插件不存在"))?;

    anyhow::ensure!(plugin["configSchema"].is_object(), "配置 Schema 缺失");

    Ok(plugin["configSchema"].clone())
}

async fn plugin_config(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match config_schema(&state.plugins, &id).and_then(|schema| {
        Ok(serde_json::json!({ "schema": schema, "config": crate::plugins::read_config(&id)? }))
    }) {
        Ok(value) => (
            [(axum::http::header::CACHE_CONTROL, "no-store")],
            Json(value),
        )
            .into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

fn allowed_origin(headers: &HeaderMap, port: u16) -> bool {
    let origin = headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    [
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
        "http://localhost:5173".into(),
        "http://127.0.0.1:5173".into(),
    ]
    .iter()
    .any(|allowed| allowed == origin)
}

async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !allowed_origin(&headers, state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }

    upgrade
        .max_message_size(4096)
        .on_upgrade(move |socket| stream(socket, state))
}

async fn send(socket: &mut WebSocket, value: ServerMessage<'_>) -> bool {
    socket
        .send(Message::Text(
            serde_json::to_string(&value)
                .expect("Protocol message is serializable")
                .into(),
        ))
        .await
        .is_ok()
}

/// 转发一条 plugin 通知；落后的 receiver 会让客户端重新获取全部内容
async fn send_plugin_event(
    socket: &mut WebSocket,
    event: Result<PluginEvent, tokio::sync::broadcast::error::RecvError>,
    registry: &PluginRegistry,
) -> bool {
    match event {
        Ok(PluginEvent::Plugins { revision }) => {
            send(socket, ServerMessage::Plugins { revision }).await
        }
        Ok(PluginEvent::Config { id }) => {
            send(socket, ServerMessage::PluginConfig { id: &id }).await
        }
        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
            let revision = registry.catalog().revision;

            send(socket, ServerMessage::Plugins { revision }).await
        }
        Err(tokio::sync::broadcast::error::RecvError::Closed) => true,
    }
}

async fn stream(mut socket: WebSocket, mut state: AppState) {
    let mut document_cursor = DocumentCursor::default();
    let mut plugin_events = state.plugins.subscribe();

    loop {
        // 发送当前状态；document 只在变化时发送
        let snapshot = state.snapshots.borrow_and_update().clone();
        let include_document = document_cursor.observe(&snapshot);

        if !send(
            &mut socket,
            ServerMessage::State {
                state: SnapshotUpdate::new(&snapshot, include_document),
            },
        )
        .await
        {
            return;
        }

        loop {
            tokio::select! {
                changed = state.snapshots.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    break;
                }
                event = plugin_events.recv() => {
                    if !send_plugin_event(&mut socket, event, &state.plugins).await {
                        return;
                    }
                }
                message = socket.recv() => {
                    let Some(Ok(message)) = message else {
                        return;
                    };

                    if let Message::Close(_) = message {
                        return;
                    }

                    let Message::Text(text) = message else {
                        continue;
                    };

                    let request = match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(request) => request,
                        Err(error) => {
                            if !send(
                                &mut socket,
                                ServerMessage::Error {
                                    message: &error.to_string(),
                                },
                            )
                            .await
                            {
                                return;
                            }

                            continue;
                        }
                    };

                    if !matches!(request, ClientMessage::Clock { .. }) {
                        if !send(&mut socket, ServerMessage::Error {
                            message: "播放控制仅支持桌面 IPC",
                        }).await {
                            return;
                        }
                        continue;
                    }

                    let result = control::ControlHandle::new(state.clone()).send(request).await;

                    let response = match result {
                        Ok(serde_json::Value::Null) => continue,
                        Ok(value) => value,
                        Err(error) => serde_json::json!({ "kind": "error", "message": error.to_string() }),
                    };

                    if socket.send(Message::Text(response.to_string().into())).await.is_err() {
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
fn test_state() -> (
    AppState,
    watch::Sender<Arc<Snapshot>>,
    mpsc::Receiver<Action>,
) {
    let snapshot = Arc::new(Coordinator::new("ipc-test".into(), vec![]).state);
    let (snapshots_tx, snapshots) = watch::channel(snapshot);
    let (actions, receiver) = mpsc::channel(8);
    let (restart, _) = mpsc::channel(1);

    (
        AppState {
            origin: Instant::now(),
            port: 38473,
            actions,
            snapshots,
            settings: Arc::new(RwLock::new(Settings::default())),
            restart,
            plugins: PluginRegistry::new(vec![], vec![], PathBuf::new()),
            diagnostics: Arc::new(vec![]),
            diagnostic_export: Arc::default(),
            kugou_enhanced: Arc::new(AtomicBool::new(false)),
        },
        snapshots_tx,
        receiver,
    )
}

#[cfg(test)]
mod asset_tests {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn exposes_only_renderer_resources_over_http() {
        let (state, _snapshots, _actions) = test_state();
        let router = renderer_routes().fallback(embedded).with_state(state);

        for (method, path) in [
            ("GET", "/api/settings"),
            ("PUT", "/api/settings"),
            ("GET", "/api/fonts"),
            ("GET", "/api/diagnostics"),
            ("POST", "/api/diagnostics/export"),
            ("POST", "/api/dialog/directory"),
            ("GET", "/api/kugou/enhanced"),
            ("POST", "/api/kugou/enhanced/launch"),
            ("POST", "/api/kugou/enhanced/shortcut"),
            ("POST", "/api/plugins/install"),
            ("POST", "/api/plugins/rescan"),
            ("POST", "/api/plugins/directory"),
            ("DELETE", "/api/plugins/example"),
            ("GET", "/api/state"),
            ("GET", "/api/catalog"),
        ] {
            let request = axum::http::Request::builder()
                .method(method)
                .uri(path)
                .body(axum::body::Body::empty())
                .unwrap();
            let response = router.clone().oneshot(request).await.unwrap();

            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
        }

        let request = axum::http::Request::builder()
            .uri("/api/plugins")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(
            router.oneshot(request).await.unwrap().status(),
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn plugin_config_http_resource_is_read_only() {
        let (state, _snapshots, _actions) = test_state();
        let request = axum::http::Request::builder()
            .method("PUT")
            .uri("/api/plugins/example/config")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = renderer_routes()
            .with_state(state)
            .oneshot(request)
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn serves_static_and_owned_assets_without_copying_their_buffers() {
        static FONT: &[u8] = b"embedded font bytes";
        let borrowed = asset_bytes(Cow::Borrowed(FONT));
        let owned = vec![1, 2, 3, 4];
        let pointer = owned.as_ptr();
        let owned = asset_bytes(Cow::Owned(owned));

        assert_eq!(borrowed.as_ptr(), FONT.as_ptr());
        assert_eq!(borrowed.as_ref(), FONT);
        assert_eq!(owned.as_ptr(), pointer);
        assert_eq!(owned.as_ref(), &[1, 2, 3, 4]);
    }
}
