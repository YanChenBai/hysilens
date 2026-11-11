use serde::Serialize;

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, serde::Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    #[default]
    Auto,
    Wesing,
    QqMusic,
    Kugou,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub source_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub status: String,
    pub position_ms: Option<f64>,
    pub duration_ms: Option<f64>,
    pub source_position_ms: Option<f64>,
    pub source_updated_unix_ms: Option<f64>,
    pub rate: Option<f64>,
    pub rate_basis: String,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,
    // Internal cache matching inputs never become part of the wire protocol.
    #[serde(skip)]
    pub(crate) lyric_match: Option<LyricMatch>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LyricMatch {
    pub file_hashes: Vec<String>,
    pub preferred_lyric_id: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playback {
    pub server_ms: f64,
    pub sessions: Vec<Session>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub text: String,
    pub start_ms: i32,
    pub end_ms: i32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub start_ms: i32,
    pub end_ms: Option<i32>,
    pub text: String,
    pub tokens: Vec<Token>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricTrack {
    pub kind: String,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: String,
    pub filename: String,
    pub metadata: Metadata,
    pub tracks: Vec<LyricTrack>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Metadata {
    pub title: String,
    pub artist: String,
    pub album: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub key: String,
    pub filename: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub line_count: usize,
    #[serde(skip)]
    pub document: std::sync::Arc<Document>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    pub issues: Vec<String>,
}
