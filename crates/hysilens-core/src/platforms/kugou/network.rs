//! 增强模式的网络兜底：按当前播放资源查询酷狗官方歌词服务
//!
//! 与酷狗客户端相同，用播放 hash、曲库歌曲 ID 和时长查询推荐歌词 ID，再用返回的
//! `accesskey` 下载 KRC；两个接口都不需要登录；只发送这三项歌曲信息，不发送账号或设备信息
//!
//! 推荐歌词 ID 用于精确定位本地缓存（酷狗常以其他版本的 hash 命名缓存文件）；下载的歌词
//! 只在本地缓存全部匹配失败时使用。查询失败后有限退避重试，结果在内存中保留最近若干首

use super::lyrics;
use crate::model::Document;
use anyhow::{Context, ensure};
use base64::Engine;
use serde::{Deserialize, de::DeserializeOwned};
use std::{
    collections::VecDeque,
    io::Read,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const SEARCH: &str = "http://krcs.kugou.com/search";

const DOWNLOAD: &str = "http://lyrics.kugou.com/download";

const TIMEOUT: Duration = Duration::from_secs(5);

const REMEMBERED: usize = 32;

const MAX_ATTEMPTS: u8 = 3;

const SEARCH_LIMIT: u64 = 512 * 1024;

// 4 MiB KRC 的 Base64 编码加上 JSON 包装
const DOWNLOAD_LIMIT: u64 = 6 * 1024 * 1024;

/// 一次查询的输入；三项都相同视为同一首歌
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    /// 当前播放资源的 hash
    pub hash: String,

    /// 酷狗曲库歌曲 ID；接口参数名为 `album_audio_id`
    pub mix_song_id: String,

    /// 时长，单位毫秒；未知时为 0
    pub duration_ms: u64,

    /// "歌手 - 歌名"，只用于命名下载的文档
    pub filename: String,
}

/// 某首歌的查询结果；在后台逐步填充
#[derive(Clone, Debug, Default)]
pub struct Found {
    /// 酷狗推荐的歌词 ID，对应本地 KRC 文件名中的 ID
    pub lyric_id: Option<String>,

    /// 下载并解码的推荐歌词
    pub document: Option<Arc<Document>>,

    /// 查询或下载失败的原因
    pub error: Option<String>,
}

#[derive(Default)]
struct Cached {
    found: Found,

    attempts: u8,

    retry_at: Option<Instant>,
}

impl Cached {
    fn begin(&mut self, now: Instant) -> bool {
        let first = self.attempts == 0;
        let retry = self.found.error.is_some()
            && self.attempts < MAX_ATTEMPTS
            && self.retry_at.is_some_and(|at| now >= at);

        if !first && !retry {
            return false;
        }

        self.attempts += 1;
        self.found.error = None;
        self.retry_at = None;

        true
    }

    fn fail(&mut self, message: String, now: Instant) {
        self.found.error = Some(message);
        self.retry_at = Some(now + Duration::from_secs(2u64.pow(u32::from(self.attempts))));
    }
}

#[derive(Default)]
struct State {
    /// 最近查询过的歌曲，最新的在末尾
    tracks: VecDeque<(Track, Cached)>,
}

impl State {
    fn get_mut(&mut self, track: &Track) -> Option<&mut Cached> {
        self.tracks
            .iter_mut()
            .find(|(known, _)| known == track)
            .map(|(_, found)| found)
    }
}

/// 后台歌词查询，按歌曲去重
#[derive(Clone, Default)]
pub struct Lookup {
    state: Arc<Mutex<State>>,
}

impl std::fmt::Debug for Lookup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Lookup").finish_non_exhaustive()
    }
}

