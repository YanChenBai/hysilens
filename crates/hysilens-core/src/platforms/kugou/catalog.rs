//! KRC cache 扫描与匹配
//!
//! cache 文件名编码了歌手、标题、audio hash 和歌词 id，因此即使 SMTC 标题很粗糙，
//! 也能按内存身份匹配到文档

use super::lyrics;
use crate::model::{Catalog, Entry, Session};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, sync::Arc};

/// 把 `path` 下每个 `*.krc` 文件加载为 catalog，按文件名排序
///
/// 单个文件解码失败会记录进 [`Catalog::issues`]，而不是中止扫描
///
/// # Errors
///
/// 目录本身无法列出时返回错误
pub fn read(path: &Path) -> anyhow::Result<Catalog> {
    let mut catalog = Catalog::default();

    for entry in fs::read_dir(path)? {
        let path = entry?.path();

        if !path
            .extension()
            .is_some_and(|value| value.eq_ignore_ascii_case("krc"))
        {
            continue;
        }

        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        match lyrics::load_document(&path) {
            Ok(mut document) => {
                document.compact();

                catalog.entries.push(Entry {
                    key: format!("{:x}", Sha256::digest(filename.as_bytes())),
                    filename,
                    title: document.metadata.title.clone(),
                    artist: document.metadata.artist.clone(),
                    album: document.metadata.album.clone(),
                    line_count: document.tracks[0].lines.len(),
                    document: Arc::new(document),
                });
            }
            Err(error) => catalog.issues.push(format!("{filename}: {error:#}")),
        }
    }
    catalog
        .entries
        .sort_by(|left, right| left.filename.cmp(&right.filename));

    Ok(catalog)
}

/// 把 session 与 KRC cache 匹配
///
/// 内存身份（`session.lyric_match`，即 reader 提供了有效 audio hash 时）优先于 SMTC 文本；
/// 手动 `key` 覆盖两者；cache 存在歧义时不返回文档，并给出 `ambiguous` 状态，
/// 以便客户端请求手动选择
pub fn resolve(
    catalog: &Catalog,
    session: &Session,
    key: Option<&str>,
) -> super::super::CatalogResolution {
    if let Some(identity) = session
        .lyric_match
        .as_ref()
        .filter(|identity| identity.file_hashes.iter().any(|hash| valid_hash(hash)))
    {
        let resolution = resolve_memory(catalog, identity, key);

        // 内存身份缺少 hash 匹配时保持 missing；同次读取的身份依次退回按酷狗推荐歌词 ID
        // 与按歌名、歌手匹配；各级遇到多份不同歌词时都给出 ambiguous
        if !(identity.metadata_fallback
            && resolution.document.is_none()
            && resolution.candidates.is_empty())
        {
            return resolution;
        }

        if let Some(resolution) = resolve_lyric_id(catalog, identity) {
            return resolution;
        }

        return resolve_text(catalog, session, key);
    }

    resolve_text(catalog, session, key)
}

