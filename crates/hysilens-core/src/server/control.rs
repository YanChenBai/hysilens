//! 进程内控制入口；桌面 IPC 持有唯一的核心运行状态和控制操作

use super::{Action, AppState, config_schema};
#[cfg(windows)]
use crate::protocol::Snapshot;
use crate::{
    config::Settings,
    model::Source,
    plugin_registry::{PluginCatalog, PluginEvent},
    protocol::{ClientMessage, DocumentCursor, ServerMessage, SnapshotUpdate},
};
use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;
use std::sync::atomic::Ordering;
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

/// 控制面板可调用的操作；不提供任意 URL 代理
#[derive(Debug, Deserialize)]
#[serde(tag = "method", rename_all = "camelCase", deny_unknown_fields)]
pub enum ControlRequest {
    /// 读取系统设置
    SettingsGet,

    /// 保存系统设置并请求服务重启
    SettingsSet {
        /// 完整的新设置
        settings: Box<Settings>,
    },

    /// 枚举系统字体
    FontsList,

    /// 读取 plugin catalog
    PluginsList,

    /// 重新扫描 plugin 目录
    PluginsRescan,

    /// 在文件管理器中打开 plugin 目录
    PluginsOpenDirectory,

    /// 移除外部 plugin
    PluginsRemove {
        /// 完整 plugin id
        id: String,
    },

    /// 读取 plugin schema 与配置
    PluginConfigGet {
        /// 完整 plugin id
        id: String,
    },

    /// 校验并保存 plugin 配置
    PluginConfigSet {
        /// 完整 plugin id
        id: String,

        /// 待保存的配置
        config: Value,
    },

    /// 显示原生目录选择窗口
    DirectoryPick {
        /// 初始目录；不作为授权范围
        directory: Option<String>,
    },

    /// 读取诊断平台说明
    DiagnosticsGet,

    /// 导出诊断报告
    DiagnosticsExport {
        /// 已确认的诊断平台
        source: Option<Source>,
    },

    /// 更新酷狗增强开关
    KugouSet {
        /// 是否启用
        enabled: bool,
    },

    /// 启动增强模式的酷狗
    KugouLaunch,

    /// 创建增强模式快捷方式
    KugouShortcut,

    /// 发送播放控制或 clock 探测
    SessionSend {
        /// 与 WebSocket 共用的客户端消息
        message: Box<ClientMessage>,
    },
}

/// 指向某一代核心服务的控制句柄；重启后由 host 替换
#[derive(Clone)]
pub struct ControlHandle {
    state: AppState,
}

impl std::fmt::Debug for ControlHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ControlHandle")
            .finish_non_exhaustive()
    }
}

impl ControlHandle {
    pub(super) fn new(state: AppState) -> Self {
        Self { state }
    }

    /// 调用白名单中的控制操作；不经过 loopback HTTP
    ///
    /// # Errors
    /// 参数校验、文件系统、平台操作或服务通信失败时返回错误
    pub async fn request(&self, request: ControlRequest) -> anyhow::Result<Value> {
        let state = self.state.clone();

        match request {
            ControlRequest::SettingsGet => {
                serde_json::to_value(state.settings.read().await.clone()).map_err(Into::into)
            }
            ControlRequest::SettingsSet { settings } => put_settings(state, *settings).await,
            ControlRequest::FontsList => serde_json::to_value(
                tokio::task::spawn_blocking(crate::fonts::system_families).await?,
            )
            .map_err(Into::into),
            ControlRequest::PluginsList => Ok(catalog_value(&state.plugins.catalog())),
            ControlRequest::PluginsRescan => rescan_plugins(state).await,
            ControlRequest::PluginsOpenDirectory => open_plugin_directory().await,
            ControlRequest::PluginsRemove { id } => remove_plugin(state, id).await,
            ControlRequest::PluginConfigGet { id } => {
                let schema = config_schema(&state.plugins, &id)?;

                Ok(
                    serde_json::json!({ "schema": schema, "config": crate::plugins::read_config(&id)? }),
                )
            }
            ControlRequest::PluginConfigSet { id, config } => {
                put_plugin_config(state, id, config).await
            }
            ControlRequest::DirectoryPick { directory } => pick_directory(directory).await,
            ControlRequest::DiagnosticsGet => diagnostic_info(state).await,
            ControlRequest::DiagnosticsExport { source } => export_diagnostics(state, source).await,
            ControlRequest::KugouSet { enabled } => put_kugou_enhanced(state, enabled).await,
            ControlRequest::KugouLaunch => kugou_launch().await,
            ControlRequest::KugouShortcut => kugou_shortcut().await,
            ControlRequest::SessionSend { message } => self.send(*message).await,
        }
    }

