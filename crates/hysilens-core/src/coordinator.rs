use crate::protocol::{Snapshot, VERSION};
use crate::{
    model::Catalog,
    model::{Document, Playback, Session, Source},
    platforms::{AdapterState, Descriptor, LyricsMode},
};
use std::{collections::HashMap, sync::Arc};

pub struct Coordinator {
    pub state: Snapshot,
    catalogs: HashMap<Source, Catalog>,
    platforms: Vec<Descriptor>,
    manual_key: Option<String>,
    embedded_documents: HashMap<Source, Option<Arc<Document>>>,
}

impl Coordinator {
    pub fn new(epoch: String, platforms: Vec<Descriptor>) -> Self {
        Self {
            state: Snapshot {
                overlay: crate::settings::OverlaySettings::default(),
                source: Source::Auto,
                version: VERSION,
                epoch,
                seq: 0,
                generation: 0,
                playback: Playback::default(),
                session: None,
                lyrics_status: "waiting".into(),
                candidates: vec![],
                document: None,
                offset_ms: 0,
                show_romanization: true,
                show_translation: true,
                cache_issues: vec![],
            },
            catalogs: HashMap::new(),
            platforms,
            manual_key: None,
            embedded_documents: HashMap::new(),
        }
    }

    pub fn playback(&mut self, playback: Playback) {
        let session = choose_session(&playback, self.state.source, &self.platforms).cloned();

        let has_changed = identity(self.state.session.as_ref()) != identity(session.as_ref());

        let duration_changed = self
            .state
            .session
            .as_ref()
            .and_then(|session| session.duration_ms)
            != session.as_ref().and_then(|session| session.duration_ms);

        self.state.playback = playback;

        self.state.session = session;

        if has_changed {
            self.state.generation += 1;

            self.manual_key = None;

            self.resolve();
        } else if duration_changed && self.manual_key.is_none() {
            // 歌曲信息与时间轴可能分批到达，不等待目录变化才重新消歧。
            self.resolve();
        }
    }

    pub fn sample(&mut self, playback: Playback, adapters: Vec<AdapterState>) {
        self.embedded_documents = adapters
            .into_iter()
            .filter(|adapter| adapter.descriptor.lyrics == LyricsMode::Embedded)
            .map(|adapter| (adapter.descriptor.source, adapter.document))
            .collect();

        self.playback(playback);

        if self
            .active_platform()
            .is_some_and(|platform| platform.lyrics == LyricsMode::Embedded)
        {
            self.resolve();
        }
    }

    pub fn source(&mut self, source: Source) {
        self.state.source = source;

        self.manual_key = None;

        self.playback(self.state.playback.clone());

        self.resolve();
    }

    pub fn catalog(&mut self, source: Source, catalog: Catalog) {
        self.catalogs.insert(source, catalog);

        self.state.cache_issues = self.all_catalogs().issues;

        self.resolve();
    }

    pub fn all_catalogs(&self) -> Catalog {
        let mut result = Catalog::default();

        for platform in &self.platforms {
            if let Some(catalog) = self.catalogs.get(&platform.source) {
                result.entries.extend(catalog.entries.clone());

                result.issues.extend(catalog.issues.clone());
            }
        }

        result
    }

    fn active_platform(&self) -> Option<Descriptor> {
        let session = self.state.session.as_ref()?;

        self.platforms
            .iter()
            .find(|platform| platform.session_id.eq_ignore_ascii_case(&session.source_id))
            .copied()
    }

    pub fn select(&mut self, generation: u64, key: Option<String>) -> anyhow::Result<()> {
        anyhow::ensure!(
            generation == self.state.generation && self.state.session.is_some(),
            "Song changed; select again"
        );

        let platform = self
            .active_platform()
            .ok_or_else(|| anyhow::anyhow!("Unknown platform"))?;

        anyhow::ensure!(
            platform.lyrics == LyricsMode::Cache,
            "{} uses the active in-memory lyric model",
            platform.session_id
        );

        if let Some(key) = &key {
            anyhow::ensure!(
                self.catalogs
                    .get(&platform.source)
                    .is_some_and(|catalog| catalog.entries.iter().any(|entry| &entry.key == key)),
                "Unknown cache entry"
            );
        }

        self.manual_key = key;

        self.resolve();

        Ok(())
    }

