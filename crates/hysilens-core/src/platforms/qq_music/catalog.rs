use super::lyrics;
#[cfg(test)]
use crate::model::Document;
use crate::model::Session;
pub use crate::model::{Catalog, Entry};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, sync::Arc};

impl Catalog {
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        let mut catalog = Self::default();

        for entry in fs::read_dir(path)? {
            let path = entry?.path();

            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            if !filename.ends_with("_qm.qrc") {
                continue;
            }

            match lyrics::load_document(&path) {
                Ok(document) => {
                    let key = format!("{:x}", Sha256::digest(filename.as_bytes()));

                    catalog.entries.push(Entry {
                        key,
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

    pub fn candidates(&self, session: &Session) -> Vec<Entry> {
        if session.title.trim().is_empty() {
            return Vec::new();
        }

        self.entries
            .iter()
            .filter(|entry| same(&entry.title, &session.title) || matches_filename(entry, session))
            .cloned()
            .collect()
    }

    pub fn automatic<'a>(&self, candidates: &'a [Entry], session: &Session) -> Option<&'a Entry> {
        if session.artist.trim().is_empty() {
            return None;
        }

        let matches: Vec<_> = candidates
            .iter()
            .filter(|entry| matches_metadata(entry, session) || matches_filename(entry, session))
            .collect();

        let first = *matches.first()?;

        // 不同文件名可能保存完全相同的原文、译文、罗马音和时间轴。
        // 只对已通过完整身份校验的记录按内容摘要判断是否存在真实歧义。
        if matches
            .iter()
            .all(|entry| entry.document.id == first.document.id)
        {
            return matches.into_iter().max_by_key(|entry| {
                let duration = filename_duration(entry, session);

                let is_close = duration
                    .zip(session.duration_ms)
                    .is_some_and(|(cache, playback)| (cache - playback).abs() <= 1500.0);

                (is_close, duration.is_some_and(|value| value > 0.0))
            });
        }

        // QQ 的时间轴可能仅表示试听范围。时长只用于多个完整身份候选之间的消歧。
        let mut by_duration = matches.into_iter().filter(|entry| {
            matches_filename(entry, session)
                && filename_duration(entry, session)
                    .zip(session.duration_ms)
                    .is_some_and(|(cache, playback)| (cache - playback).abs() <= 1500.0)
        });

        let first = by_duration.next()?;

        by_duration.next().is_none().then_some(first)
    }
}

pub fn resolve(
    catalog: &Catalog,
    session: &Session,
    key: Option<&str>,
) -> super::super::CatalogResolution {
    let candidates = catalog.candidates(session);

    let manual = key.and_then(|key| catalog.entries.iter().find(|entry| entry.key == key));

    let automatic = catalog.automatic(&candidates, session);

    let entry = manual.or(automatic);

    let status = if let Some(entry) = entry {
        if manual.is_some() {
            "manual"
        } else if matches_metadata(entry, session) {
            "matchedMetadata"
        } else {
            "matchedCacheFilename"
        }
    } else if candidates.is_empty() {
        "missing"
    } else {
        "ambiguous"
    };

    super::super::CatalogResolution {
        document: entry.map(|entry| entry.document.clone()),
        candidates,
        status,
    }
}

pub fn matches_filename(entry: &Entry, session: &Session) -> bool {
    filename_duration(entry, session).is_some()
}

pub fn matches_metadata(entry: &Entry, session: &Session) -> bool {
    same(&entry.title, &session.title)
        && same_artists(&entry.artist, &session.artist)
        && same(&entry.album, &session.album)
}

fn filename_duration(entry: &Entry, session: &Session) -> Option<f64> {
    let (artist, remainder) = entry.filename.strip_suffix("_qm.qrc")?.split_once(" - ")?;

    if !same_artists(artist, &session.artist) {
        return None;
    }

    // 标题和专辑自身也可能含有分隔符；逐个验证标题边界，不按固定四段截断。
    for (boundary, _) in remainder.match_indices(" - ") {
        if !same_filename_text(&remainder[..boundary], &session.title) {
            continue;
        }

        let Some((seconds, album)) = remainder[boundary + 3..].split_once(" - ") else {
            continue;
        };

        let Ok(seconds) = seconds.parse::<f64>() else {
            continue;
        };

        if seconds.is_finite() && seconds >= 0.0 && same_filename_text(album, &session.album) {
            return Some(seconds * 1000.0);
        }
    }

    None
}

fn same(left: &str, right: &str) -> bool {
    normalized_text(left) == normalized_text(right)
}

fn same_filename_text(cached: &str, media: &str) -> bool {
    if same(cached, media) {
        return true;
    }

    // QQ 缓存将 Windows 不可用字符及句点替换为下划线。
    // 只投影媒体元数据到文件名形式，不把下划线反向猜成标点，也不改歌词内部元数据。
    let projected: String = media
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '.'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect();

    same(cached, &projected)
}

fn normalized_text(value: &str) -> String {
    // 折叠包括 NBSP、全角空格在内的 Unicode 空白，保留词间间隔和版本文字。
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn same_artists(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        let mut artists: Vec<_> = value.split(['/', '_']).map(normalized_text).collect();

        // 单个名字中的前后下划线保留；只归一有完整成员的多歌手分隔。
        if artists.iter().any(String::is_empty) {
            return vec![normalized_text(value)];
        }

        artists.sort();

        artists.dedup();

        artists
    };

    normalize(left) == normalize(right)
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::model::Metadata;

    fn entry(key: &str, album: &str) -> Entry {
        Entry {
            key: key.into(),
            filename: key.into(),
            title: "A".into(),
            artist: "Singer".into(),
            album: album.into(),
            line_count: 0,
            document: Arc::new(Document {
                id: key.into(),
                filename: key.into(),
                metadata: Metadata::default(),
                tracks: vec![],
                warnings: vec![],
            }),
        }
    }

    #[test]
    fn requires_artist_album_and_unique_candidate() {
        let catalog = Catalog {
            entries: vec![entry("1", "Studio"), entry("2", "Live")],
            issues: vec![],
        };

        let mut session = Session {
            title: "A".into(),
            artist: "Singer".into(),
            album: "Studio".into(),
            ..Session::default()
        };

        let candidates = catalog.candidates(&session);

        assert_eq!(catalog.automatic(&candidates, &session).unwrap().key, "1");

        let duplicates = vec![entry("1", "Studio"), entry("3", "Studio")];

        assert!(catalog.automatic(&duplicates, &session).is_none());

        session.album.clear();

        assert!(catalog.automatic(&candidates, &session).is_none());
    }

    #[test]
    fn accepts_cache_title_when_lyric_header_has_a_cover_suffix() {
        let mut cached = entry("cover", "【泠鸢翻唱2019】");

        cached.title = "勾指起誓（Cover：ilem）".into();

        cached.artist = "泠鸢yousa".into();

        cached.filename = "泠鸢yousa - 勾指起誓 - 183 - 【泠鸢翻唱2019】_qm.qrc".into();

        let catalog = Catalog {
            entries: vec![cached],
            issues: vec![],
        };

        let mut session = Session {
            title: "勾指起誓".into(),
            artist: "泠鸢yousa".into(),
            album: "【泠鸢翻唱2019】".into(),
            duration_ms: Some(183529.0),
            ..Session::default()
        };

        let candidates = catalog.candidates(&session);

        assert_eq!(candidates.len(), 1);

        assert!(catalog.automatic(&candidates, &session).is_some());

        session.artist = "ilem".into();

        assert!(catalog.candidates(&session).is_empty());

        session.artist = "泠鸢yousa".into();

        session.duration_ms = Some(200_000.0);

        assert!(
            catalog
                .automatic(&catalog.candidates(&session), &session)
                .is_some()
        );

        session.duration_ms = Some(183529.0);

        session.title = "勾指起誓 (Live)".into();

        assert!(catalog.candidates(&session).is_empty());
    }

    #[test]
    fn uses_cache_album_when_lyrics_were_reused_from_a_compilation() {
        let mut cached = entry("sakura", "城城完全精选");

        cached.title = "Para Para Sakura".into();

        cached.artist = "郭富城".into();

        cached.filename = "郭富城 - Para Para Sakura - 223 - 芭啦芭啦樱之花_qm.qrc".into();

        let catalog = Catalog {
            entries: vec![cached],
            issues: vec![],
        };

        let mut session = Session {
            title: "Para Para Sakura".into(),
            artist: "郭富城".into(),
            album: "芭啦芭啦樱之花".into(),
            duration_ms: Some(223000.0),
            ..Session::default()
        };

        let candidates = catalog.candidates(&session);

        assert!(catalog.automatic(&candidates, &session).is_some());

        session.duration_ms = Some(60_055.0);

        assert!(catalog.automatic(&candidates, &session).is_some());

        session.album = "其他专辑".into();

        assert!(catalog.automatic(&candidates, &session).is_none());
    }

    #[test]
    fn treats_identical_lyric_documents_as_one_choice_without_duration() {
        let mut unknown = entry("unknown", "Studio");

        unknown.filename = "Singer - A - 0 - Studio_qm.qrc".into();

        let mut known = entry("known", "Studio");

        known.filename = "Singer - A - 232 - Studio_qm.qrc".into();

        known.document = unknown.document.clone();

        let catalog = Catalog {
            entries: vec![unknown, known],
            issues: vec![],
        };

        let session = Session {
            title: "A".into(),
            artist: "Singer".into(),
            album: "Studio".into(),
            ..Session::default()
        };

        let candidates = catalog.candidates(&session);

        assert_eq!(
            catalog.automatic(&candidates, &session).unwrap().key,
            "known"
        );

        let mut different = candidates.clone();

        different[1].document = entry("different-content", "Studio").document;

        assert!(catalog.automatic(&different, &session).is_none());
    }

    #[test]
    fn matches_multiple_artists_and_unicode_spacing_without_dropping_guests() {
        let mut cached = entry("tough-days", "实况电影");

        cached.title = "低潮期".into();

        cached.artist = "丁世光".into();

        cached.filename = "丁世光_叶喜儿 - 低潮期 \u{a0}Tough Days - 251 - 实况电影_qm.qrc".into();

        let catalog = Catalog {
            entries: vec![cached],
            issues: vec![],
        };

        let mut session = Session {
            title: "低潮期  Tough Days".into(),
            artist: "丁世光/叶喜儿".into(),
            album: "实况电影".into(),
            duration_ms: Some(251764.0),
            ..Session::default()
        };

        let candidates = catalog.candidates(&session);

        assert_eq!(candidates.len(), 1);

        assert_eq!(
            catalog.automatic(&candidates, &session).unwrap().key,
            "tough-days"
        );

        assert_eq!(filename_duration(&candidates[0], &session), Some(251000.0));

        session.artist = "丁世光".into();

        assert!(catalog.automatic(&candidates, &session).is_none());

        session.artist = "丁世光/另一位歌手".into();

        assert!(catalog.automatic(&candidates, &session).is_none());

        session.artist = "丁世光/叶喜儿".into();

        session.title = "低潮期 Tough Days (Live)".into();

        assert!(catalog.automatic(&candidates, &session).is_none());
    }

    #[test]
    fn preserves_delimiters_inside_titles_and_albums() {
        let mut cached = entry("hyphen", "Album - Part 1");

        cached.filename = "Singer - A - Part 1 - 232 - Album - Part 1_qm.qrc".into();

        let session = Session {
            title: "A - Part 1".into(),
            artist: "Singer".into(),
            album: "Album - Part 1".into(),
            ..Session::default()
        };

        assert!(matches_filename(&cached, &session));
    }

    #[test]
    fn matches_sanitized_cache_album_without_removing_opera_title_prefix() {
        let mut cached = entry("opera", "粤剧名曲精华第七辑：唐伯虎点秋香");

        cached.title = "【粤剧】天蓬元帅戏嫦娥".into();

        cached.artist = "尹光/胡美仪".into();

        cached.filename =
            "尹光_胡美仪 - 天蓬元帅戏嫦娥 - 382 - 粤剧名曲精华, Vol_ 7_ 唐伯虎点秋香_qm.qrc".into();

        let catalog = Catalog {
            entries: vec![cached],
            issues: vec![],
        };

        let mut session = Session {
            title: "天蓬元帅戏嫦娥".into(),
            artist: "尹光/胡美仪".into(),
            album: "粤剧名曲精华, Vol. 7: 唐伯虎点秋香".into(),
            duration_ms: Some(60_055.0),
            ..Session::default()
        };

        let candidates = catalog.candidates(&session);

        assert_eq!(candidates.len(), 1);

        assert!(!matches_metadata(&candidates[0], &session));

        assert!(catalog.automatic(&candidates, &session).is_some());

        session.album = "粤剧名曲精华, Vol. 8: 唐伯虎点秋香".into();

        assert!(catalog.automatic(&candidates, &session).is_none());

        session.album = "粤剧名曲精华, Vol. 7: 唐伯虎点秋香".into();

        session.title = "唐伯虎点秋香".into();

        assert!(catalog.automatic(&candidates, &session).is_none());

        assert!(same_filename_text("Song_ Part _", "Song: Part ?"));

        assert!(!same("Vol_ 7_", "Vol. 7:"));

        assert!(!same_filename_text("Vol. 7:", "Vol_ 7_"));
    }

    #[test]
    fn accepts_both_empty_albums_but_never_treats_missing_album_as_a_wildcard() {
        let mut cached = entry("xiangyao", "");

        cached.title = "帝女花之香夭 (粤剧版)".into();

        cached.artist = "尹光/胡美仪".into();

        cached.filename = "尹光_胡美仪 - 帝女花之香夭 - 376 - _qm.qrc".into();

        let catalog = Catalog {
            entries: vec![cached],
            issues: vec![],
        };

        let mut session = Session {
            title: "帝女花之香夭".into(),
            artist: "尹光/胡美仪".into(),
            album: String::new(),
            duration_ms: Some(376856.0),
            ..Session::default()
        };

        let candidates = catalog.candidates(&session);

        assert_eq!(candidates.len(), 1);

        assert_eq!(
            catalog.automatic(&candidates, &session).unwrap().key,
            "xiangyao"
        );

        session.album = "其他专辑".into();

        assert!(catalog.automatic(&candidates, &session).is_none());

        let named = vec![entry("named-album", "Studio")];

        let missing = Session {
            title: "A".into(),
            artist: "Singer".into(),
            ..Session::default()
        };

        assert!(catalog.automatic(&named, &missing).is_none());

        let distinct = vec![entry("version-1", ""), entry("version-2", "")];

        assert!(catalog.automatic(&distinct, &missing).is_none());
    }
}