    /// 安装原始 ZIP 字节，复用归档大小和内容校验
    ///
    /// # Errors
    /// ZIP 超限、校验或安装失败时返回错误
    pub async fn install(&self, bytes: Vec<u8>) -> anyhow::Result<Value> {
        anyhow::ensure!(
            bytes.len() <= crate::plugins::MAX_PACKAGE_BYTES,
            "插件包过大"
        );
        let registry = self.state.plugins.clone();

        tokio::task::spawn_blocking(move || {
            let manifest = crate::plugins::install_package(&crate::plugins::root(), &bytes)?;
            registry.rescan();

            Ok(manifest)
        })
        .await?
    }

    /// 校验播放控制消息并交给唯一的 reducer
    ///
    /// # Errors
    /// 消息无效、选择已过期或 reducer 已停止时返回错误
    pub async fn send(&self, message: ClientMessage) -> anyhow::Result<Value> {
        let action = match message {
            ClientMessage::Overlay { settings } => {
                settings.validate()?;

                Action::Overlay(settings)
            }
            ClientMessage::Source { value } => Action::Source(value),
            ClientMessage::Clock { client_ms } => {
                anyhow::ensure!(client_ms.is_finite(), "clock 时间无效");

                return Ok(serde_json::to_value(ServerMessage::Clock {
                    client_ms,
                    server_ms: self.state.origin.elapsed().as_secs_f64() * 1000.0,
                })?);
            }
            ClientMessage::Select { generation, key } => {
                let (reply, response) = oneshot::channel();
                self.state
                    .actions
                    .send(Action::Select {
                        generation,
                        key,
                        reply,
                    })
                    .await?;
                response.await?.map_err(anyhow::Error::msg)?;

                return Ok(Value::Null);
            }
            ClientMessage::Offset { value } => Action::Offset(value),
            ClientMessage::Display {
                romanization,
                translation,
            } => Action::Display {
                romanization,
                translation,
            },
        };

        self.state.actions.send(action).await?;

        Ok(Value::Null)
    }

    /// 转发 snapshot 与 plugin 通知；接收端关闭或核心服务停止时退出
    ///
    /// # Errors
    /// 状态无法序列化时返回错误
    pub async fn forward(
        &self,
        send: impl Fn(Value) -> bool + Send + Sync + Clone,
    ) -> anyhow::Result<()> {
        tokio::select! {
            result = self.forward_state(send.clone()) => result,
            result = self.forward_kugou(send) => result,
        }
    }

