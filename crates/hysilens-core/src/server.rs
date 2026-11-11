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

pub enum WebRoot {
    Embedded,
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
}

pub struct RuntimeOptions {
    pub settings: Settings,
    pub restart: mpsc::Sender<Settings>,
}

pub async fn serve_on(
    listener: tokio::net::TcpListener,
    options: RuntimeOptions,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let source = options.settings.source;
    let registrations = vec![
        crate::platforms::wesing::registration(),
        crate::platforms::qq_music::registration(options.settings.cache_dir.clone()),
        crate::platforms::kugou::registration(options.settings.kugou_cache_dir.clone()),
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
    // Exits within one debounce interval once `stop` is set on shutdown.
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
    };

    let router = Router::new()
        .route("/api/state", get(snapshot))
        .route("/api/fonts", get(system_fonts))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/dialog/directory", post(pick_directory))
        .route(
            "/api/diagnostics/kugou/export",
            post(export_kugou_diagnostics),
        )
        .route("/api/catalog", get(get_catalog))
        .route("/api/events", get(events))
        .route("/api/plugins", get(plugin_list))
        .route("/api/plugins/directory", post(open_plugin_directory))
        .route("/api/plugins/rescan", post(rescan_plugins))
        .route(
            "/api/plugins/install",
            // Overrides the small default limit below for this route only.
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
        asset.data.into_owned(),
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

/// Install a `hysilens pack` archive into the user plugin directory.
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
        // Publish immediately instead of waiting for the watcher's debounce.
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

/// Manual fallback for file systems where change notifications are unreliable.
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
    // URLs come from validated local manifest paths; quote HTML attributes separately.
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
    // Set the innermost document canvas too; a transparent iframe can expose
    // WebView's default white canvas instead of the preview behind it.
    let (background, color_scheme) = match query.preview.as_deref() {
        Some("1" | "dark") => ("#000", "dark"),
        Some("light") => ("#fafafa", "light"),
        Some("system") => ("light-dark(#fafafa,#000)", "light dark"),
        _ => ("transparent", "light"),
    };
    // The page runs in a sandboxed iframe (opaque origin), so its module and styles are
    // fetched cross-origin; the SDK runtime is bundled into the entry by `hysilens build`.
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

/// Plugin pages are sandboxed without `allow-same-origin`, so every request they make is
/// cross-origin. Assets are public, read-only files; mutating APIs keep `allowed_origin`.
const PLUGIN_CORS: (axum::http::HeaderName, &str) =
    (axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");

fn plugin_bytes(id: &str, path: &str) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(crate::plugins::valid_id(id), "插件标识无效");
    // An installed package is authoritative; never mix old embedded assets into it.
    if let Some(directory) = crate::plugins::directory(id)? {
        return Ok(std::fs::read(crate::plugins::local_path(
            &directory, path,
        )?)?);
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
    .map(|asset| asset.data.into_owned())
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
            // Notify at once; the file watcher may report the same write again, harmlessly.
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

async fn export_kugou_diagnostics(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !allowed_origin(&headers, state.port) {
        return StatusCode::FORBIDDEN.into_response();
    }
    #[cfg(windows)]
    {
        let snapshots = state.snapshots.clone();
        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<String>> {
            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let Some(path) = rfd::FileDialog::new()
                .set_title("导出酷狗诊断报告")
                .add_filter("JSON 诊断报告", &["json"])
                .set_file_name(format!("hysilens-kugou-{timestamp}.json"))
                .save_file()
            else {
                return Ok(None);
            };
            let before = diagnostic_runtime_snapshot(snapshots.borrow().as_ref());
            let mut report = crate::platforms::kugou::diagnostic_report();
            report["runtimeBefore"] = before;
            report["runtimeAfter"] = diagnostic_runtime_snapshot(snapshots.borrow().as_ref());
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
    (StatusCode::NOT_IMPLEMENTED, "酷狗诊断报告仅支持 Windows").into_response()
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
        "lyricsStatus": snapshot.lyrics_status,
        "cacheIssues": snapshot.cache_issues,
    })
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

/// Forward one plugin notification; a lagging receiver asks clients to refetch everything.
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