impl Lookup {
    /// 返回该歌曲目前已得到的结果；首次遇到时在后台发起查询
    pub fn poll(&self, track: &Track) -> Found {
        let Ok(mut state) = self.state.lock() else {
            return Found::default();
        };

        if state.get_mut(track).is_none() {
            if state.tracks.len() >= REMEMBERED {
                state.tracks.pop_front();
            }

            state.tracks.push_back((track.clone(), Cached::default()));
        }

        let Some(cached) = state.get_mut(track) else {
            return Found::default();
        };

        if !cached.begin(Instant::now()) {
            return cached.found.clone();
        }

        let found = cached.found.clone();
        drop(state);

        let shared = self.state.clone();
        let track = track.clone();

        std::thread::spawn(move || {
            let update = |change: &dyn Fn(&mut Cached)| {
                if let Ok(mut state) = shared.lock()
                    && let Some(found) = state.get_mut(&track)
                {
                    change(found);
                }
            };

            let result = fetch(&track, |lyric_id| {
                update(&|cached| cached.found.lyric_id = Some(lyric_id.to_owned()))
            });

            match result {
                Ok(document) => update(&|cached| cached.found.document = Some(document.clone())),
                Err(error) => {
                    let message = format!("{error:#}");
                    update(&|cached| cached.fail(message.clone(), Instant::now()));
                }
            }
        });

        found
    }
}

fn read_json<T: DeserializeOwned>(reader: impl Read, limit: u64) -> anyhow::Result<T> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "酷狗歌词服务响应超过大小限制");

    Ok(serde_json::from_slice(&bytes)?)
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    proposal: String,

    #[serde(default)]
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
    #[serde(deserialize_with = "text")]
    id: String,

    accesskey: String,
}

#[derive(Deserialize)]
struct DownloadResponse {
    #[serde(default)]
    status: i64,

    #[serde(default)]
    content: String,
}

/// 接口里的数字 ID 有时以字符串、有时以数字返回
fn text<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(value) => value,
        other => other.to_string(),
    })
}

/// 从搜索结果中选出推荐的候选；没有推荐时取第一个
fn choose(response: SearchResponse) -> Option<Candidate> {
    let mut candidates = response.candidates;
    let index = candidates
        .iter()
        .position(|candidate| candidate.id == response.proposal)
        .unwrap_or(0);

    (index < candidates.len()).then(|| candidates.swap_remove(index))
}

fn decode_download(response: DownloadResponse, filename: String) -> anyhow::Result<Document> {
    ensure!(
        response.status == 200 && !response.content.is_empty(),
        "酷狗歌词下载失败"
    );

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(response.content.trim())
        .context("酷狗歌词内容编码无效")?;

    lyrics::decode(&bytes, filename)
}

