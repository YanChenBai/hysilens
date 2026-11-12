//! 与桌面 UI 共享的 wire protocol：snapshot、客户端命令与 server 消息

use crate::settings::OverlaySettings;
use crate::{
    model::Entry,
    model::{Document, Playback, Session, Source},
};
use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc};

/// wire protocol 版本；客户端会拒绝携带其他任何值的 snapshot
pub const VERSION: u32 = 1;

/// overlay server 在一次发布时的完整状态
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    // 渲染配置
    /// overlay 渲染所用的 settings
    pub overlay: OverlaySettings,

    // 来源选择与原始 playback 采样
    /// 正在跟随的平台；`Auto` 跟随当前正在播放的平台
    pub source: Source,

    /// 每个 adapter 最新的原始采样
    pub playback: Playback,

    /// 从 `playback` 中为 `source` 选出的 session
    pub session: Option<Session>,

    // 歌词解析
    /// 歌词解析状态，例如 `waiting`、`loading`、`missing`、

    /// `ambiguous`、`manual`、`matchedMetadata` 或 `captureError`
    pub lyrics_status: String,

    /// 可能匹配 `session` 的 cache 条目，供手动选择
    pub candidates: Vec<Entry>,

    /// `session` 的解析后歌词；歌词解析完成前为 `None`
    pub document: Option<Arc<Document>>,

    /// 扫描 cache 目录时收集到的错误
    pub cache_issues: Vec<String>,

    // protocol 记账：客户端据此接受或丢弃保留状态
    /// wire protocol 版本，本 build 中始终为 [`VERSION`]
    pub version: u32,

    /// 服务进程的不透明标识，由 pid 与启动时间推导
    pub epoch: String,

    /// 发布计数器，每发一条 state 消息递增；客户端忽略更小的值
    pub seq: u64,

    /// 每当播放的曲目变化时递增；`select` 必须回显当前值
    pub generation: u64,

    // 由用户持有的呈现状态
    /// 应用于时间轴的歌词偏移，单位为毫秒（`-10000..=10000`）
    pub offset_ms: i32,

    /// overlay 在可用时是否绘制音译演绎
    pub show_romanization: bool,

    /// overlay 在可用时是否绘制翻译演绎
    pub show_translation: bool,
}

/// overlay UI 通过事件 socket 发送的命令
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ClientMessage {
    /// 替换 overlay 外观 settings
    Overlay {
        /// 新 settings；[`OverlaySettings::validate`] 失败时该消息被逐条拒绝
        settings: OverlaySettings,
    },

    /// 跟随另一个平台
    Source {
        /// 要跟随的平台；`auto` 恢复自动选择
        value: Source,
    },

    /// clock 探测，用于估算客户端与 server 的时间偏移
    Clock {
        /// 探测发出时的客户端 clock，单位为毫秒
        #[serde(rename = "clientMs")]
        client_ms: f64,
    },

    /// 为当前歌曲选定一个 cache 条目
    Select {
        /// 客户端认为的当前 generation；过期的选择会被拒绝
        generation: u64,

        /// 所选条目的 key，`None` 表示由平台自动选择
        key: Option<String>,
    },

    /// 设置歌词偏移
    Offset {
        /// 偏移，单位为毫秒，server 会限制到 `-10000..=10000`
        value: i32,
    },

    /// 切换可选的歌词演绎
    Display {
        /// 在可用时绘制音译演绎
        romanization: bool,

        /// 在可用时绘制翻译演绎
        translation: bool,
    },
}

/// server 通过事件 socket 推送的消息
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ServerMessage<'a> {
    /// 一份新发布的 snapshot；`document` 未变化时省略
    State {
        /// snapshot 主体
        state: SnapshotUpdate<'a>,
    },

    /// 对 [`ClientMessage::Clock`] 的回复，用于测量往返时间
    Clock {
        /// 从探测中回显的客户端时间戳，单位为毫秒
        #[serde(rename = "clientMs")]
        client_ms: f64,

        /// 回复发出时的 server 时间戳，单位为毫秒，自 server 启动算起
        #[serde(rename = "serverMs")]
        server_ms: f64,
    },

    /// 某条命令被拒绝；文本会展示给用户
    Error {
        /// 人类可读的原因
        message: &'a str,
    },

    /// plugin catalog 发生变化；客户端重新获取 `/api/plugins`
    Plugins {
        /// 客户端必须重新获取的 catalog revision；在同一进程内单调递增
        revision: u64,
    },

    /// 某个 plugin 保存的 config 发生变化；客户端重新获取 `/api/plugins/{id}/config`
    PluginConfig {
        /// 配置发生变化的 plugin
        id: &'a str,
    },
}