    fn resolve(&mut self) {
        self.state.document = None;

        self.state.candidates.clear();

        self.state.lyrics_status = "waiting".into();

        let Some(session) = &self.state.session else {
            return;
        };

        let Some(platform) = self.active_platform() else {
            return;
        };

        if platform.lyrics == LyricsMode::Embedded {
            self.state.document = self
                .embedded_documents
                .get(&platform.source)
                .cloned()
                .flatten();

            self.state.lyrics_status = if session.error.is_some() {
                "captureError"
            } else if self.state.document.is_some() {
                "matchedMemory"
            } else {
                "loading"
            }
            .into();

            return;
        }

        let Some(catalog) = self.catalogs.get(&platform.source) else {
            self.state.lyrics_status = "missing".into();

            return;
        };

        let Some(resolve) = platform.resolve_cache else {
            self.state.lyrics_status = "missing".into();

            return;
        };

        let resolution = resolve(catalog, session, self.manual_key.as_deref());

        self.state.document = resolution.document;

        self.state.candidates = resolution.candidates;

        self.state.lyrics_status = resolution.status.into();
    }
}

fn choose_session<'a>(
    playback: &'a Playback,
    source: Source,
    platforms: &[Descriptor],
) -> Option<&'a Session> {
    platforms
        .iter()
        .filter(|platform| source == Source::Auto || platform.source == source)
        .find_map(|platform| {
            playback.sessions.iter().find(|session| {
                session.source_id.eq_ignore_ascii_case(platform.session_id)
                    && !session.title.is_empty()
                    && (source != Source::Auto
                        || !matches!(session.status.as_str(), "opened" | "closed" | "stopped"))
            })
        })
}

fn identity(session: Option<&Session>) -> Option<(&str, &str, &str, &str)> {
    session.map(|session| {
        (
            session.source_id.as_str(),
            session.title.as_str(),
            session.artist.as_str(),
            session.album.as_str(),
        )
    })
}

#[cfg(test)]
mod tests {

    use super::*;

    fn sample(coordinator: &mut Coordinator, playback: Playback, document: Option<Arc<Document>>) {
        coordinator.sample(
            playback,
            vec![AdapterState {
                descriptor: crate::platforms::wesing::DESCRIPTOR,
                document,
            }],
        );
    }