    async fn forward_kugou(&self, send: impl Fn(Value) -> bool + Send) -> anyhow::Result<()> {
        let mut previous = Value::Null;

        loop {
            let status = get_kugou_enhanced(self.state.clone()).await?;

            if status != previous {
                if !send(serde_json::json!({ "kind": "kugou", "status": status })) {
                    return Ok(());
                }

                previous = status;
            }

            /* OS 进程和调试端口由后端采样，状态变化才推送，不需要前端定时调用 IPC */
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    async fn forward_state(&self, send: impl Fn(Value) -> bool + Send) -> anyhow::Result<()> {
        let mut snapshots = self.state.snapshots.clone();
        let mut plugins = self.state.plugins.subscribe();
        let mut cursor = DocumentCursor::default();

        loop {
            let snapshot = snapshots.borrow_and_update().clone();
            let include_document = cursor.observe(&snapshot);

            let mut value = serde_json::to_value(ServerMessage::State {
                state: SnapshotUpdate::new(&snapshot, include_document),
            })?;
            value["sentAtMs"] =
                serde_json::json!(self.state.origin.elapsed().as_secs_f64() * 1000.0);

            if !send(value) {
                return Ok(());
            }

            loop {
                let event = tokio::select! {
                    changed = snapshots.changed() => {
                        if changed.is_err() {
                            return Ok(());
                        }
                        break;
                    }
                    event = plugins.recv() => event,
                };

                let value = match event {
                    Ok(PluginEvent::Plugins { revision }) => {
                        serde_json::to_value(ServerMessage::Plugins { revision })?
                    }
                    Ok(PluginEvent::Config { id }) => {
                        serde_json::to_value(ServerMessage::PluginConfig { id: &id })?
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        serde_json::json!({ "kind": "resync" })
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
                };

                if !send(value) {
                    return Ok(());
                }
            }
        }
    }
}

fn catalog_value(catalog: &PluginCatalog) -> Value {
    serde_json::json!({
        "plugins": catalog.plugins,
        "errors": catalog.errors,
        "revision": catalog.revision,
    })
}

/// 删除外部 plugin 并通知所有客户端；当前使用的 plugin 由 reducer 切回默认值
async fn remove_plugin(state: AppState, id: String) -> anyhow::Result<Value> {
    let registry = state.plugins.clone();
    let removed_id = id.clone();
    let catalog = tokio::task::spawn_blocking(move || {
        crate::plugins::remove(&crate::plugins::root(), &removed_id)?;

        anyhow::Ok(registry.rescan())
    })
    .await??;

    state.actions.send(Action::PluginRemoved(id)).await?;

    Ok(catalog_value(&catalog))
}

/// 在变更通知不可靠的文件系统上提供手动兜底
async fn rescan_plugins(state: AppState) -> anyhow::Result<Value> {
    let registry = state.plugins.clone();
    let catalog = tokio::task::spawn_blocking(move || registry.rescan()).await?;

    Ok(catalog_value(&catalog))
}

async fn open_plugin_directory() -> anyhow::Result<Value> {
    tokio::task::spawn_blocking(|| -> anyhow::Result<()> {
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
    .await??;

    Ok(Value::Null)
}

async fn put_plugin_config(state: AppState, id: String, value: Value) -> anyhow::Result<Value> {
    let schema = config_schema(&state.plugins, &id)?;
    crate::plugins::save_config(&id, &schema, &value)?;
    state.plugins.config_changed(&id);

    Ok(Value::Null)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticInfo {
    source: Option<Source>,

    name: &'static str,

    description: &'static str,
}

async fn diagnostic_info(state: AppState) -> anyhow::Result<Value> {
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

    serde_json::to_value(target.map_or(DiagnosticInfo {
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
    })).map_err(Into::into)
}

async fn export_diagnostics(state: AppState, source: Option<Source>) -> anyhow::Result<Value> {
    let target = source.and_then(|source| {
        state
            .diagnostics
            .iter()
            .find(|(descriptor, _)| descriptor.source == source)
            .cloned()
    });

    anyhow::ensure!(source.is_none() || target.is_some(), "未知的诊断平台");
    let guard = state
        .diagnostic_export
        .clone()
        .try_lock_owned()
        .context("已有诊断报告正在导出")?;

    #[cfg(windows)]
    {
        let snapshots = state.snapshots.clone();
        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<String>> {
            let _guard = guard;
            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let name = target
                .as_ref()
                .map_or("基础诊断", |(_, diagnostics)| diagnostics.name);
            let source = source.map(serde_json::to_value).transpose()?;
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
                "source": source,
                "platform": platform,
                "runtimeBefore": before,
                "runtimeAfter": diagnostic_runtime_snapshot(snapshots.borrow().as_ref()),
            });

            std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;

            Ok(Some(path.to_string_lossy().into_owned()))
        })
        .await;

        Ok(serde_json::json!({ "path": result?? }))
    }
    #[cfg(not(windows))]
    {
        let _ = (guard, target);

        anyhow::bail!("诊断报告导出仅支持 Windows")
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

async fn pick_directory(directory: Option<String>) -> anyhow::Result<Value> {
    #[cfg(windows)]
    {
        let path = tokio::task::spawn_blocking(move || {
            let mut dialog = rfd::FileDialog::new().set_title("选择歌词缓存目录");

            if let Some(directory) = directory.filter(|value| !value.is_empty()) {
                dialog = dialog.set_directory(directory);
            }

            dialog
                .pick_folder()
                .map(|path| path.to_string_lossy().into_owned())
        })
        .await?;

        Ok(serde_json::json!({ "path": path }))
    }
    #[cfg(not(windows))]
    {
        let _ = directory;

        anyhow::bail!("当前平台不支持文件夹选择")
    }
}

/// 酷狗增强模式的当前状态，供设置页显示
async fn get_kugou_enhanced(state: AppState) -> anyhow::Result<Value> {
    use crate::platforms::kugou::cdp;

    let enabled = state.settings.read().await.kugou_cdp;
    let (port_open, endpoint_available, running) = tokio::task::spawn_blocking(|| {
        #[cfg(windows)]
        let running = crate::platforms::kugou::launcher::running();
        #[cfg(not(windows))]
        let running = false;

        let port_open = cdp::port_open(cdp::PORT);
        let endpoint_available = port_open && cdp::endpoint_available(cdp::PORT).unwrap_or(false);

        (port_open, endpoint_available, running)
    })
    .await?;
    let status = cdp::status();

    Ok(serde_json::json!({
        "enabled": enabled,
        "port": cdp::PORT,
        "portOpen": port_open,
        "endpointAvailable": endpoint_available,
        "kugouRunning": running,
        "connected": enabled && status.connected,
        "error": if enabled { status.error } else { None },
    }))
}

/// 切换酷狗增强模式并持久化；只更新运行时开关，不重启服务
async fn put_kugou_enhanced(state: AppState, enabled: bool) -> anyhow::Result<Value> {
    let mut settings = state.settings.write().await;
    let previous = settings.kugou_cdp;
    settings.kugou_cdp = enabled;

    if let Err(error) = settings.save() {
        settings.kugou_cdp = previous;

        return Err(error.context("保存设置失败"));
    }

    state.kugou_enhanced.store(enabled, Ordering::Relaxed);

    Ok(Value::Null)
}

/// 以增强模式启动酷狗；酷狗已以普通模式运行时返回 409，不会结束它
async fn kugou_launch() -> anyhow::Result<Value> {
    #[cfg(windows)]
    {
        use crate::platforms::kugou::launcher::{Launch, launch};
        let result = tokio::task::spawn_blocking(launch).await??;
        anyhow::ensure!(
            !matches!(result, Launch::NormalMode),
            "酷狗正以普通模式运行，请从托盘完全退出酷狗后再启动"
        );

        Ok(serde_json::json!({ "result": result }))
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("当前平台不支持启动酷狗")
    }
}

/// 在桌面创建以增强模式启动酷狗的快捷方式
async fn kugou_shortcut() -> anyhow::Result<Value> {
    #[cfg(windows)]
    {
        let path = tokio::task::spawn_blocking(crate::platforms::kugou::launcher::create_shortcut)
            .await??;

        Ok(serde_json::json!({ "path": path }))
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("当前平台不支持创建快捷方式")
    }
}

async fn put_settings(state: AppState, settings: Settings) -> anyhow::Result<Value> {
    settings.validate()?;

    if settings.port != state.port {
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, settings.port))
            .context("端口不可用")?;
    }

    state
        .restart
        .send(settings)
        .await
        .context("核心服务已停止")?;

    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::super::test_state;
    use super::*;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn reads_the_shared_settings_and_registry() {
        let (state, _snapshots, _actions) = test_state();
        let control = ControlHandle::new(state.clone());
        state.settings.write().await.port = 43123;

        assert_eq!(
            control.request(ControlRequest::SettingsGet).await.unwrap()["port"],
            43123
        );
        assert_eq!(
            control.request(ControlRequest::PluginsList).await.unwrap()["revision"],
            1
        );
    }

    #[tokio::test]
    async fn validates_controls_before_enqueueing_and_propagates_selection_errors() {
        let (state, _snapshots, mut actions) = test_state();
        let control = ControlHandle::new(state);
        let invalid = crate::settings::OverlaySettings {
            visible_lines: 0,
            ..Default::default()
        };

        assert!(
            control
                .send(ClientMessage::Overlay { settings: invalid })
                .await
                .is_err()
        );
        assert!(actions.try_recv().is_err());

        let (result, ()) = tokio::join!(
            control.send(ClientMessage::Select {
                generation: 999,
                key: None
            }),
            async {
                let Action::Select { reply, .. } = actions.recv().await.unwrap() else {
                    panic!("expected selection action");
                };
                reply.send(Err("歌曲已切换".into())).unwrap();
            }
        );

        assert_eq!(result.unwrap_err().to_string(), "歌曲已切换");
    }

    #[tokio::test]
    async fn forwards_initial_state_deltas_and_plugin_events() {
        let (state, snapshots, _actions) = test_state();
        let plugins = state.plugins.clone();
        let control = ControlHandle::new(state);
        let (events, mut received) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            control
                .forward_state(|value| events.send(value).is_ok())
                .await
                .unwrap();
        });

        let first = received.recv().await.unwrap();
        assert_eq!(first["state"]["epoch"], "ipc-test");
        assert!(first["sentAtMs"].is_number());
        assert!(first["state"].get("document").is_some());

        snapshots.send_modify(|snapshot| Arc::make_mut(snapshot).seq += 1);
        let next = received.recv().await.unwrap();
        assert_eq!(next["state"]["seq"], 1);
        assert!(next["state"].get("document").is_none());

        plugins.config_changed("example");
        assert_eq!(
            received.recv().await.unwrap(),
            serde_json::json!({ "kind": "pluginConfig", "id": "example" })
        );
        drop(snapshots);
        task.await.unwrap();
    }

    #[test]
    fn rejects_unknown_operations_and_extra_arguments() {
        for value in [
            serde_json::json!({ "method": "fetch", "url": "file:///secret" }),
            serde_json::json!({ "method": "pluginsRemove", "id": "x", "force": true }),
            serde_json::json!({ "method": "directoryPick", "directory": 42 }),
            serde_json::json!({ "method": "kugouGet" }),
        ] {
            assert!(serde_json::from_value::<ControlRequest>(value).is_err());
        }
    }

    #[tokio::test]
    async fn preserves_operation_errors_and_empty_success() {
        let (state, _snapshots, mut actions) = test_state();
        let control = ControlHandle::new(state);

        let error = control
            .request(ControlRequest::PluginsRemove {
                id: "builtin:scroll".into(),
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("内置插件不能删除"));
        assert_eq!(
            control
                .send(ClientMessage::Offset { value: 10 })
                .await
                .unwrap(),
            Value::Null
        );
        assert!(matches!(actions.recv().await, Some(Action::Offset(10))));
    }
}

#[cfg(all(test, windows))]
mod diagnostic_tests {
    use super::*;
    use crate::coordinator::Coordinator;
    use crate::model::{Document, LyricMatch, Metadata, Session};
    use std::sync::Arc;

    #[test]
    fn exports_internal_matching_evidence_and_document_summary_without_changing_wire() {
        let mut snapshot = Coordinator::new("diagnostic-test".into(), vec![]).state;
        snapshot.session = Some(Session {
            lyric_match: Some(LyricMatch {
                file_hashes: vec!["audio-hash".into()],
                preferred_lyric_id: Some("42".into()),
                preferred_document_id: None,
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
