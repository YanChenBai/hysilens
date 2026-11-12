//! 把 adapter 采样归约为 overlay UI 渲染所用的单个 snapshot

use crate::protocol::{Snapshot, VERSION};
use crate::{
    model::Catalog,
    model::{Document, Playback, Session, Source},
    platforms::{AdapterState, Descriptor, LyricsMode},
};
use std::{collections::HashMap, sync::Arc};

/// 把 adapter 采样归约为发布给客户端的 snapshot
///
/// 选择规则都在这里：平台顺序决定自动匹配，手动 [`Coordinator::select`] 会覆盖它，
/// 直到歌曲变化；`generation` 每次换歌恰好递增一次，从而能拒绝过期的选择
pub struct Coordinator {
    /// 当前 snapshot；所有者发布它的克隆并自行递增 `state.seq`
    pub state: Snapshot,

    catalogs: HashMap<Source, Catalog>,

    platforms: Vec<Descriptor>,

    manual_key: Option<String>,

    /// adapter 本轮直接给出的歌词：内嵌平台的主要来源，缓存平台本地匹配失败时的网络兜底
    adapter_documents: HashMap<Source, Option<Arc<Document>>>,
}

impl Coordinator {
    /// 为一个 server 进程创建 coordinator；`epoch` 标识该进程
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
            adapter_documents: HashMap::new(),
        }
    }

    /// 合并一次 playback 采样，并在播放曲目变化时重新解析
    ///
    /// 曲目变化会开启新的 generation 并清除手动选择；同一曲目只有上报了新的时长或
    /// 歌词身份时才会重新执行解析
    pub fn playback(&mut self, playback: Playback) {
        let session = choose_session(&playback, self.state.source, &self.platforms).cloned();
        let has_changed = identity(self.state.session.as_ref()) != identity(session.as_ref());
        let duration_changed = self
            .state
            .session
            .as_ref()
            .and_then(|session| session.duration_ms)
            != session.as_ref().and_then(|session| session.duration_ms);

        let lyric_identity_changed = self
            .state
            .session
            .as_ref()
            .and_then(|s| s.lyric_match.as_ref())
            != session.as_ref().and_then(|s| s.lyric_match.as_ref());

        self.state.playback = playback;

        self.state.session = session;

        if has_changed {
            self.state.generation += 1;

            self.manual_key = None;

            self.resolve();
        } else if (duration_changed || lyric_identity_changed) && self.manual_key.is_none() {
            // 曲目元数据与时间轴可能分几次采样到达，因此这里重试解析而不用等 catalog 变化
            self.resolve();
        }
    }

    /// 保存本轮 adapter 给出的歌词 document，然后处理 `playback`
    pub fn sample(&mut self, playback: Playback, adapters: Vec<AdapterState>) {
        let previous = self.active_adapter_document();

        self.adapter_documents = adapters
            .into_iter()
            .map(|adapter| (adapter.descriptor.source, adapter.document))
            .collect();

        self.playback(playback);

        let Some(platform) = self.active_platform() else {
            return;
        };

        if platform.lyrics == LyricsMode::Embedded {
            self.resolve();
        } else if self.manual_key.is_none() && self.active_adapter_document() != previous {
            // 网络兜底歌词在曲目开始后才到达，需要重新解析
            self.resolve();
        }
    }

    /// 当前平台 adapter 给出的歌词 document 的 id
    fn active_adapter_document(&self) -> Option<String> {
        let platform = self.active_platform()?;

        self.adapter_documents
            .get(&platform.source)?
            .as_ref()
            .map(|document| document.id.clone())
    }

    /// 切换跟随的平台并忘记任何手动选择
    pub fn source(&mut self, source: Source) {
        self.state.source = source;

        self.manual_key = None;

        self.playback(self.state.playback.clone());

        self.resolve();
    }

    /// 替换某个平台缓存的 catalog 并重新解析歌词
    pub fn catalog(&mut self, source: Source, catalog: Catalog) {
        self.catalogs.insert(source, catalog);

        self.state.cache_issues = self.all_catalogs().issues;

        self.resolve();
    }

    /// 按平台注册顺序把所有平台的 catalog 合并成一个
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

    /// 为当前歌曲锁定 cache 条目 `key`，或传 `None` 恢复自动匹配
    ///
    /// # Errors
    /// `generation` 已不是当前值、没有歌曲在播放、该平台不使用 cache catalog，
    /// 或 `key` 不在该 catalog 中时失败
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
                .adapter_documents
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

        let network = self
            .adapter_documents
            .get(&platform.source)
            .cloned()
            .flatten();

        let resolution = self
            .catalogs
            .get(&platform.source)
            .zip(platform.resolve_cache)
            .map(|(catalog, resolve)| resolve(catalog, session, self.manual_key.as_deref()));

        if let Some(resolution) = resolution {
            self.state.document = resolution.document;
            self.state.candidates = resolution.candidates;
            self.state.lyrics_status = resolution.status.into();
        } else {
            self.state.lyrics_status = "missing".into();
        }

        // 本地缓存没有可用匹配且用户未手动选择时，才使用 adapter 从网络取得的歌词
        if self.state.document.is_none()
            && self.manual_key.is_none()
            && let Some(document) = network
        {
            self.state.document = Some(document);
            self.state.lyrics_status = "matchedNetwork".into();
        }
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
                    && (!session.title.is_empty() || session.track_id.is_some())
                    && (source != Source::Auto
                        || !matches!(session.status.as_str(), "opened" | "closed" | "stopped"))
            })
        })
}

