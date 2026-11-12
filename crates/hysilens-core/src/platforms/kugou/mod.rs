//! 酷狗 adapter
//!
//! session 来自系统媒体传输控制（SMTC）；只读内存读取器附着在 `KuGou.exe` 或
//! `KuGouMusic.exe` 上，用毫秒级时间轴与用于匹配缓存 KRC 歌词的队列标识细化 session
//!
//! 采样失败后会放弃内存访问，并且最多每两秒重试一次；期间 SMTC 状态继续发布，缓存歌词由
//! [`catalog`] 解析，不接触播放器进程
//!
//! 开启增强模式时优先通过 [`cdp`] 读取酷狗内嵌网页提供的播放信息，失败时回退到内存读取

pub mod catalog;
pub mod cdp;
#[cfg(windows)]
mod identity;
#[cfg(windows)]
pub mod launcher;
pub mod lyrics;
#[cfg(windows)]
mod memory;

#[cfg(windows)]
pub use memory::diagnostic_report;

use super::{Adapter, Descriptor, LyricsMode, Registration, Sample};
use crate::model::{Playback, Session, Source};
use std::{
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

/// 从当前 KuGou 队列条目读取的只读标识，不依赖 SMTC 文本
#[cfg(windows)]
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct KugouIdentity {
    pub queue_item_id: i32,

    pub source_type: i32,

    pub list_id: i32,

    pub source_song_id: i32,

    pub music_file_hash: String,

    pub last_played_music_hash: String,

    pub mix_song_id: u64,

    pub recommend_krc_id: u32,
}

#[cfg(windows)]
impl KugouIdentity {
    pub(super) fn track_id(&self) -> String {
        format!(
            "kugou:{}:{}:{}:{}:{}:{}:{}",
            self.queue_item_id,
            self.source_type,
            self.list_id,
            self.source_song_id,
            self.mix_song_id,
            self.music_file_hash,
            self.last_played_music_hash
        )
    }

    pub(super) fn lyric_match(&self) -> crate::model::LyricMatch {
        crate::model::LyricMatch {
            file_hashes: vec![
                self.music_file_hash.clone(),
                self.last_played_music_hash.clone(),
            ],
            preferred_lyric_id: (self.recommend_krc_id != 0)
                .then(|| self.recommend_krc_id.to_string()),
        }
    }
}

/// KuGou 的静态 descriptor，歌词与本地 KRC cache 匹配
pub const DESCRIPTOR: Descriptor = Descriptor {
    source: Source::Kugou,
    session_id: "kugou",
    lyrics: LyricsMode::Cache,
    resolve_cache: Some(catalog::resolve),
};

/// 创建 KuGou adapter 工厂，并为 `cache` 启动后台 KRC cache 扫描器
///
/// `enhanced` 为真时优先通过酷狗调试端口读取播放信息
pub fn registration(cache: PathBuf, enhanced: bool) -> Registration {
    let diagnostic_cache = cache.clone();

    Registration {
        descriptor: DESCRIPTOR,
        diagnostics: super::diagnostics::Diagnostics {
            name: "酷狗音乐",
            description: "采集酷狗进程与模块版本、文件哈希、歌曲 Hash / ID 读取结果、KRC 候选命中与拒绝原因、元数据差异、解析错误、时间轴采样和少量只读内存片段",
            collect: std::sync::Arc::new(move |snapshot| {
                #[cfg(windows)]
                let mut report = diagnostic_report();

                #[cfg(not(windows))]
                let mut report = serde_json::json!({ "error": "酷狗内存诊断仅支持 Windows" });

                report["cache"] = super::diagnostics::cache(&diagnostic_cache, ".krc");
                let catalog = catalog::read(&diagnostic_cache);

                report["memoryIdentity"] = catalog::diagnostic_identity(
                    super::diagnostics::session(snapshot, DESCRIPTOR),
                    catalog.as_ref().ok(),
                );
                report["matching"] = super::diagnostics::matching(
                    snapshot,
                    DESCRIPTOR,
                    catalog,
                    catalog::diagnostic_evidence,
                );
                report["identityProbeSummary"] = identity_probe_summary(&report);

                report
            }),
        },
        create: Box::new(move |context| {
            let probe = Probe::new(context.origin, enhanced);
            super::qq_music::cache::spawn_for(cache, context, Source::Kugou, "krc", catalog::read);
            Box::new(probe)
        }),
    }
}

/// KuGou adapter 状态
///
/// session 来自 SMTC probe，可选的内存读取器补充毫秒级进度、时长和歌词匹配所用的队列标识；
/// 失败后按需重新附着
pub struct Probe {
    origin: Instant,

    media: anyhow::Result<super::qq_music::Probe>,

    /// 开启增强模式时存在
    cdp: Option<cdp::Channel>,

    #[cfg(windows)]
    memory: Option<memory::Reader>,

    previous: Option<(String, String, Option<String>, f64, Instant)>,

    last_attach: Option<Instant>,

    #[cfg(windows)]
    attach_error: Option<String>,
}

impl std::fmt::Debug for Probe {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = formatter.debug_struct("Probe");

        debug.field("origin", &self.origin);
        debug.field("media_ready", &self.media.is_ok());
        debug.field("cdp", &self.cdp);

        #[cfg(windows)]
        {
            debug.field("memory_attached", &self.memory.is_some());
            debug.field("attach_error", &self.attach_error);
        }

        debug.finish_non_exhaustive()
    }
}

