//! platform adapter、coordinator 与 wire protocol 共用的领域类型

use serde::Serialize;

/// overlay 跟随的音乐播放器
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, serde::Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// 跟随当前报告了可播放 session 的平台
    #[default]
    Auto,

    /// 全民K歌
    Wesing,

    /// QQ 音乐
    QqMusic,

    /// 酷狗
    Kugou,
}

/// 从 platform adapter 采样到的一次播放器 session
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// 产生该采样的 adapter；与 adapter id 比较时忽略大小写
    pub source_id: String,

    /// 播放器窗口显示的曲目元数据
    pub title: String,

    /// 播放器显示的曲目艺人；未知时为空
    pub artist: String,

    /// 播放器显示的专辑名；未知时为空
    pub album: String,

    /// 播放器状态，例如 `playing`、`paused`、`opened`、`closed` 或 `stopped`
    pub status: String,

    // 时间线数值单位为毫秒；每项在播放器上报之前保持 `None`
    /// 当前播放位置
    pub position_ms: Option<f64>,

    /// 曲目总时长，也用于区分其他方面完全相同的 cache 条目
    pub duration_ms: Option<f64>,

    /// 平台上报的位置，未经客户端 clock 平滑
    pub source_position_ms: Option<f64>,

    /// 采样 `source_position_ms` 时的 Unix 毫秒时间戳
    pub source_updated_unix_ms: Option<f64>,

    /// 客户端对流逝时间应用的倍速系数；`1.0` 为正常播放
    pub rate: Option<f64>,

    /// 记录 `rate` 来源的自由格式标签
    pub rate_basis: String,

    /// 上一次采样失败的原因（若有）
    pub error: Option<String>,

    /// 不透明的原生曲目 id；只做相等比较，不要解析
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_id: Option<String>,

    // 内部的 cache 匹配输入永不进入 wire protocol
    #[serde(skip)]
    pub(crate) lyric_match: Option<LyricMatch>,
}

/// 从播放器推导出的 cache 匹配输入，不进入 wire
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LyricMatch {
    /// 平台为当前曲目关联的音频 hash
    pub file_hashes: Vec<String>,

    /// 平台本身推荐的歌词 id（若有）
    pub preferred_lyric_id: Option<String>,

    /// 按当前播放 hash 下载的推荐歌词内容标识，用于确认跨 hash 缓存
    pub preferred_document_id: Option<String>,

    /// hash 没有匹配到任何缓存时，是否允许退回按歌名与歌手匹配
    ///
    /// 只在歌名、歌手与 hash 来自同一次读取时开启：酷狗会把同一首歌其他音质或版本的歌词
    /// 以另一个 hash 命名缓存，单靠 hash 会漏掉它们
    pub metadata_fallback: bool,
}

/// 一轮采样：adapter 看到的所有 session 加上共享的 server clock
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playback {
    /// 采样时的 server clock，单位为毫秒，自 capture 起点算起
    pub server_ms: f64,

    /// 按 adapter 注册顺序排列的 session
    pub sessions: Vec<Session>,

    /// 本轮 adapter 失败信息，用 `; ` 连接
    pub error: Option<String>,
}

/// [`Line`] 内部一个音节大小的区间，用于卡拉OK高亮
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    /// 该区间覆盖的文本
    pub text: String,

    /// 相对曲目起点的起始偏移，单位为毫秒
    pub start_ms: i32,

    /// 相对曲目起点的结束偏移，单位为毫秒
    pub end_ms: i32,
}

/// 一行歌词，可选地拆成带时间戳的 token
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    /// 相对曲目起点的起始偏移，单位为毫秒
    pub start_ms: i32,

    /// 结束偏移，单位为毫秒；该行没有显式结束时间时为 `None`
    pub end_ms: Option<i32>,

    /// 平台存储的该行文本
    pub text: String,

    /// 平台只提供行级时间轴时为空
    pub tokens: Vec<Token>,
}

