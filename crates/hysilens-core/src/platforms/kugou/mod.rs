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
                self.memory = memory::Reader::attach().ok();
            }
            self.memory.as_ref().map_or_else(
                || {
                    Err(anyhow::anyhow!(
                        "无法读取酷狗播放进度：权限不足或客户端版本不支持"
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
                session.position_ms = Some(position.min(duration));
                session.source_position_ms = session.position_ms;
                session.duration_ms = Some(duration);
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
                session.error = Some(error.to_string());
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