impl Probe {
    /// 创建 adapter，并将 `server_ms` 的原点设为 `origin`；`enhanced` 开启增强模式
    pub fn new(origin: Instant, enhanced: bool) -> Self {
        Self {
            origin,
            media: super::qq_music::Probe::new(origin),
            cdp: enhanced.then(|| cdp::Channel::new(cdp::PORT)),
            #[cfg(windows)]
            memory: None,
            previous: None,
            last_attach: None,
            #[cfg(windows)]
            attach_error: None,
        }
    }

    /// 轮询一次 SMTC，存在 KuGou session 时附着到播放器进程
    ///
    /// 失败后内存读取器最多每两秒重试一次，时间轴读取失败时，session 保留其 SMTC 状态并上报
    /// 内存错误
    pub fn sample(&mut self) -> Playback {
        let mut playback = match &self.media {
            Ok(probe) => probe.sample(),
            Err(error) => {
                return Playback {
                    server_ms: self.origin.elapsed().as_secs_f64() * 1000.0,
                    error: Some(error.to_string()),
                    ..Default::default()
                };
            }
        };
        playback
            .sessions
            .retain(|session| session.source_id.eq_ignore_ascii_case("kugou"));

        // 增强模式可用时优先使用它的进度与身份，并释放内存读取句柄；失败时回退到内存方案
        let enhanced = self.cdp.as_mut().map(cdp::Channel::sample);

        // 酷狗不是从带 AppUserModelID 的快捷方式启动时，系统媒体会话可能识别不到它；
        // 增强模式读到播放信息时直接据此建立会话
        if playback.sessions.is_empty()
            && let Some(Ok(info)) = &enhanced
        {
            playback.sessions.push(info.session());
        }

        let Some(session) = playback.sessions.first_mut() else {
            #[cfg(windows)]
            {
                self.memory = None;
                self.attach_error = None;
            }
            self.previous = None;
            self.last_attach = None;

            return playback;
        };

        if let Some(Ok(info)) = &enhanced {
            if let Some(status) = info.status() {
                session.status = status.into();
            }

            session.track_id = Some(info.track_id());
            session.lyric_match = Some(info.lyric_match());

            #[cfg(windows)]
            {
                self.memory = None;
                self.attach_error = None;
            }

            let (position, duration) = (info.position_ms, info.duration_ms);
            self.publish_timeline(session, position, duration);
            playback.server_ms = self.origin.elapsed().as_secs_f64() * 1000.0;

            return playback;
        }

        let enhanced_error = match enhanced {
            Some(Err(error)) => Some(format!("{error:#}")),
            _ => None,
        };

        #[cfg(windows)]
        let timeline = {
            if self.memory.is_none()
                && self
                    .last_attach
                    .is_none_or(|last| last.elapsed().as_secs() >= 2)
            {
                self.last_attach = Some(Instant::now());

                match memory::Reader::attach() {
                    Ok(reader) => {
                        self.memory = Some(reader);
                        self.attach_error = None;
                    }
                    Err(error) => self.attach_error = Some(format!("{error:#}")),
                }
            }
            self.memory.as_ref().map_or_else(
                || {
                    Err(anyhow::anyhow!(
                        "{}",
                        self.attach_error
                            .as_deref()
                            .unwrap_or("正在等待重新连接酷狗进程")
                    ))
                },
                memory::Reader::timeline,
            )
        };
        #[cfg(not(windows))]
        let timeline: anyhow::Result<(f64, Option<f64>)> =
            Err(anyhow::anyhow!("酷狗内存读取仅支持 Windows"));
        #[cfg(windows)]
        if let Some(reader) = &self.memory
            && let Ok(identity) = reader.identity()
        {
            // 身份失败由专用诊断报告记录，继续使用现有缓存匹配
            session.track_id = Some(identity.track_id());
            session.lyric_match = Some(identity.lyric_match());
        }

        match timeline {
            Ok((position, duration)) => self.publish_timeline(session, position, duration),
            Err(error) => {
                #[cfg(windows)]
                {
                    self.memory = None;
                }
                self.previous = None;
                session.position_ms = None;
                session.source_position_ms = None;
                session.rate = None;
                session.rate_basis = "unavailable".into();
                session.error = Some(match &enhanced_error {
                    Some(enhanced) => format!("增强模式：{enhanced}；内存读取：{error:#}"),
                    None => format!("{error:#}"),
                });
            }
        }
        playback.server_ms = self.origin.elapsed().as_secs_f64() * 1000.0;
        playback
    }

