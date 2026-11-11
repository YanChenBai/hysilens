use crate::settings::OverlaySettings;
use crate::{
    model::Entry,
    model::{Document, Playback, Session, Source},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub overlay: OverlaySettings,
    pub source: Source,
    pub version: u32,
    pub epoch: String,
    pub seq: u64,
    pub generation: u64,
    pub playback: Playback,
    pub session: Option<Session>,
    pub lyrics_status: String,
    pub candidates: Vec<Entry>,
    pub document: Option<Arc<Document>>,
    pub offset_ms: i32,
    pub show_romanization: bool,
    pub show_translation: bool,
    pub cache_issues: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ClientMessage {
    Overlay {
        settings: OverlaySettings,
    },
    Source {
        value: Source,
    },
    Clock {
        #[serde(rename = "clientMs")]
        client_ms: f64,
    },
    Select {
        generation: u64,
        key: Option<String>,
    },
    Offset {
        value: i32,
    },
    Display {
        romanization: bool,
        translation: bool,
    },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ServerMessage<'a> {
    State {
        state: SnapshotUpdate<'a>,
    },
    Clock {
        #[serde(rename = "clientMs")]
        client_ms: f64,
        #[serde(rename = "serverMs")]
        server_ms: f64,
    },
    Error {
        message: &'a str,
    },
    /// The plugin catalog changed; clients refetch `/api/plugins`.
    Plugins {
        revision: u64,
    },
    /// A plugin's saved config changed; clients refetch `/api/plugins/{id}/config`.
    PluginConfig {
        id: &'a str,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotHeader<'a> {
    overlay: &'a OverlaySettings,
    source: Source,
    version: u32,
    epoch: &'a str,
    seq: u64,
    generation: u64,
    playback: &'a Playback,
    session: &'a Option<Session>,
    lyrics_status: &'a str,
    candidates: &'a [Entry],
    offset_ms: i32,
    show_romanization: bool,
    show_translation: bool,
    cache_issues: &'a [String],
}

#[derive(Serialize)]
pub struct SnapshotUpdate<'a> {
    #[serde(flatten)]
    header: SnapshotHeader<'a>,
    // 外层 None 表示省略，Some(&None) 表示明确清除旧歌词。
    #[serde(skip_serializing_if = "Option::is_none")]
    document: Option<&'a Option<Arc<Document>>>,
}

impl<'a> SnapshotUpdate<'a> {
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

#[derive(Default)]
pub struct DocumentCursor {
    sent: Option<(String, u64, String, String, Vec<String>)>,
}

impl DocumentCursor {
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