/// 按歌名、歌手与文件名匹配，不使用 hash
fn resolve_text(
    catalog: &Catalog,
    session: &Session,
    key: Option<&str>,
) -> super::super::CatalogResolution {
    let candidates: Vec<_> = catalog
        .entries
        .iter()
        .filter(|entry| matches_title(entry, session))
        .cloned()
        .collect();

    let manual = key.and_then(|key| catalog.entries.iter().find(|entry| entry.key == key));
    let eligible: Vec<_> = candidates
        .iter()
        .filter(|entry| matches(entry, session))
        .collect();

    // 完整歌词元数据优先于文件名回退，避免旧缓存遮住当前客户端发布的版本
    // 同一级存在不同歌词内容时仍保留歧义，不按文件时间或排列顺序猜测
    let metadata: Vec<_> = eligible
        .iter()
        .copied()
        .filter(|entry| {
            normalized(&entry.title) == normalized(&session.title)
                && artists(&entry.artist) == artists(&session.artist)
        })
        .collect();

    let preferred = if metadata.is_empty() {
        &eligible
    } else {
        &metadata
    };

    let automatic = preferred.first().copied().filter(|first| {
        preferred
            .iter()
            .all(|entry| entry.document.id == first.document.id)
    });

    // 歌手缺失时仅接受一个完整文件名候选，且歌词时间轴必须落在真实播放范围内
    // 不继承上一首的歌手，也不在多个同名歌曲之间猜测
    let title_only = (session.artist.trim().is_empty() && candidates.len() == 1)
        .then(|| &candidates[0])
        .filter(|entry| matches_without_artist(entry, session));

    let selected = manual.or(automatic).or(title_only);
    let status = if manual.is_some() {
        "manual"
    } else if let Some(entry) = automatic {
        if normalized(&entry.title) == normalized(&session.title)
            && artists(&entry.artist) == artists(&session.artist)
        {
            "matchedMetadata"
        } else if filename_identity(&entry.filename)
            .is_some_and(|(artist, _)| artists(artist) == artists(&session.artist))
        {
            "matchedCacheFilename"
        } else {
            "matchedPrimaryArtist"
        }
    } else if title_only.is_some() {
        "matchedTitleOnly"
    } else if candidates.is_empty() {
        "missing"
    } else {
        "ambiguous"
    };
    super::super::CatalogResolution {
        document: selected.map(|entry| entry.document.clone()),
        candidates,
        status,
    }
}

/// 按酷狗官方歌词搜索给出的推荐歌词 ID 匹配缓存文件名中的 ID，不要求 hash 一致
///
/// 酷狗会以同一首歌其他音质或版本的 hash 命名缓存；该 ID 由当前播放 hash 查询得到，
/// 还需与按当前 hash 下载的歌词内容一致，避免缓存中的 ID 碰撞；没有对应文件时返回 `None`
fn resolve_lyric_id(
    catalog: &Catalog,
    identity: &crate::model::LyricMatch,
) -> Option<super::super::CatalogResolution> {
    let preferred = identity.preferred_lyric_id.as_ref()?;
    let candidates: Vec<_> = catalog
        .entries
        .iter()
        .filter(|entry| {
            cache_fields(&entry.filename).is_some_and(|(_, _, id)| id.to_string() == *preferred)
        })
        .cloned()
        .collect();

    candidates.first()?;
    let confirmed = identity
        .preferred_document_id
        .as_ref()
        .and_then(|expected| {
            candidates
                .iter()
                .find(|entry| entry.document.id == *expected)
        });

    Some(super::super::CatalogResolution {
        document: confirmed.map(|entry| entry.document.clone()),
        status: if confirmed.is_some() {
            "matchedLyricId"
        } else {
            "missing"
        },
        candidates,
    })
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 32
        && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        && hash.bytes().any(|byte| byte != b'0')
}

pub(crate) fn diagnostic_identity(
    session: Option<&Session>,
    catalog: Option<&Catalog>,
) -> serde_json::Value {
    let identity = session.and_then(|session| session.lyric_match.as_ref());
    let valid =
        identity.is_some_and(|identity| identity.file_hashes.iter().any(|hash| valid_hash(hash)));
    let entries = catalog.map_or(&[][..], |catalog| catalog.entries.as_slice());
    let evidence: Vec<_> = session.map_or_else(Vec::new, |session| {
        entries
            .iter()
            .map(|entry| diagnostic_evidence(entry, session))
            .collect()
    });

    serde_json::json!({
        "sessionDetected": session.is_some(),
        "trackId": session.and_then(|session| session.track_id.as_ref()),
        "available": identity.is_some(),
        "validHashAvailable": valid,
        "fileHashes": identity.map(|identity| &identity.file_hashes),
        "preferredLyricId": identity.and_then(|identity| identity.preferred_lyric_id.as_ref()),
        "evidenceSource": "runtimeAtCollectionStart",
        "selectionPolicy": if valid { "memoryHashThenPreferredLyricId" } else { "metadataThenCacheFilename" },
        "cacheParsed": catalog.is_some(),
        "hashMatchedEntryCount": evidence.iter().filter(|entry| entry["hashMatched"] == true).count(),
        "eligibleRecommendedEntryCount": evidence.iter().filter(|entry| entry["recommendationEligible"] == true).count(),
        "unrelatedRecommendedIdEntryCount": evidence.iter().filter(|entry| entry["preferredLyricIdMatched"] == true && entry["hashMatched"] == false).count(),
    })
}

