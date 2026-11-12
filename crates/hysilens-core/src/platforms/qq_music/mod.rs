//! QQ 音乐 adapter
//!
//! 播放状态来自 SMTC，歌词来自 [`catalog`] 中 resolver 匹配到的 `_qm.qrc` cache，
//! 不进行任何内存访问

pub(crate) mod cache;
pub mod catalog;
mod diagnostics;
pub mod index;
pub mod lyrics;
mod playback;

use super::{Adapter, Descriptor, LyricsMode, Registration, Sample};
use crate::model::{Playback, Source};
use std::{path::PathBuf, time::Instant};

/// QQ Music 的静态 descriptor，歌词与本地 QRC cache 匹配
pub const DESCRIPTOR: Descriptor = Descriptor {
    source: Source::QqMusic,
    session_id: "QQMusic.exe",
    lyrics: LyricsMode::Cache,
    resolve_cache: Some(catalog::resolve),
};

/// 创建 QQ Music adapter 工厂，并为 `cache` 启动后台 QRC cache scanner
pub fn registration(cache: PathBuf) -> Registration {
    let diagnostic_cache = cache.clone();

    Registration {
        descriptor: DESCRIPTOR,
        diagnostics: super::diagnostics::Diagnostics {
            name: "QQ 音乐",
            description: "采集 QQ 音乐的进程与版本、媒体会话、原始与推算时间轴、更新时间、播放速率来源，以及歌词缓存目录、QRC 解析错误、匹配候选和元数据差异",
            collect: std::sync::Arc::new(move |snapshot| {
                diagnostics::report(&diagnostic_cache, snapshot)
            }),
        },
        create: Box::new(move |context| {
            let adapter = QqMusic {
                origin: context.origin,
                probe: playback::Probe::new(context.origin),
            };

            cache::spawn(cache, context);

            Box::new(adapter)
        }),
    }
}

pub use playback::Probe;

struct QqMusic {
    origin: Instant,

    probe: anyhow::Result<playback::Probe>,
}

impl Adapter for QqMusic {
    fn sample(&mut self) -> Sample {
        let playback = match &self.probe {
            Ok(probe) => {
                let mut playback = probe.sample();
                playback.sessions.retain(|session| {
                    session
                        .source_id
                        .eq_ignore_ascii_case(DESCRIPTOR.session_id)
                });
                playback
            }
            Err(error) => Playback {
                server_ms: self.origin.elapsed().as_secs_f64() * 1000.0,
                error: Some(error.to_string()),
                ..Playback::default()
            },
        };

        Sample {
            playback,
            document: None,
        }
    }
}