/// [`Snapshot`] 去掉 document 后的借用形式，用作 state 消息的头部
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotHeader<'a> {
    // 渲染配置
    overlay: &'a OverlaySettings,

    source: Source,

    // protocol 记账
    version: u32,

    epoch: &'a str,

    seq: u64,

    generation: u64,

    // playback 与歌词解析
    playback: &'a Playback,

    session: &'a Option<Session>,

    lyrics_status: &'a str,

    candidates: &'a [Entry],

    cache_issues: &'a [String],

    // 由用户持有的呈现状态
    offset_ms: i32,

    show_romanization: bool,

    show_translation: bool,
}

/// state 消息体：snapshot 头部加上可选的歌词 document
#[derive(Debug, Serialize)]
pub struct SnapshotUpdate<'a> {
    #[serde(flatten)]
    header: SnapshotHeader<'a>,

    // 外层 `None` 省略该字段；`Some(&None)` 显式清空之前的歌词
    #[serde(skip_serializing_if = "Option::is_none")]
    document: Option<&'a Option<Arc<Document>>>,
}

impl<'a> SnapshotUpdate<'a> {
    /// 为一条 state 消息借用 `snapshot`
    ///
    /// `include_document` 在 document 变化时携带它；不携带时客户端保留已有的
    /// document，该字段从消息中省略
    pub fn new(snapshot: &'a Snapshot, include_document: bool) -> Self {
        Self {
            header: SnapshotHeader {
                overlay: &snapshot.overlay,
                source: snapshot.source,
                version: snapshot.version,
                epoch: &snapshot.epoch,
                seq: snapshot.seq,
                generation: snapshot.generation,
                playback: &snapshot.playback,
                session: &snapshot.session,
                lyrics_status: &snapshot.lyrics_status,
                candidates: &snapshot.candidates,
                offset_ms: snapshot.offset_ms,
                show_romanization: snapshot.show_romanization,
                show_translation: snapshot.show_translation,
                cache_issues: &snapshot.cache_issues,
            },
            document: include_document.then_some(&snapshot.document),
        }
    }
}

/// 记住上次发送的歌词 document，以便省略未变化的 document
#[derive(Default)]
pub struct DocumentCursor {
    /// 最近一次发送的 document 的 `(epoch, generation, id, filename, warnings)`
    sent: Option<(String, u64, String, String, Vec<String>)>,
}

impl fmt::Debug for DocumentCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.sent {
            Some((epoch, generation, id, filename, warnings)) => formatter
                .debug_struct("DocumentCursor")
                .field("epoch", epoch)
                .field("generation", generation)
                .field("id", id)
                .field("filename", filename)
                .field("warnings", &warnings.len())
                .finish(),
            None => formatter.debug_struct("DocumentCursor").finish(),
        }
    }
}