    /// 发布一次有效进度，并用相邻两次采样推断是否按正常速率播放
    fn publish_timeline(&mut self, session: &mut Session, position: f64, duration: Option<f64>) {
        let now = Instant::now();
        let normal = self.previous.as_ref().is_some_and(
            |(title, artist, track_id, old_position, sampled)| {
                let elapsed = now.duration_since(*sampled).as_secs_f64() * 1000.0;
                title == &session.title
                    && artist == &session.artist
                    && track_id == &session.track_id
                    && elapsed >= 50.0
                    && (0.8..=1.2).contains(&((position - old_position) / elapsed))
            },
        );
        apply_timeline(session, position, duration, normal);
        self.previous = Some((
            session.title.clone(),
            session.artist.clone(),
            session.track_id.clone(),
            position,
            now,
        ));
    }
}

fn apply_timeline(session: &mut Session, position: f64, duration: Option<f64>, normal: bool) {
    // 时长链可能读错堆对象字段，而独立的 kgplayer.dll 进度仍正常推进
    // 缺少时长不是播放错误：发布真实进度、清除旧歌时长，不拿错误时长截断进度；
    // 连续进度采样仍可确认正常播放速率，时长链恢复后下一帧直接补回时长
    session.position_ms = Some(position);
    session.source_position_ms = Some(position);
    session.duration_ms = duration;
    session.source_updated_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|time| time.as_secs_f64() * 1000.0);
    session.rate = (normal && session.status == "playing").then_some(1.0);
    session.rate_basis = if session.rate.is_some() {
        "inferredNormal"
    } else {
        "unavailable"
    }
    .into();
    session.error = None;
}

impl Adapter for Probe {
    fn sample(&mut self) -> Sample {
        Sample {
            playback: self.sample(),
            document: None,
        }
    }
}