/// 同一首歌的一份歌词演绎
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricTrack {
    /// 演绎名称：`original`、`translation` 或 `romanization`
    pub kind: String,

    /// 该演绎的行；所有演绎与原曲共享同一时间轴
    pub lines: Vec<Line>,
}

/// 解析后的歌词文件：元数据加上其中包含的所有演绎
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    /// 用作客户端比较两个 document 是否同一份的内容摘要
    pub id: String,

    /// 该 document 解析自的 cache 文件
    pub filename: String,

    /// 从文件头读出的元数据
    pub metadata: Metadata,

    /// 原曲在前，随后是各翻译与音译演绎
    pub tracks: Vec<LyricTrack>,

    /// 非致命解析问题，例如无法读取的翻译轨
    pub warnings: Vec<String>,
}

impl Document {
    /// cache 文档会长期驻留，解析完成后归还文本与逐词数组的预留容量
    pub(crate) fn compact(&mut self) {
        self.id.shrink_to_fit();
        self.filename.shrink_to_fit();
        self.metadata.title.shrink_to_fit();
        self.metadata.artist.shrink_to_fit();
        self.metadata.album.shrink_to_fit();

        for track in &mut self.tracks {
            track.kind.shrink_to_fit();

            for line in &mut track.lines {
                line.text.shrink_to_fit();

                for token in &mut line.tokens {
                    token.text.shrink_to_fit();
                }

                line.tokens.shrink_to_fit();
            }

            track.lines.shrink_to_fit();
        }

        for warning in &mut self.warnings {
            warning.shrink_to_fit();
        }

        self.tracks.shrink_to_fit();
        self.warnings.shrink_to_fit();
    }
}

/// 歌词文件内存储的曲目元数据
#[derive(Clone, Debug, Default, Serialize)]
pub struct Metadata {
    /// 文件中记录的标题
    pub title: String,

    /// 文件中记录的艺人
    pub artist: String,

    /// 文件中记录的专辑
    pub album: String,
}

/// 平台 cache 中的一个歌词文件，供 overlay 的选择器列出
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// 客户端发回 `select` 消息时携带的 cache key
    pub key: String,

    /// cache 目录内的文件名
    pub filename: String,

    /// 从文件名解析出的标题
    pub title: String,

    /// 从文件名解析出的艺人
    pub artist: String,

    /// 从文件名解析出的专辑
    pub album: String,

    /// 歌词行数，在选择器中显示
    pub line_count: usize,

    /// 解析后的歌词，仅在 snapshot 的 `document` 字段被包含时发送
    #[serde(skip)]
    pub document: std::sync::Arc<Document>,
}

/// 为一个平台发现的全部条目，加上目录级问题
#[derive(Clone, Debug, Default, Serialize)]
pub struct Catalog {
    /// 按 adapter 顺序排列的条目
    pub entries: Vec<Entry>,

    /// 阻止文件被列出的错误
    pub issues: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compacts_retained_lyrics_without_changing_document_identity_or_wire_content() {
        let mut text = String::with_capacity(1024);
        text.push_str("歌词");
        let mut tokens = Vec::with_capacity(128);
        tokens.push(Token {
            text: text.clone(),
            start_ms: 0,
            end_ms: 1000,
        });
        let mut lines = Vec::with_capacity(128);
        lines.push(Line {
            text,
            start_ms: 0,
            end_ms: Some(1000),
            tokens,
        });
        let mut document = Document {
            id: "identity".into(),
            filename: "song.qrc".into(),
            metadata: Metadata::default(),
            tracks: vec![LyricTrack {
                kind: "original".into(),
                lines,
            }],
            warnings: vec!["warning".into()],
        };
        let before = serde_json::to_vec(&document).unwrap();

        document.compact();

        assert_eq!(serde_json::to_vec(&document).unwrap(), before);
        assert_eq!(document.tracks[0].lines.capacity(), 1);
        assert_eq!(document.tracks[0].lines[0].tokens.capacity(), 1);
        assert_eq!(document.tracks[0].lines[0].text.capacity(), "歌词".len());
    }
}