impl DocumentCursor {
    /// 记录 `snapshot` 并报告其 document 是否必须发给客户端
    ///
    /// epoch、generation、document id、filename 或 warning 列表的变化都算作新的
    /// document；其他情况视为已经交付过
    pub fn observe(&mut self, snapshot: &Snapshot) -> bool {
        let key = (
            snapshot.epoch.clone(),
            snapshot.generation,
            snapshot
                .document
                .as_ref()
                .map_or_else(String::new, |document| document.id.clone()),
            snapshot
                .document
                .as_ref()
                .map_or_else(String::new, |document| document.filename.clone()),
            snapshot
                .document
                .as_ref()
                .map_or_else(Vec::new, |document| document.warnings.clone()),
        );

        let changed = self.sent.as_ref() != Some(&key);

        self.sent = Some(key);

        changed
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn keeps_cache_matching_inputs_out_of_snapshot_and_updates() {
        let mut snapshot = crate::coordinator::Coordinator::new(
            "test".into(),
            crate::platforms::test_descriptors(),
        )
        .state;

        let session = Session {
            source_id: "test-player".into(),
            track_id: Some("opaque-track".into()),
            lyric_match: Some(crate::model::LyricMatch {
                file_hashes: vec!["private-hash".into()],
                preferred_lyric_id: Some("private-lyric".into()),
                metadata_fallback: false,
            }),
            ..Default::default()
        };

        let expected = serde_json::to_value(Session {
            lyric_match: None,
            ..session.clone()
        })
        .unwrap();

        snapshot.session = Some(session.clone());
        snapshot.playback.sessions.push(session);

        for value in [
            serde_json::to_value(&snapshot).unwrap(),
            serde_json::to_value(SnapshotUpdate::new(&snapshot, false)).unwrap(),
        ] {
            assert_eq!(value["session"], expected);
            assert_eq!(value["playback"]["sessions"][0], expected);
            assert_eq!(value["session"]["trackId"], "opaque-track");

            let json = value.to_string();

            assert!(!json.contains("private-hash"));
            assert!(!json.contains("private-lyric"));
        }
    }

    #[test]
    fn keeps_v1_snapshot_shape_and_distinguishes_omission_from_clear() {
        let mut snapshot = crate::coordinator::Coordinator::new(
            "test".into(),
            crate::platforms::test_descriptors(),
        )
        .state;

        assert_eq!(
            serde_json::to_value(SnapshotUpdate::new(&snapshot, true)).unwrap(),
            serde_json::to_value(&snapshot).unwrap()
        );

        let mut cursor = DocumentCursor::default();

        assert!(cursor.observe(&snapshot));

        assert!(!cursor.observe(&snapshot));

        let value = serde_json::to_value(SnapshotUpdate::new(&snapshot, false)).unwrap();

        assert!(value.get("document").is_none());

        snapshot.document = Some(Arc::new(Document {
            id: "A".into(),
            filename: "A".into(),
            metadata: Default::default(),
            tracks: vec![],
            warnings: vec![],
        }));

        assert!(cursor.observe(&snapshot));

        let mut document = (**snapshot.document.as_ref().unwrap()).clone();

        document.warnings.push("diagnostic changed".into());

        snapshot.document = Some(Arc::new(document));

        assert!(cursor.observe(&snapshot));

        snapshot.document = None;

        assert!(cursor.observe(&snapshot));

        assert!(
            serde_json::to_value(SnapshotUpdate::new(&snapshot, true)).unwrap()["document"]
                .is_null()
        );

        snapshot.generation += 1;

        assert!(cursor.observe(&snapshot));
    }

    #[test]
    fn accepts_existing_commands_and_serializes_typed_responses() {
        assert!(matches!(
            serde_json::from_str::<ClientMessage>(r#"{"kind":"source","value":"qqMusic"}"#)
                .unwrap(),
            ClientMessage::Source {
                value: Source::QqMusic
            }
        ));

        assert!(matches!(
            serde_json::from_str::<ClientMessage>(r#"{"kind":"clock","clientMs":123}"#).unwrap(),
            ClientMessage::Clock { client_ms: 123.0 }
        ));

        let message = serde_json::to_value(ServerMessage::Clock {
            client_ms: 123.0,
            server_ms: 456.0,
        })
        .unwrap();

        assert_eq!(
            message,
            serde_json::json!({"kind":"clock","clientMs":123.0,"serverMs":456.0})
        );
    }

    #[test]
    fn serializes_plugin_notifications_for_the_web_client() {
        assert_eq!(
            serde_json::to_value(ServerMessage::Plugins { revision: 3 }).unwrap(),
            serde_json::json!({"kind":"plugins","revision":3})
        );

        assert_eq!(
            serde_json::to_value(ServerMessage::PluginConfig { id: "ink-verse" }).unwrap(),
            serde_json::json!({"kind":"pluginConfig","id":"ink-verse"})
        );
    }
}