fn identity_probe_summary(report: &serde_json::Value) -> serde_json::Value {
    let attempts: Vec<_> = report["processes"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|process| process["samples"].as_array().into_iter().flatten())
        .filter_map(|sample| sample.get("identity"))
        .collect();

    serde_json::json!({
        "attemptCount": attempts.len(),
        "successCount": attempts.iter().filter(|attempt| attempt.get("value").is_some()).count(),
        "failureCount": attempts.iter().filter(|attempt| attempt.get("error").is_some()).count(),
        "errors": attempts.iter().filter_map(|attempt| attempt.get("error")).take(16).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    #[ignore = "只读采样已经运行的酷狗并导出诊断，不会启动播放器"]
    fn inspect_running_kugou_diagnostics() {
        let origin = Instant::now();
        let mut probe = Probe::new(origin, false);
        let mut coordinator =
            crate::coordinator::Coordinator::new("live-diagnostics".into(), vec![DESCRIPTOR]);

        coordinator.state.source = Source::Kugou;

        for _ in 0..4 {
            coordinator.playback(probe.sample());
            std::thread::sleep(std::time::Duration::from_millis(250));
        }

        let settings = crate::config::Settings::load().unwrap();
        let registration = registration(settings.kugou_cache_dir, false);
        let report = (registration.diagnostics.collect)(&coordinator.state);
        let path = std::env::temp_dir().join("hysilens-kugou-live-diagnostics.json");

        std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        println!(
            "{}",
            serde_json::json!({
                "path": path,
                "memoryIdentity": report["memoryIdentity"],
                "identityProbeSummary": report["identityProbeSummary"],
                "automaticStatus": report["matching"]["automatic"]["status"],
                "candidateCount": report["matching"]["automatic"]["candidateCount"],
                "parseErrorCount": report["matching"]["parseErrorCount"],
            })
        );

        assert!(
            report["identityProbeSummary"]["successCount"]
                .as_u64()
                .is_some_and(|count| count > 0)
        );
        assert_eq!(report["memoryIdentity"]["validHashAvailable"], true);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "需要以增强模式运行的酷狗和本地 KRC 缓存"]
    fn matches_running_kugou_through_enhanced_mode() {
        let origin = Instant::now();
        let mut probe = Probe::new(origin, true);
        let mut coordinator =
            crate::coordinator::Coordinator::new("live-enhanced".into(), vec![DESCRIPTOR]);
        let settings = crate::config::Settings::load().unwrap();

        coordinator.source(Source::Kugou);
        coordinator.catalog(
            Source::Kugou,
            catalog::read(&settings.kugou_cache_dir).unwrap(),
        );

        for _ in 0..3 {
            let playback = probe.sample();
            println!(
                "sessions={:?} error={:?}",
                playback
                    .sessions
                    .iter()
                    .map(|session| (&session.source_id, &session.title, &session.track_id))
                    .collect::<Vec<_>>(),
                playback.error
            );
            coordinator.playback(playback);
            std::thread::sleep(std::time::Duration::from_millis(250));
        }

        let session = coordinator.state.session.clone().unwrap();
        let document = coordinator.state.document.clone();

        println!(
            "{}",
            serde_json::json!({
                "title": session.title,
                "status": session.status,
                "positionMs": session.position_ms,
                "durationMs": session.duration_ms,
                "rateBasis": session.rate_basis,
                "trackId": session.track_id,
                "error": session.error,
                "lyricsStatus": coordinator.state.lyrics_status,
                "document": document.as_ref().map(|document| &document.filename),
            })
        );

        assert!(session.track_id.unwrap().starts_with("kugou:cdp:"));
        assert!(session.position_ms.is_some());
        assert!(cdp::status().connected);
    }

    #[test]
    fn summarizes_identity_read_failures_separately_from_successes() {
        let report = identity_probe_summary(&serde_json::json!({
            "processes": [{ "samples": [
                { "identity": { "value": { "mixSongId": 42 } } },
                { "identity": { "error": "queue changed during read" } },
            ] }],
        }));

        assert_eq!(report["attemptCount"], 2);
        assert_eq!(report["successCount"], 1);
        assert_eq!(report["failureCount"], 1);
        assert_eq!(report["errors"][0], "queue changed during read");
    }

    #[test]
    fn publishes_position_and_playback_rate_without_duration() {
        let mut session = Session {
            status: "playing".into(),
            duration_ms: Some(563.0),
            error: Some("old duration error".into()),
            ..Default::default()
        };
        apply_timeline(&mut session, 123548.812, None, true);
        assert_eq!(session.position_ms, Some(123548.812));
        assert_eq!(session.source_position_ms, session.position_ms);
        assert_eq!(session.duration_ms, None);
        assert_eq!(session.rate, Some(1.0));
        assert_eq!(session.rate_basis, "inferredNormal");
        assert!(session.source_updated_unix_ms.is_some());
        assert!(session.error.is_none());

        session.status = "paused".into();
        apply_timeline(&mut session, 123548.812, None, true);
        assert_eq!(session.position_ms, Some(123548.812));
        assert_eq!(session.rate, None);

        session.status = "playing".into();
        apply_timeline(&mut session, 90000.0, Some(174000.0), false);
        assert_eq!(session.position_ms, Some(90000.0));
        assert_eq!(session.duration_ms, Some(174000.0));
        assert_eq!(session.rate, None);
    }
}