    #[test]
    fn auto_skips_idle_sessions_before_a_playing_kugou_session() {
        let playback = Playback {
            sessions: vec![
                Session {
                    source_id: "QQMusic.exe".into(),
                    title: "Old track".into(),
                    status: "opened".into(),
                    ..Default::default()
                },
                Session {
                    source_id: "kugou".into(),
                    title: "Current track".into(),
                    status: "playing".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let descriptors = crate::platforms::test_descriptors();
        assert_eq!(
            choose_session(&playback, Source::Auto, &descriptors)
                .unwrap()
                .source_id,
            "kugou"
        );
        assert_eq!(
            choose_session(&playback, Source::QqMusic, &descriptors)
                .unwrap()
                .source_id,
            "QQMusic.exe"
        );
    }

    #[test]
    fn follows_registered_order_and_session_ids_without_platform_branches() {
        let example = Descriptor {
            source: Source::Wesing,
            session_id: "Example.exe",
            resolve_cache: None,
            lyrics: LyricsMode::Embedded,
        };

        let qq = crate::platforms::qq_music::DESCRIPTOR;

        let playback = Playback {
            sessions: vec![
                Session {
                    source_id: "Example.exe".into(),
                    title: "Example".into(),
                    ..Session::default()
                },
                Session {
                    source_id: "QQMusic.exe".into(),
                    title: "QQ".into(),
                    ..Session::default()
                },
            ],
            ..Playback::default()
        };

        let document = Arc::new(Document {
            id: "example".into(),
            filename: "memory".into(),
            metadata: Default::default(),
            tracks: vec![],
            warnings: vec![],
        });

        let mut coordinator = Coordinator::new("test".into(), vec![qq, example]);

        coordinator.sample(
            playback.clone(),
            vec![AdapterState {
                descriptor: example,
                document: Some(document),
            }],
        );

        assert_eq!(coordinator.state.session.as_ref().unwrap().title, "QQ");

        coordinator.source(Source::Wesing);

        assert_eq!(coordinator.state.session.as_ref().unwrap().title, "Example");

        assert_eq!(coordinator.state.document.as_ref().unwrap().id, "example");

        coordinator.sample(
            playback,
            vec![AdapterState {
                descriptor: example,
                document: None,
            }],
        );

        assert!(coordinator.state.document.is_none());
    }

    #[test]
    fn prioritizes_wesing_and_switches_sources_without_leaking_documents() {
        let mut coordinator = Coordinator::new("test".into(), crate::platforms::test_descriptors());

        let playback = Playback {
            sessions: vec![
                Session {
                    source_id: "QQMusic.exe".into(),
                    title: "QQ".into(),
                    ..Session::default()
                },
                Session {
                    source_id: "WeSing.exe".into(),
                    title: "Karaoke".into(),
                    ..Session::default()
                },
            ],
            ..Playback::default()
        };

        let document = Arc::new(Document {
            id: "memory".into(),
            filename: "memory".into(),
            metadata: Default::default(),
            tracks: vec![],
            warnings: vec![],
        });

        sample(&mut coordinator, playback.clone(), Some(document.clone()));

        assert_eq!(
            coordinator.state.session.as_ref().unwrap().source_id,
            "WeSing.exe"
        );

        assert_eq!(coordinator.state.lyrics_status, "matchedMemory");

        let generation = coordinator.state.generation;

        coordinator.source(Source::QqMusic);

        assert_eq!(coordinator.state.session.as_ref().unwrap().title, "QQ");

        assert!(coordinator.state.document.is_none());

        assert!(coordinator.state.generation > generation);

        assert!(coordinator.select(generation, None).is_err());

        coordinator.source(Source::Wesing);

        assert_eq!(coordinator.state.document.as_ref().unwrap().id, "memory");

        assert!(
            coordinator
                .select(coordinator.state.generation, None)
                .is_err()
        );

        sample(&mut coordinator, playback.clone(), None);

        assert!(coordinator.state.document.is_none());

        sample(&mut coordinator, playback.clone(), Some(document));

        let qq_only = Playback {
            sessions: vec![playback.sessions[0].clone()],
            ..Playback::default()
        };

        sample(&mut coordinator, qq_only, None);

        assert!(coordinator.state.session.is_none());

        assert!(coordinator.state.document.is_none());

        coordinator.source(Source::Auto);

        assert_eq!(coordinator.state.session.as_ref().unwrap().title, "QQ");

        assert!(coordinator.state.document.is_none());
    }

    #[test]
    fn clears_memory_lyrics_on_failure_and_replaces_them_without_a_title_change() {
        let mut coordinator = Coordinator::new("test".into(), crate::platforms::test_descriptors());

        let mut playback = Playback {
            sessions: vec![Session {
                source_id: "WeSing.exe".into(),
                title: "A".into(),
                ..Session::default()
            }],
            ..Playback::default()
        };

        let document = |id: &str| {
            Arc::new(Document {
                id: id.into(),
                filename: "memory".into(),
                metadata: Default::default(),
                tracks: vec![],
                warnings: vec![],
            })
        };

        sample(&mut coordinator, playback.clone(), Some(document("first")));

        sample(&mut coordinator, playback.clone(), Some(document("second")));

        assert_eq!(coordinator.state.document.as_ref().unwrap().id, "second");

        playback.sessions[0].error = Some("Invalid model".into());

        sample(&mut coordinator, playback, None);

        assert!(coordinator.state.document.is_none());

        assert_eq!(coordinator.state.lyrics_status, "captureError");
    }

    #[test]
    fn rejects_selection_for_an_old_song_and_clears_document_on_switch() {
        let mut coordinator = Coordinator::new("test".into(), crate::platforms::test_descriptors());

        let playback = |title: &str| Playback {
            sessions: vec![Session {
                source_id: "QQMusic.exe".into(),
                title: title.into(),
                ..Session::default()
            }],
            ..Playback::default()
        };

        coordinator.playback(playback("A"));

        let old_generation = coordinator.state.generation;

        coordinator.playback(playback("B"));

        assert!(coordinator.select(old_generation, None).is_err());

        assert!(coordinator.state.document.is_none());

        assert_eq!(coordinator.state.lyrics_status, "missing");
    }

    #[test]
    fn retries_ambiguous_match_when_timeline_duration_arrives() {
        use crate::{model::Entry, model::Metadata};

        let mut coordinator = Coordinator::new("test".into(), crate::platforms::test_descriptors());

        let entries = [0, 232]
            .into_iter()
            .map(|seconds| {
                let filename = format!("Singer - A - {seconds} - Studio_qm.qrc");

                Entry {
                    key: seconds.to_string(),
                    filename: filename.clone(),
                    title: "A".into(),
                    artist: "Singer".into(),
                    album: "Studio".into(),
                    line_count: 0,
                    document: Arc::new(Document {
                        id: seconds.to_string(),
                        filename,
                        metadata: Metadata::default(),
                        tracks: vec![],
                        warnings: vec![],
                    }),
                }
            })
            .collect();

        coordinator.catalog(
            Source::QqMusic,
            Catalog {
                entries,
                issues: vec![],
            },
        );

        let sample = |duration_ms| Playback {
            sessions: vec![Session {
                source_id: "QQMusic.exe".into(),
                title: "A".into(),
                artist: "Singer".into(),
                album: "Studio".into(),
                duration_ms,
                ..Session::default()
            }],
            ..Playback::default()
        };

        coordinator.playback(sample(None));

        assert_eq!(coordinator.state.lyrics_status, "ambiguous");

        let generation = coordinator.state.generation;

        coordinator.playback(sample(Some(232_012.0)));

        assert_eq!(
            coordinator.state.document.as_ref().unwrap().filename,
            "Singer - A - 232 - Studio_qm.qrc"
        );

        assert_eq!(coordinator.state.generation, generation);
    }
}
