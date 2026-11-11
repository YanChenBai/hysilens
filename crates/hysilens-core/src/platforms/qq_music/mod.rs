pub(crate) mod cache;
pub mod catalog;
pub mod index;
pub mod lyrics;
mod playback;

use super::{Adapter, Descriptor, LyricsMode, Registration, Sample};
use crate::model::{Playback, Source};
use std::{path::PathBuf, time::Instant};

pub const DESCRIPTOR: Descriptor = Descriptor {
    source: Source::QqMusic,
    session_id: "QQMusic.exe",
    lyrics: LyricsMode::Cache,
    resolve_cache: Some(catalog::resolve),
};

pub fn registration(cache: PathBuf) -> Registration {
    Registration {
        descriptor: DESCRIPTOR,
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