pub(crate) fn diagnostic_evidence(entry: &Entry, session: &Session) -> serde_json::Value {
    let fields = cache_fields(&entry.filename);
    let identity = session.lyric_match.as_ref();
    let hash_match = fields.is_some_and(|(_, hash, _)| {
        valid_hash(hash)
            && identity.is_some_and(|identity| {
                identity
                    .file_hashes
                    .iter()
                    .any(|current| hash.eq_ignore_ascii_case(current))
            })
    });
    let id_match = fields.is_some_and(|(_, _, id)| {
        identity
            .and_then(|identity| identity.preferred_lyric_id.as_ref())
            .is_some_and(|preferred| id.to_string() == *preferred)
    });

    serde_json::json!({
        "cacheHash": fields.map(|(_, hash, _)| hash),
        "cacheLyricId": fields.map(|(_, _, id)| id),
        "hashMatched": hash_match,
        "preferredLyricIdMatched": id_match,
        "recommendationEligible": hash_match && id_match,
        "titleMatched": matches_title(entry, session),
        "metadataEligible": matches(entry, session),
        "titleOnlyEligible": matches_without_artist(entry, session),
    })
}

fn resolve_memory(
    catalog: &Catalog,
    identity: &crate::model::LyricMatch,
    key: Option<&str>,
) -> super::super::CatalogResolution {
    // 身份比 SMTC 文本更强；缺失 hash 匹配时不能退回到同名但属于不同录音/版本的 cache
    let candidates: Vec<_> = catalog
        .entries
        .iter()
        .filter(|entry| {
            cache_fields(&entry.filename).is_some_and(|(_, hash, _)| {
                valid_hash(hash)
                    && identity
                        .file_hashes
                        .iter()
                        .any(|current| hash.eq_ignore_ascii_case(current))
            })
        })
        .cloned()
        .collect();

    let manual = key.and_then(|key| catalog.entries.iter().find(|entry| entry.key == key));

    // 实际报告中推荐 ID 曾在无关歌曲之间撞车；只有在用当前 audio hash 约束过候选之后
    // 才能应用这一偏好
    let recommended: Vec<_> = candidates
        .iter()
        .filter(|entry| {
            identity
                .preferred_lyric_id
                .as_ref()
                .is_some_and(|preferred| {
                    cache_fields(&entry.filename)
                        .is_some_and(|(_, _, id)| id.to_string() == *preferred)
                })
        })
        .collect();

    let preferred = if recommended.is_empty() {
        candidates.iter().collect::<Vec<_>>()
    } else {
        recommended
    };

    let automatic = preferred.first().copied().filter(|first| {
        preferred
            .iter()
            .all(|entry| entry.document.id == first.document.id)
    });

    let document = manual.or(automatic).map(|entry| entry.document.clone());
    let status = if manual.is_some() {
        "manual"
    } else if automatic.is_some() {
        "matchedMemoryHash"
    } else if candidates.is_empty() {
        "missing"
    } else {
        "ambiguous"
    };
    super::super::CatalogResolution {
        document,
        candidates,
        status,
    }
}

fn matches(entry: &Entry, session: &Session) -> bool {
    if session.title.trim().is_empty() || session.artist.trim().is_empty() {
        return false;
    }

    if !entry.album.is_empty()
        && !session.album.is_empty()
        && normalized(&entry.album) != normalized(&session.album)
    {
        return false;
    }

    if normalized(&entry.title) == normalized(&session.title)
        && matches_artists(&entry.artist, &session.artist)
    {
        return true;
    }

    let Some((artist, title)) = filename_identity(&entry.filename) else {
        return false;
    };
    matches_artists(artist, &session.artist)
        && filename_text(title) == filename_text(&session.title)
}