fn identity(session: Option<&Session>) -> Option<(&str, &str, &str, &str)> {
    session.map(|session| {
        // 原生身份能识别同名不同版本，并忽略 SMTC 文本的延迟更新
        if let Some(track_id) = session.track_id.as_deref() {
            return (session.source_id.as_str(), track_id, "", "");
        }
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

    #[test]
    fn uses_adapter_network_lyrics_only_when_local_cache_has_no_match() {
        use crate::model::Entry;

        let document = |id: &str| {
            Arc::new(Document {
                id: id.into(),
                filename: String::new(),
                metadata: Default::default(),
                tracks: vec![],
                warnings: vec![],
            })
        };
        let descriptor = crate::platforms::kugou::DESCRIPTOR;
        let mut coordinator = Coordinator::new("test".into(), vec![descriptor]);
        let playback = |title: &str| Playback {
            sessions: vec![Session {
                source_id: "kugou".into(),
                title: title.into(),
                artist: "Singer".into(),
                status: "playing".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let adapters = |network: Option<&str>| {
            vec![AdapterState {
                descriptor,
                document: network.map(document),
            }]
        };

        coordinator.source(Source::Kugou);
        coordinator.catalog(
            Source::Kugou,
            Catalog {
                entries: vec![Entry {
                    key: "local".into(),
                    filename: format!("Singer - Cached-{}-1-00000000.krc", "aa".repeat(16)),
                    title: "Cached".into(),
                    artist: "Singer".into(),
                    album: String::new(),
                    line_count: 0,
                    document: document("local"),
                }],
                issues: vec![],
            },
        );

        // 查询尚未完成时保持 missing，网络歌词到达后不换曲也会重新解析
        coordinator.sample(playback("Uncached"), adapters(None));
        assert_eq!(coordinator.state.lyrics_status, "missing");
        coordinator.sample(playback("Uncached"), adapters(Some("network")));
        assert_eq!(coordinator.state.lyrics_status, "matchedNetwork");
        assert_eq!(coordinator.state.document.as_ref().unwrap().id, "network");

        // 本地缓存可匹配时优先使用本地
        coordinator.sample(playback("Cached"), adapters(Some("network")));
        assert_eq!(coordinator.state.lyrics_status, "matchedMetadata");
        assert_eq!(coordinator.state.document.as_ref().unwrap().id, "local");
    }

    #[test]
    fn native_identity_resets_same_title_selection_and_refreshes_lyric_recommendations() {
        use crate::model::{Entry, LyricMatch};

        let mut coordinator =
            Coordinator::new("test".into(), vec![crate::platforms::kugou::DESCRIPTOR]);

        let entries = [1, 2]
            .into_iter()
            .map(|id| Entry {
                key: id.to_string(),
                filename: format!("Singer - Same-{}-{id}-00000000.krc", "aa".repeat(16)),
                title: "Same".into(),
                artist: "Singer".into(),
                album: String::new(),
                line_count: 0,
                document: Arc::new(Document {
                    id: id.to_string(),
                    filename: String::new(),
                    metadata: Default::default(),
                    tracks: vec![],
                    warnings: vec![],
                }),
            })
            .collect();

        coordinator.catalog(
            Source::Kugou,
            Catalog {
                entries,
                issues: vec![],
            },
        );

        let sample = |queue_item_id: i32, recommend_krc_id: u32, artist: &str| Playback {
            sessions: vec![Session {
                source_id: "kugou".into(),
                title: "Same".into(),
                artist: artist.into(),
                status: "playing".into(),
                track_id: Some(format!("track:{queue_item_id}")),
                lyric_match: Some(LyricMatch {
                    file_hashes: vec!["aa".repeat(16)],
                    preferred_lyric_id: Some(recommend_krc_id.to_string()),
                    preferred_document_id: None,
                    metadata_fallback: false,
                }),
                ..Default::default()
            }],
            ..Default::default()
        };

        coordinator.playback(sample(42, 1, "Singer"));

        let generation = coordinator.state.generation;

        assert_eq!(coordinator.state.document.as_ref().unwrap().id, "1");

        coordinator.playback(sample(42, 2, "Singer"));

        assert_eq!(coordinator.state.document.as_ref().unwrap().id, "2");
        assert_eq!(coordinator.state.generation, generation);

        coordinator.select(generation, Some("1".into())).unwrap();
        coordinator.playback(sample(42, 2, "Singer、Collaborator"));

        assert_eq!(coordinator.state.lyrics_status, "manual");
        assert_eq!(coordinator.state.generation, generation);

        coordinator.playback(sample(43, 2, "Singer、Collaborator"));

        assert_eq!(coordinator.state.generation, generation + 1);
        assert_eq!(coordinator.state.lyrics_status, "matchedMemoryHash");
        assert_eq!(coordinator.state.document.as_ref().unwrap().id, "2");
        assert!(coordinator.select(generation, Some("1".into())).is_err());

        coordinator.playback(sample(42, 1, "Singer"));

        assert_eq!(coordinator.state.generation, generation + 2);
        assert_eq!(coordinator.state.document.as_ref().unwrap().id, "1");
    }

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
