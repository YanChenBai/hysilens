pub mod catalog;
pub mod lyrics;
#[cfg(windows)]
mod memory;

use super::{Adapter, Descriptor, LyricsMode, Registration, Sample};
use crate::model::{Playback, Source};
use std::{
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const DESCRIPTOR: Descriptor = Descriptor {
    source: Source::Kugou,
    session_id: "kugou",
    lyrics: LyricsMode::Cache,
    resolve_cache: Some(catalog::resolve),
};

pub fn registration(cache: PathBuf) -> Registration {
    Registration {
        descriptor: DESCRIPTOR,
        create: Box::new(move |context| {
            let probe = Probe::new(context.origin);
            super::qq_music::cache::spawn_for(cache, context, Source::Kugou, "krc", catalog::read);
            Box::new(probe)
        }),
    }
}

pub struct Probe {
    origin: Instant,
    media: anyhow::Result<super::qq_music::Probe>,
    #[cfg(windows)]
    memory: Option<memory::Reader>,
    previous: Option<(String, String, f64, Instant)>,
    last_attach: Option<Instant>,
    #[cfg(windows)]
    attach_error: Option<String>,
}

impl Probe {
    pub fn new(origin: Instant) -> Self {
        Self {
            origin,
            media: super::qq_music::Probe::new(origin),
            #[cfg(windows)]
            memory: None,
            previous: None,
            last_attach: None,
            #[cfg(windows)]
            attach_error: None,
        }
    }

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
        let timeline: anyhow::Result<(f64, f64)> =
            Err(anyhow::anyhow!("酷狗内存读取仅支持 Windows"));
        match timeline {
            Ok((position, duration)) => {
                let now = Instant::now();
                let normal =
                    self.previous
                        .as_ref()
                        .is_some_and(|(title, artist, old_position, sampled)| {
                            let elapsed = now.duration_since(*sampled).as_secs_f64() * 1000.0;
                            title == &session.title
                                && artist == &session.artist
                                && elapsed >= 50.0
                                && (0.8..=1.2).contains(&((position - old_position) / elapsed))
                        });
                // 尚未加载歌曲时可能读到 0/0；保留连接，等待下一次采样。
                session.position_ms = (duration > 0.0).then_some(position.min(duration));
                session.source_position_ms = session.position_ms;
                session.duration_ms = (duration > 0.0).then_some(duration);
                session.source_updated_unix_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|time| time.as_secs_f64() * 1000.0);
                session.rate =
                    (normal && duration > 0.0 && session.status == "playing").then_some(1.0);
                session.rate_basis = if session.rate.is_some() {
                    "inferredNormal"
                } else {
                    "unavailable"
                }
                .into();
                self.previous =
                    Some((session.title.clone(), session.artist.clone(), position, now));
            }
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
                session.error = Some(format!("{error:#}"));
            }
        }
        playback.server_ms = self.origin.elapsed().as_secs_f64() * 1000.0;
        playback
    }
}

impl Adapter for Probe {
    fn sample(&mut self) -> Sample {
        Sample {
            playback: self.sample(),
            document: None,
        }
    }
}