fn matches_without_artist(entry: &Entry, session: &Session) -> bool {
    let Some((artist, title)) = filename_identity(&entry.filename) else {
        return false;
    };

    let Some(duration) = session
        .duration_ms
        .filter(|value| value.is_finite() && *value > 0.0)
    else {
        return false;
    };

    if artist.trim().is_empty()
        || filename_text(title) != filename_text(&session.title)
        || (!entry.album.is_empty()
            && !session.album.is_empty()
            && normalized(&entry.album) != normalized(&session.album))
    {
        return false;
    }

    let Some(original) = entry
        .document
        .tracks
        .iter()
        .find(|track| track.kind == "original")
    else {
        return false;
    };
    !original.lines.is_empty()
        && original.lines.iter().all(|line| {
            line.start_ms >= 0
                && f64::from(line.start_ms) <= duration + 2000.0
                && line
                    .end_ms
                    .is_none_or(|end| end >= line.start_ms && f64::from(end) <= duration + 2000.0)
                && line.tokens.iter().all(|token| {
                    token.start_ms >= 0
                        && token.end_ms >= token.start_ms
                        && f64::from(token.end_ms) <= duration + 2000.0
                })
        })
}

fn matches_title(entry: &Entry, session: &Session) -> bool {
    !session.title.trim().is_empty()
        && (normalized(&entry.title) == normalized(&session.title)
            || filename_identity(&entry.filename)
                .is_some_and(|(_, title)| filename_text(title) == filename_text(&session.title)))
}

fn matches_artists(cached: &str, media: &str) -> bool {
    if artists(cached) == artists(media) {
        return true;
    }

    // 实测酷狗 SMTC 对合唱只发布首位歌手；仅单歌手媒体允许匹配缓存的首位，
    // 最终仍必须通过完整标题与唯一歌词内容校验；不匹配任意一位参与歌手
    let reported = artists(media);
    reported.len() == 1
        && !reported[0].is_empty()
        && cached
            .split(['/', '、', '&', ';', '；', ','])
            .next()
            .is_some_and(|first| normalized(first) == reported[0])
}

fn filename_identity(filename: &str) -> Option<(&str, &str)> {
    cache_fields(filename)?.0.split_once(" - ")
}

fn cache_fields(filename: &str) -> Option<(&str, &str, u64)> {
    let stem = filename.strip_suffix(".krc")?;
    let (stem, flags) = stem.rsplit_once('-')?;
    let (stem, id) = stem.rsplit_once('-')?;
    let (stem, hash) = stem.rsplit_once('-')?;

    if flags.len() != 8
        || !flags.bytes().all(|value| value.is_ascii_hexdigit())
        || id.is_empty()
        || !id.bytes().all(|value| value.is_ascii_digit())
        || hash.len() != 32
        || !hash.bytes().all(|value| value.is_ascii_hexdigit())
    {
        return None;
    }
    Some((stem, hash, id.parse().ok()?))
}

fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn filename_text(value: &str) -> String {
    // 酷狗文件名会删除斜杠等不可用字符；保留版本后缀与标点，不做歌名截短
    value
        .chars()
        .filter(|value| {
            !value.is_whitespace()
                && !matches!(value, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        })
        .flat_map(char::to_lowercase)
        .collect()
}

fn artists(value: &str) -> Vec<String> {
    // 下划线可以属于艺名（如马也_Crabbit），不能当作多歌手分隔符
    let mut artists: Vec<_> = value
        .split(['/', '、', '&', ';', '；', ','])
        .map(normalized)
        .collect();
    artists.sort();
    artists.dedup();
    artists
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, Metadata};

    fn entry() -> Entry {
        Entry { key: "key".into(), filename: "松原みき - 真夜中のドアStay With Me (深夜门扉留在我身边)(シングルver.)-1e1898733bb33fbee0cd6edad4ac0deb-213233970-00000000.krc".into(), title: String::new(), artist: String::new(), album: String::new(), line_count: 0, document: Arc::new(Document { id: "same".into(), filename: String::new(), metadata: Metadata::default(), tracks: vec![], warnings: vec![] }) }
    }

    #[test]
    fn diagnostic_evidence_distinguishes_hash_hits_from_unrelated_recommended_ids() {
        let mut matched = entry();
        matched.filename = "歌手 - 歌曲-11111111111111111111111111111111-42-00000000.krc".into();
        let mut unrelated = matched.clone();
        unrelated.key = "unrelated".into();
        unrelated.filename = "歌手 - 歌曲-22222222222222222222222222222222-42-00000000.krc".into();
        let session = Session {
            source_id: "kugou".into(),
            title: "歌曲".into(),
            lyric_match: Some(crate::model::LyricMatch {
                file_hashes: vec!["11111111111111111111111111111111".into()],
                preferred_lyric_id: Some("42".into()),
                preferred_document_id: None,
                metadata_fallback: false,
            }),
            ..Session::default()
        };
        let catalog = Catalog {
            entries: vec![matched.clone(), unrelated.clone()],
            issues: vec![],
        };
        let report = diagnostic_identity(Some(&session), Some(&catalog));

        assert_eq!(report["hashMatchedEntryCount"], 1);
        assert_eq!(report["eligibleRecommendedEntryCount"], 1);
        assert_eq!(report["unrelatedRecommendedIdEntryCount"], 1);
        assert_eq!(
            diagnostic_evidence(&matched, &session)["recommendationEligible"],
            true
        );
        assert_eq!(
            diagnostic_evidence(&unrelated, &session)["recommendationEligible"],
            false
        );
        assert_eq!(
            diagnostic_identity(Some(&Session::default()), None)["available"],
            false
        );

        let mut snapshot = crate::coordinator::Coordinator::new("diagnostics".into(), vec![]).state;
        snapshot.session = Some(session);
        let report = crate::platforms::diagnostics::matching(
            &snapshot,
            super::super::DESCRIPTOR,
            Ok(catalog),
            diagnostic_evidence,
        );

        assert_eq!(report["automatic"]["status"], "matchedMemoryHash");
        assert_eq!(report["automatic"]["relatedRejectedEntryCount"], 1);
        assert_eq!(
            report["automatic"]["relatedRejectedEntries"][0]["platformEvidence"]["hashMatched"],
            false
        );
    }

    #[test]
    fn matches_sanitized_filename_without_losing_versions_or_guest_artists() {
        let entry = entry();
        let mut session = Session {
            title: "真夜中のドア/Stay With Me (深夜门扉/留在我身边) (シングルver.)".into(),
            artist: "松原みき".into(),
            ..Default::default()
        };
        assert!(matches(&entry, &session));
        session.title.push_str(" (Live)");
        assert!(!matches(&entry, &session));
        assert_eq!(
            artists("马也_Crabbit、Fanko冯思源"),
            artists("Fanko冯思源/马也_Crabbit")
        );
        assert_ne!(artists("A/B"), artists("A"));
    }

    #[test]
    fn matches_search_list_selection_without_artist_only_when_unique_and_bounded() {
        let mut entry = entry();
        entry.filename =
            "中森明菜 - 難破船 (难破船)-7c8a95b38e58fa6bdb45f58c61de6f17-181628032-00000000.krc"
                .into();
        Arc::make_mut(&mut entry.document).tracks = vec![crate::model::LyricTrack {
            kind: "original".into(),
            lines: vec![crate::model::Line {
                start_ms: 1000,
                end_ms: Some(228380),
                text: "fixture".into(),
                tokens: vec![],
            }],
        }];

        let mut session = Session {
            title: "難破船 (难破船)".into(),
            artist: String::new(),
            duration_ms: Some(264000.0),
            ..Default::default()
        };

        let mut catalog = Catalog {
            entries: vec![entry.clone()],
            issues: vec![],
        };
        assert_eq!(resolve(&catalog, &session, None).status, "matchedTitleOnly");
        session.duration_ms = None;
        assert!(resolve(&catalog, &session, None).document.is_none());
        session.duration_ms = Some(100000.0);
        assert!(resolve(&catalog, &session, None).document.is_none());
        session.duration_ms = Some(264000.0);
        session.title.push_str(" (Live)");
        assert!(resolve(&catalog, &session, None).document.is_none());
        session.title = "難破船 (难破船)".into();
        catalog.entries.push(entry);
        assert_eq!(resolve(&catalog, &session, None).status, "ambiguous");
        catalog.entries.pop();
        session.artist = "其他歌手".into();
        assert!(resolve(&catalog, &session, None).document.is_none());
    }

    #[test]
    fn uses_only_the_first_artist_when_smtc_omits_collaborators() {
        let mut entry = entry();
        entry.filename = "李佳薇、张新成 - 甲乙丙丁 (合唱版)-97b4c8a5a50bbd7d39740bf627c660b1-814619243-00000000.krc".into();

        let catalog = Catalog {
            entries: vec![entry],
            issues: vec![],
        };

        let mut session = Session {
            title: "甲乙丙丁 (合唱版)".into(),
            artist: "李佳薇".into(),
            ..Default::default()
        };
        assert_eq!(
            resolve(&catalog, &session, None).status,
            "matchedPrimaryArtist"
        );
        session.artist = "张新成".into();
        assert!(resolve(&catalog, &session, None).document.is_none());
        session.artist = "李佳薇/其他歌手".into();
        assert!(resolve(&catalog, &session, None).document.is_none());
        assert_eq!(resolve(&catalog, &session, None).candidates.len(), 1);
    }

    #[test]
    fn prefers_exact_metadata_over_filename_fallback_without_guessing_between_revisions() {
        let fallback = entry();
        let session = Session {
            title: "真夜中のドア/Stay With Me (深夜门扉/留在我身边) (シングルver.)".into(),
            artist: "松原みき".into(),
            ..Default::default()
        };

        let mut exact = fallback.clone();
        exact.key = "exact".into();
        exact.title = session.title.clone();
        exact.artist = session.artist.clone();
        Arc::make_mut(&mut exact.document).id = "exact-document".into();

        let mut catalog = Catalog {
            entries: vec![fallback, exact.clone()],
            issues: vec![],
        };

        let resolution = resolve(&catalog, &session, None);
        assert_eq!(resolution.status, "matchedMetadata");
        assert_eq!(resolution.document.unwrap().id, "exact-document");
        assert_eq!(resolution.candidates.len(), 2);
        assert_eq!(resolve(&catalog, &session, Some("key")).status, "manual");

        catalog.entries.reverse();
        assert_eq!(resolve(&catalog, &session, None).status, "matchedMetadata");
        exact.key = "revision".into();
        Arc::make_mut(&mut exact.document).id = "other-revision".into();
        catalog.entries.push(exact);
        assert_eq!(resolve(&catalog, &session, None).status, "ambiguous");
    }

    #[test]
    fn rejects_distinct_documents_but_allows_manual_selection() {
        let first = entry();
        let mut second = first.clone();
        second.key = "other".into();
        Arc::make_mut(&mut second.document).id = "different".into();

        let catalog = Catalog {
            entries: vec![first, second],
            issues: vec![],
        };

        let session = Session {
            title: "真夜中のドア/Stay With Me (深夜门扉/留在我身边) (シングルver.)".into(),
            artist: "松原みき".into(),
            ..Default::default()
        };
        assert_eq!(resolve(&catalog, &session, None).status, "ambiguous");
        assert_eq!(resolve(&catalog, &session, Some("key")).status, "manual");
    }

    fn identified_session() -> Session {
        Session {
            title: "Different / SMTC alias".into(),
            artist: "Missing collaborator".into(),
            lyric_match: Some(crate::model::LyricMatch {
                file_hashes: vec!["12".repeat(16), "1e1898733bb33fbee0cd6edad4ac0deb".into()],
                preferred_lyric_id: Some("213233970".into()),
                preferred_document_id: None,
                metadata_fallback: false,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn matches_binary_memory_hash_despite_different_smtc_text_or_missing_duration() {
        let catalog = Catalog {
            entries: vec![entry()],
            issues: vec![],
        };

        let result = resolve(&catalog, &identified_session(), None);
        assert_eq!(result.status, "matchedMemoryHash");
        assert_eq!(result.document.unwrap().id, "same");

        let mut session = identified_session();
        session.lyric_match.as_mut().unwrap().file_hashes[0] =
            session.lyric_match.as_ref().unwrap().file_hashes[1].to_uppercase();
        session.lyric_match.as_mut().unwrap().file_hashes[1].clear();
        assert!(resolve(&catalog, &session, None).document.is_some());
    }

    #[test]
    fn constrains_recommended_lyric_id_by_hash_and_refuses_other_recordings() {
        let first = entry();
        let mut revision = first.clone();
        revision.key = "revision".into();
        revision.filename = revision.filename.replace("-213233970-", "-987-");
        Arc::make_mut(&mut revision.document).id = "revision".into();

        let mut wrong_song = first.clone();
        wrong_song.key = "wrong".into();
        wrong_song.filename = wrong_song
            .filename
            .replace("1e1898733bb33fbee0cd6edad4ac0deb", &"ff".repeat(16));
        Arc::make_mut(&mut wrong_song.document).id = "wrong".into();

        let mut catalog = Catalog {
            entries: vec![wrong_song, revision, first],
            issues: vec![],
        };

        let mut session = identified_session();
        assert_eq!(
            resolve(&catalog, &session, None).document.unwrap().id,
            "same"
        );
        assert_eq!(resolve(&catalog, &session, None).candidates.len(), 2);
        session.lyric_match.as_mut().unwrap().preferred_lyric_id = Some("987".into());
        assert_eq!(
            resolve(&catalog, &session, None).document.unwrap().id,
            "revision"
        );
        session.lyric_match.as_mut().unwrap().preferred_lyric_id = None;
        assert_eq!(resolve(&catalog, &session, None).status, "ambiguous");
        assert_eq!(resolve(&catalog, &session, Some("wrong")).status, "manual");
        catalog.entries.reverse();
        assert_eq!(resolve(&catalog, &session, None).status, "ambiguous");

        session.title = catalog.entries[0].title.clone();
        session.lyric_match.as_mut().unwrap().file_hashes[0] = "ab".repeat(16);
        session.lyric_match.as_mut().unwrap().file_hashes[1] = "cd".repeat(16);
        session.lyric_match.as_mut().unwrap().preferred_lyric_id = Some("213233970".into());
        assert_eq!(resolve(&catalog, &session, None).status, "missing");
        assert!(resolve(&catalog, &session, None).document.is_none());
    }

    #[test]
    fn enhanced_identity_falls_back_to_metadata_when_no_cache_hash_matches() {
        // 实测：酷狗以另一版本的 hash 命名刚下载的 KRC，当前播放 hash 与之不同
        let cached = entry();
        let (artist, title) = filename_identity(&cached.filename)
            .map(|(artist, title)| (artist.to_owned(), title.to_owned()))
            .unwrap();
        let catalog = Catalog {
            entries: vec![cached],
            issues: vec![],
        };
        let mut session = Session {
            title,
            artist,
            lyric_match: Some(crate::model::LyricMatch {
                file_hashes: vec!["ab".repeat(16)],
                preferred_lyric_id: None,
                preferred_document_id: None,
                metadata_fallback: true,
            }),
            ..Default::default()
        };

        let resolution = resolve(&catalog, &session, None);
        assert!(resolution.document.is_some());
        assert_ne!(resolution.status, "missing");

        session.lyric_match.as_mut().unwrap().metadata_fallback = false;
        assert_eq!(resolve(&catalog, &session, None).status, "missing");
    }

    fn cached(filename: &str, id: &str) -> Entry {
        let mut cached = entry();
        cached.key = id.into();
        cached.filename = filename.into();
        Arc::make_mut(&mut cached.document).id = id.into();
        cached
    }

    #[test]
    fn enhanced_identity_matches_recommended_lyric_id_across_hashes() {
        // 实测：《你怎么舍得我难过 (烟嗓版)》播放 hash 为 8ac66fdc…，酷狗官方歌词搜索推荐
        // 713023760，而酷狗缓存的 KRC 用另一 hash、原版标题命名；歌名与 hash 都对不上
        let mut session = Session {
            title: "你怎么舍得我难过 (烟嗓版)".into(),
            artist: "谢尔西".into(),
            lyric_match: Some(crate::model::LyricMatch {
                file_hashes: vec!["8ac66fdc5445e5bd8c76e979af022796".into()],
                preferred_lyric_id: Some("713023760".into()),
                preferred_document_id: Some("recommended".into()),
                metadata_fallback: true,
            }),
            ..Default::default()
        };
        let mut catalog = Catalog {
            entries: vec![
                cached(
                    "谢尔西 - 你怎么舍得我难过-0c66b54f2db9050557eae282010f7c19-713023760-00000000.krc",
                    "recommended",
                ),
                cached(
                    "谢尔西 - 你怎么舍得我难过-0c66b54f2db9050557eae282010f7c19-713023123-00000000.krc",
                    "alternative",
                ),
            ],
            issues: vec![],
        };

        let resolution = resolve(&catalog, &session, None);
        assert_eq!(resolution.status, "matchedLyricId");
        assert_eq!(resolution.document.unwrap().id, "recommended");
        assert_eq!(
            resolve(&catalog, &session, Some("alternative")).status,
            "manual"
        );

        // 尚未查到推荐 ID 时，原版标题与“(烟嗓版)”不同，不按歌名猜测
        session.lyric_match.as_mut().unwrap().preferred_lyric_id = None;
        assert_eq!(resolve(&catalog, &session, None).status, "missing");

        // 内存身份的推荐 ID 曾在无关歌曲间撞车，仍必须先受 hash 约束
        session.lyric_match.as_mut().unwrap().preferred_lyric_id = Some("713023760".into());
        session.lyric_match.as_mut().unwrap().metadata_fallback = false;
        assert_eq!(resolve(&catalog, &session, None).status, "missing");

        // 推荐 ID 碰撞时，只接受与下载内容一致的本地缓存
        session.lyric_match.as_mut().unwrap().metadata_fallback = true;
        catalog.entries.push(cached(
            "谢尔西 - 你怎么舍得我难过-11111111111111111111111111111111-713023760-00000000.krc",
            "other-content",
        ));
        assert_eq!(
            resolve(&catalog, &session, None).document.unwrap().id,
            "recommended"
        );

        session.lyric_match.as_mut().unwrap().preferred_document_id = None;
        assert!(resolve(&catalog, &session, None).document.is_none());

        session.lyric_match.as_mut().unwrap().preferred_document_id = Some("unrelated".into());
        assert!(resolve(&catalog, &session, None).document.is_none());
    }

    #[test]
    fn zero_or_missing_memory_identity_keeps_existing_filename_fallback() {
        let cached = entry();
        let (artist, title) = filename_identity(&cached.filename).unwrap();
        let mut session = Session {
            title: title.into(),
            artist: artist.into(),
            ..Default::default()
        };

        let catalog = Catalog {
            entries: vec![cached],
            issues: vec![],
        };

        let previous = resolve(&catalog, &session, None).status;
        session.lyric_match = Some(crate::model::LyricMatch {
            file_hashes: vec!["0".repeat(32)],
            ..Default::default()
        });
        assert_eq!(resolve(&catalog, &session, None).status, previous);
    }
}