/// 查询推荐歌词并下载；拿到推荐 ID 后立即通过 `on_lyric_id` 发布
fn fetch(track: &Track, on_lyric_id: impl Fn(&str)) -> anyhow::Result<Arc<Document>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .build()?;
    let duration = track.duration_ms.to_string();

    let response = client
        .get(SEARCH)
        .query(&[
            ("ver", "1"),
            ("man", "yes"),
            ("client", "mobi"),
            ("keyword", ""),
            ("duration", duration.as_str()),
            ("hash", track.hash.as_str()),
            ("album_audio_id", track.mix_song_id.as_str()),
        ])
        .send()
        .context("无法连接酷狗歌词服务")?
        .error_for_status()?;
    let search: SearchResponse =
        read_json(response, SEARCH_LIMIT).context("酷狗歌词搜索结果无效")?;

    let candidate = choose(search).context("酷狗歌词服务没有这首歌的歌词")?;
    on_lyric_id(&candidate.id);

    let response = client
        .get(DOWNLOAD)
        .query(&[
            ("ver", "1"),
            ("client", "pc"),
            ("id", candidate.id.as_str()),
            ("accesskey", candidate.accesskey.as_str()),
            ("fmt", "krc"),
            ("charset", "utf8"),
        ])
        .send()
        .context("无法下载酷狗歌词")?
        .error_for_status()?;
    let download: DownloadResponse =
        read_json(response, DOWNLOAD_LIMIT).context("酷狗歌词下载结果无效")?;

    let filename = format!(
        "{}-{}-{}-network.krc",
        track.filename, track.hash, candidate.id
    );
    let mut document = decode_download(download, filename)?;
    document.compact();

    Ok(Arc::new(document))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_failed_requests_with_backoff_and_a_limit() {
        let mut cached = Cached::default();
        let mut now = Instant::now();

        for attempt in 1..=MAX_ATTEMPTS {
            assert!(cached.begin(now));
            assert!(!cached.begin(now));
            cached.fail("timeout".into(), now);
            assert!(!cached.begin(now));
            now += Duration::from_secs(2u64.pow(u32::from(attempt)));
        }

        assert!(!cached.begin(now));
        assert_eq!(cached.attempts, MAX_ATTEMPTS);
    }

    #[test]
    fn keeps_completed_results_without_retrying() {
        let mut cached = Cached::default();
        let now = Instant::now();
        assert!(cached.begin(now));
        cached.found.lyric_id = Some("42".into());

        assert!(!cached.begin(now + Duration::from_secs(60)));
        assert_eq!(cached.found.lyric_id.as_deref(), Some("42"));
    }

    #[test]
    fn bounds_json_before_deserializing_and_stops_reading_at_the_limit() {
        let body = br#"{"candidates":[]}"#;
        let parsed: SearchResponse = read_json(&body[..], body.len() as u64).unwrap();
        assert!(parsed.candidates.is_empty());

        let mut oversized = std::io::Cursor::new(vec![b' '; 100]);
        assert!(read_json::<SearchResponse>(&mut oversized, 16).is_err());
        assert_eq!(oversized.position(), 17);
    }

    #[test]
    fn chooses_proposal_and_accepts_numeric_ids() {
        let response: SearchResponse = serde_json::from_str(
            r#"{"proposal":"713023760","candidates":[
              {"id":713023123,"accesskey":"B"},
              {"id":"713023760","accesskey":"A"}
            ]}"#,
        )
        .unwrap();
        let chosen = choose(response).unwrap();
        assert_eq!(
            (chosen.id.as_str(), chosen.accesskey.as_str()),
            ("713023760", "A")
        );

        let response: SearchResponse =
            serde_json::from_str(r#"{"proposal":"","candidates":[{"id":"1","accesskey":"K"}]}"#)
                .unwrap();
        assert_eq!(choose(response).unwrap().id, "1");

        let response: SearchResponse = serde_json::from_str(r#"{"candidates":[]}"#).unwrap();
        assert!(choose(response).is_none());
    }

    #[test]
    fn rejects_failed_or_invalid_downloads() {
        let failed = DownloadResponse {
            status: 404,
            content: String::new(),
        };
        assert!(decode_download(failed, "x".into()).is_err());

        let invalid = DownloadResponse {
            status: 200,
            content: "not base64!".into(),
        };
        assert!(decode_download(invalid, "x".into()).is_err());

        let not_krc = DownloadResponse {
            status: 200,
            content: base64::engine::general_purpose::STANDARD.encode(b"plain"),
        };
        assert!(decode_download(not_krc, "x".into()).is_err());
    }

    #[test]
    #[ignore = "访问酷狗歌词服务"]
    fn fetches_recommended_lyrics_from_kugou() {
        // 2026-11-12 实测：推荐 ID 与本地缓存文件名中的 ID 一致
        let track = Track {
            hash: "8ac66fdc5445e5bd8c76e979af022796".into(),
            mix_song_id: "933392747".into(),
            duration_ms: 264000,
            filename: "谢尔西 - 你怎么舍得我难过 (烟嗓版)".into(),
        };
        let ids = Mutex::new(Vec::new());
        let document = fetch(&track, |id| ids.lock().unwrap().push(id.to_owned())).unwrap();

        assert_eq!(ids.into_inner().unwrap(), ["713023760"]);
        assert!(!document.tracks[0].lines.is_empty());
        println!(
            "{} lines, first: {:?}",
            document.tracks[0].lines.len(),
            document.tracks[0].lines.first().map(|line| &line.text)
        );
    }
}
