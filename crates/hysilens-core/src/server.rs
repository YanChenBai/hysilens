//! HTTP 与 WebSocket server：提供内嵌 UI、snapshot 流以及 plugin API

use crate::plugin_registry::{PluginCatalog, PluginEvent, PluginRegistry};
use crate::protocol::{ClientMessage, DocumentCursor, ServerMessage, Snapshot, SnapshotUpdate};
use crate::{
    capture::{self, Event as CaptureEvent},
    config::Settings,
    coordinator::Coordinator,
    model::Catalog,
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
    routing::{get, post},
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

    catalog: Arc<RwLock<Catalog>>,

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
            "Build the overlay before Rust: vp run @hysilens/desktop-ui#build:web"
        ),
        WebRoot::Directory(path) => anyhow::ensure!(
            path.join("index.html").is_file(),
            "Build the overlay before Rust: vp run @hysilens/desktop-ui#build:web"
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
    let catalog = Arc::new(RwLock::new(Catalog::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let mut captures = capture::spawn(origin, registrations, stop.clone());
    let catalog_state = catalog.clone();
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
                Action::Capture(CaptureEvent::Playback(playback, document)) => {
                    coordinator.sample(playback, document)
                }
                Action::Source(source) => coordinator.source(source),
                Action::Capture(CaptureEvent::Catalog(update)) => {
                    coordinator.catalog(update.source, update.catalog);

                    *catalog_state.write().await = coordinator.all_catalogs();
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
        catalog,
        settings,
        restart: options.restart,
        plugins,
        diagnostics,
        diagnostic_export: Arc::default(),
        kugou_enhanced,
    };

    let router = Router::new()
        .route("/api/state", get(snapshot))
        .route("/api/fonts", get(system_fonts))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/dialog/directory", post(pick_directory))
        .route(
            "/api/kugou/enhanced",
            get(get_kugou_enhanced).put(put_kugou_enhanced),
        )
        .route("/api/kugou/enhanced/launch", post(kugou_launch))
        .route("/api/kugou/enhanced/shortcut", post(kugou_shortcut))
        .route("/api/diagnostics", get(diagnostic_info))
        .route("/api/diagnostics/export", post(export_diagnostics))
        .route("/api/catalog", get(get_catalog))
        .route("/api/events", get(events))
        .route("/api/plugins", get(plugin_list))
        .route("/api/plugins/directory", post(open_plugin_directory))
        .route("/api/plugins/rescan", post(rescan_plugins))
        .route(
            "/api/plugins/install",
            // 仅对该路由覆盖下方较小的默认 body 上限
            post(install_plugin).layer(DefaultBodyLimit::max(crate::plugins::MAX_PACKAGE_BYTES)),
        )
        .route("/api/plugins/{id}/view", get(plugin_view))
        .route(
            "/api/plugins/{id}/config",
            get(plugin_config).put(put_plugin_config),
        )
        .route("/plugins/{id}/{*path}", get(plugin_asset))
        .layer(DefaultBodyLimit::max(64 * 1024));

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

/// 把 `hysilens pack` 压缩包安装到用户 plugin 目录
async fn install_plugin(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if !allowed_origin(&headers, state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let registry = state.plugins.clone();
    let result = tokio::task::spawn_blocking(move || {
        let manifest = crate::plugins::install_package(&crate::plugins::root(), &body)?;

        // 立即发布，而不是等 watcher 的 debounce
        registry.rescan();

        anyhow::Ok(manifest)
    })
    .await;

    match result {
        Ok(Ok(manifest)) => Json(manifest).into_response(),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, format!("{error:#}")).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// 在变更通知不可靠的文件系统上提供手动兜底
async fn rescan_plugins(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !allowed_origin(&headers, state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let registry = state.plugins.clone();

    match tokio::task::spawn_blocking(move || registry.rescan()).await {
        Ok(catalog) => catalog_json(&catalog).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn open_plugin_directory(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !allowed_origin(&headers, state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let result = tokio::task::spawn_blocking(|| -> anyhow::Result<()> {
        let directory = crate::plugins::root();
        std::fs::create_dir_all(&directory)?;

        #[cfg(target_os = "windows")]
        let mut command = std::process::Command::new("explorer.exe");
        #[cfg(target_os = "macos")]
        let mut command = std::process::Command::new("open");
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let mut command = std::process::Command::new("xdg-open");

        command.arg(directory).spawn()?;

        Ok(())
    })
    .await;

    match result {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
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

#[cfg(test)]
mod asset_tests {
    use super::*;

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

async fn put_plugin_config(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(value): Json<serde_json::Value>,
) -> Response {
    if !allowed_origin(&headers, state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }

    match config_schema(&state.plugins, &id)
        .and_then(|schema| crate::plugins::save_config(&id, &schema, &value))
    {
        Ok(()) => {
            // 立即通知；文件 watcher 可能再次上报同一次写入，但无害
            state.plugins.config_changed(&id);

            StatusCode::NO_CONTENT.into_response()
        }
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

async fn snapshot(State(state): State<AppState>) -> Json<Arc<Snapshot>> {
    Json(state.snapshots.borrow().clone())
}

async fn get_settings(State(state): State<AppState>) -> Json<Settings> {
    Json(state.settings.read().await.clone())
}

async fn system_fonts() -> Response {
    match tokio::task::spawn_blocking(crate::fonts::system_families).await {
        Ok(fonts) => Json(fonts).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticInfo {
    source: Option<Source>,

    name: &'static str,

    description: &'static str,
}

async fn diagnostic_info(State(state): State<AppState>) -> Json<DiagnosticInfo> {
    let snapshot = state.snapshots.borrow();
    let descriptors: Vec<_> = state
        .diagnostics
        .iter()
        .map(|(descriptor, _)| *descriptor)
        .collect();
    let target = crate::platforms::diagnostics::target(
        snapshot.source,
        snapshot
            .session
            .as_ref()
            .map(|session| session.source_id.as_str()),
        &descriptors,
    );

    Json(target.map_or(DiagnosticInfo {
        source: None,
        name: "基础诊断",
        description: "当前未命中播放器，仅导出应用版本、平台选择、媒体会话和歌词匹配状态，帮助排查识别失败",
    }, |index| {
        let (descriptor, diagnostics) = &state.diagnostics[index];

        DiagnosticInfo {
            source: Some(descriptor.source),
            name: diagnostics.name,
            description: diagnostics.description,
        }
    }))
}

#[derive(serde::Deserialize)]
struct DiagnosticRequest {
    source: Option<Source>,
}

async fn export_diagnostics(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DiagnosticRequest>,
) -> Response {
    if !allowed_origin(&headers, state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let target = request.source.and_then(|source| {
        state
            .diagnostics
            .iter()
            .find(|(descriptor, _)| descriptor.source == source)
            .cloned()
    });

    if request.source.is_some() && target.is_none() {
        return (StatusCode::BAD_REQUEST, "未知的诊断平台").into_response();
    }

    let Ok(guard) = state.diagnostic_export.clone().try_lock_owned() else {
        return (StatusCode::CONFLICT, "已有诊断报告正在导出").into_response();
    };

    #[cfg(windows)]
    {
        let snapshots = state.snapshots.clone();
        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<String>> {
            let _guard = guard;
            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let name = target
                .as_ref()
                .map_or("基础诊断", |(_, diagnostics)| diagnostics.name);
            let source = request.source.map(serde_json::to_value).transpose()?;
            let slug = source
                .as_ref()
                .and_then(|source| source.as_str())
                .unwrap_or("base");
            let Some(path) = rfd::FileDialog::new()
                .set_title(format!("导出{name}报告"))
                .add_filter("JSON 诊断报告", &["json"])
                .set_file_name(format!("hysilens-{slug}-{timestamp}.json"))
                .save_file()
            else {
                return Ok(None);
            };

            let initial = snapshots.borrow().clone();
            let before = diagnostic_runtime_snapshot(&initial);
            let platform = target.map(|(_, diagnostics)| (diagnostics.collect)(&initial));
            let report = serde_json::json!({
                "schemaVersion": 1,
                "hysilensVersion": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "hostArchitecture": std::env::consts::ARCH,
                "source": request.source,
                "platform": platform,
                "runtimeBefore": before,
                "runtimeAfter": diagnostic_runtime_snapshot(snapshots.borrow().as_ref()),
            });

            std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;

            Ok(Some(path.to_string_lossy().into_owned()))
        })
        .await;

        match result {
            Ok(Ok(path)) => Json(serde_json::json!({ "path": path })).into_response(),
            Ok(Err(error)) => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")).into_response()
            }
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (guard, target);

        (StatusCode::NOT_IMPLEMENTED, "诊断报告导出仅支持 Windows").into_response()
    }
}

#[cfg(windows)]
fn diagnostic_runtime_snapshot(snapshot: &Snapshot) -> serde_json::Value {
    serde_json::json!({
        "capturedUnixMs": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
        "hysilensVersion": env!("CARGO_PKG_VERSION"),
        "protocolVersion": snapshot.version,
        "epoch": snapshot.epoch,
        "seq": snapshot.seq,
        "generation": snapshot.generation,
        "source": snapshot.source,
        "playback": snapshot.playback,
        "session": snapshot.session,
        "lyricMatch": snapshot.session.as_ref().and_then(|session| session.lyric_match.as_ref()).map(|identity| serde_json::json!({
            "fileHashes": identity.file_hashes,
            "preferredLyricId": identity.preferred_lyric_id,
        })),
        "lyricsStatus": snapshot.lyrics_status,
        "candidates": snapshot.candidates,
        "document": snapshot.document.as_ref().map(|document| serde_json::json!({
            "id": document.id,
            "metadata": document.metadata,
            "warnings": document.warnings,
        })),
        "cacheIssues": snapshot.cache_issues,
    })
}

#[cfg(all(test, windows))]
mod diagnostic_tests {
    use super::*;
    use crate::model::{Document, LyricMatch, Metadata, Session};

    #[test]
    fn exports_internal_matching_evidence_and_document_summary_without_changing_wire() {
        let mut snapshot = Coordinator::new("diagnostic-test".into(), vec![]).state;
        snapshot.session = Some(Session {
            lyric_match: Some(LyricMatch {
                file_hashes: vec!["audio-hash".into()],
                preferred_lyric_id: Some("42".into()),
                metadata_fallback: false,
            }),
            ..Session::default()
        });
        snapshot.document = Some(Arc::new(Document {
            id: "document".into(),
            filename: "song.qrc".into(),
            metadata: Metadata::default(),
            tracks: vec![],
            warnings: vec!["translation unavailable".into()],
        }));

        let report = diagnostic_runtime_snapshot(&snapshot);
        let wire = serde_json::to_value(&snapshot).unwrap();

        assert_eq!(report["lyricMatch"]["fileHashes"][0], "audio-hash");
        assert_eq!(report["lyricMatch"]["preferredLyricId"], "42");
        assert_eq!(report["document"]["warnings"][0], "translation unavailable");
        assert!(report["document"].get("tracks").is_none());
        assert!(wire["session"].get("lyricMatch").is_none());
    }
}

#[derive(serde::Deserialize)]
struct DirectoryRequest {
    directory: Option<String>,
}

async fn pick_directory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DirectoryRequest>,
) -> Response {
    if !allowed_origin(&headers, state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }

    #[cfg(windows)]
    {
        let selected = tokio::task::spawn_blocking(move || {
            let mut dialog = rfd::FileDialog::new().set_title("选择歌词缓存目录");

            if let Some(directory) = request.directory.filter(|value| !value.is_empty()) {
                dialog = dialog.set_directory(directory);
            }

            dialog
                .pick_folder()
                .map(|path| path.to_string_lossy().into_owned())
        })
        .await;

        match selected {
            Ok(path) => Json(serde_json::json!({ "path": path })).into_response(),
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = request.directory;
        (StatusCode::NOT_IMPLEMENTED, "当前平台不支持文件夹选择").into_response()
    }
}

/// 酷狗增强模式的当前状态，供设置页显示
async fn get_kugou_enhanced(State(state): State<AppState>) -> Response {
    use crate::platforms::kugou::cdp;

    let enabled = state.settings.read().await.kugou_cdp;
    let (port_open, running) = tokio::task::spawn_blocking(|| {
        #[cfg(windows)]
        let running = crate::platforms::kugou::launcher::running();
        #[cfg(not(windows))]
        let running = false;

        (cdp::port_open(cdp::PORT), running)
    })
    .await
    .unwrap_or_default();
    let status = cdp::status();

    Json(serde_json::json!({
        "enabled": enabled,
        "port": cdp::PORT,
        "portOpen": port_open,
        "kugouRunning": running,
        "connected": enabled && status.connected,
        "error": if enabled { status.error } else { None },
    }))
    .into_response()
}

#[derive(serde::Deserialize)]
struct EnhancedRequest {
    enabled: bool,
}

/// 切换酷狗增强模式并持久化；只更新运行时开关，不重启服务
async fn put_kugou_enhanced(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<EnhancedRequest>,
) -> Response {
    if !allowed_origin(&headers, state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let mut settings = state.settings.write().await;
    let previous = settings.kugou_cdp;

    settings.kugou_cdp = request.enabled;

    if let Err(error) = settings.save() {
        settings.kugou_cdp = previous;

        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("保存设置失败: {error:#}"),
        )
            .into_response();
    }

    state
        .kugou_enhanced
        .store(request.enabled, Ordering::Relaxed);

    StatusCode::NO_CONTENT.into_response()
}

/// 以增强模式启动酷狗；酷狗已以普通模式运行时返回 409，不会结束它
async fn kugou_launch(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !allowed_origin(&headers, state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }

    #[cfg(windows)]
    {
        use crate::platforms::kugou::launcher::{Launch, launch};

        match tokio::task::spawn_blocking(launch).await {
            Ok(Ok(Launch::NormalMode)) => (
                StatusCode::CONFLICT,
                "酷狗正以普通模式运行，请从托盘完全退出酷狗后再启动",
            )
                .into_response(),
            Ok(Ok(result)) => Json(serde_json::json!({ "result": result })).into_response(),
            Ok(Err(error)) => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")).into_response()
            }
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        }
    }
    #[cfg(not(windows))]
    {
        (StatusCode::NOT_IMPLEMENTED, "当前平台不支持启动酷狗").into_response()
    }
}

/// 在桌面创建以增强模式启动酷狗的快捷方式
async fn kugou_shortcut(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !allowed_origin(&headers, state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }

    #[cfg(windows)]
    {
        match tokio::task::spawn_blocking(crate::platforms::kugou::launcher::create_shortcut).await
        {
            Ok(Ok(path)) => Json(serde_json::json!({ "path": path })).into_response(),
            Ok(Err(error)) => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}")).into_response()
            }
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        }
    }
    #[cfg(not(windows))]
    {
        (StatusCode::NOT_IMPLEMENTED, "当前平台不支持创建快捷方式").into_response()
    }
}

async fn put_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(settings): Json<Settings>,
) -> Response {
    if !allowed_origin(&headers, state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }

    if let Err(error) = settings.validate() {
        return (StatusCode::BAD_REQUEST, error.to_string()).into_response();
    }

    if settings.port != state.port
        && let Err(error) =
            std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, settings.port))
    {
        return (StatusCode::BAD_REQUEST, format!("端口不可用: {error}")).into_response();
    }

    if let Err(error) = state.restart.send(settings).await {
        return (StatusCode::SERVICE_UNAVAILABLE, error.to_string()).into_response();
    }

    StatusCode::ACCEPTED.into_response()
}

async fn get_catalog(State(state): State<AppState>) -> Json<Catalog> {
    Json(state.catalog.read().await.clone())
}

async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let origin = headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    let is_allowed = [
        format!("http://127.0.0.1:{}", state.port),
        format!("http://localhost:{}", state.port),
        "http://localhost:5173".into(),
        "http://127.0.0.1:5173".into(),
    ]
    .contains(&origin.to_string());

    if !is_allowed {
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

                    match request {
                        ClientMessage::Overlay { settings } => {
                            if let Err(error) = settings.validate() {
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

                            if state.actions.send(Action::Overlay(settings)).await.is_err() {
                                return;
                            }
                        }
                        ClientMessage::Source { value } => {
                            if state.actions.send(Action::Source(value)).await.is_err() {
                                return;
                            }
                        }
                        ClientMessage::Clock { client_ms } => {
                            if !send(
                                &mut socket,
                                ServerMessage::Clock {
                                    client_ms,
                                    server_ms: state.origin.elapsed().as_secs_f64() * 1000.0,
                                },
                            )
                            .await
                            {
                                return;
                            }
                        }
                        ClientMessage::Select { generation, key } => {
                            let (reply, response) = oneshot::channel();

                            if state
                                .actions
                                .send(Action::Select {
                                    generation,
                                    key,
                                    reply,
                                })
                                .await
                                .is_err()
                            {
                                return;
                            }

                            if let Ok(Err(message)) = response.await
                                && !send(&mut socket, ServerMessage::Error { message: &message }).await
                            {
                                return;
                            }
                        }
                        ClientMessage::Offset { value } => {
                            if state.actions.send(Action::Offset(value)).await.is_err() {
                                return;
                            }
                        }
                        ClientMessage::Display {
                            romanization,
                            translation,
                        } => {
                            if state
                                .actions
                                .send(Action::Display {
                                    romanization,
                                    translation,
                                })
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
            }
        }
    }
}
