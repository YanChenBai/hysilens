//! 平台按需诊断的注册契约与有界采样工具，不参与日常 capture

#[cfg(windows)]
pub(crate) mod windows;

use super::Descriptor;
use crate::{
    model::{Catalog, Entry, Session, Source},
    protocol::Snapshot,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};

/// 由平台提供的导出说明和同步采集器
#[derive(Clone)]
pub struct Diagnostics {
    /// 确认弹窗和保存窗口中的平台名称
    pub name: &'static str,

    /// 平台实际采集内容的用户说明
    pub description: &'static str,

    /// 在 blocking worker 上调用；不得操作正常 capture 所持有的线程绑定资源
    pub collect: Arc<dyn Fn(&Snapshot) -> Value + Send + Sync>,
}

pub(crate) fn session(snapshot: &Snapshot, descriptor: Descriptor) -> Option<&Session> {
    snapshot
        .session
        .as_ref()
        .filter(|session| {
            session
                .source_id
                .eq_ignore_ascii_case(descriptor.session_id)
        })
        .or_else(|| {
            snapshot.playback.sessions.iter().find(|session| {
                session
                    .source_id
                    .eq_ignore_ascii_case(descriptor.session_id)
            })
        })
}

pub(crate) fn matching(
    snapshot: &Snapshot,
    descriptor: Descriptor,
    catalog: anyhow::Result<Catalog>,
    evidence: fn(&Entry, &Session) -> Value,
) -> Value {
    let catalog = match catalog {
        Ok(catalog) => catalog,
        Err(error) => return json!({ "error": format!("{error:#}") }),
    };
    let mut report = json!({
        "parsedEntryCount": catalog.entries.len(),
        "parseErrorCount": catalog.issues.len(),
        "parseErrors": catalog.issues.iter().take(32).collect::<Vec<_>>(),
    });
    let Some(session) = session(snapshot, descriptor) else {
        report["sessionError"] = json!("采集开始时没有目标平台的媒体会话，无法验证缓存匹配");

        return report;
    };
    let Some(resolve) = descriptor.resolve_cache else {
        return report;
    };
    let resolution = resolve(&catalog, session, None);
    let active = snapshot
        .session
        .as_ref()
        .is_some_and(|active| active.source_id.eq_ignore_ascii_case(descriptor.session_id));

    report["session"] = json!(session);
    report["runtime"] = json!({
        "isTargetActive": active,
        "status": active.then_some(&snapshot.lyrics_status),
        "documentId": active.then(|| snapshot.document.as_ref().map(|document| &document.id)),
    });
    let describe = |entry: &Entry| {
        json!({
            "entry": entry,
            "documentId": entry.document.id,
            "selected": resolution.document.as_ref().is_some_and(|document| document.id == entry.document.id),
            "rawMetadataDifferences": {
                "title": session.title != entry.title,
                "artist": session.artist != entry.artist,
                "album": session.album != entry.album,
            },
            "warnings": entry.document.warnings,
            "platformEvidence": evidence(entry, session),
        })
    };
    let related: Vec<_> = catalog
        .entries
        .iter()
        .filter(|entry| {
            !session.title.is_empty()
                && (entry.title == session.title || entry.filename.contains(&session.title))
                && !resolution
                    .candidates
                    .iter()
                    .any(|candidate| candidate.key == entry.key)
        })
        .collect();

    report["automatic"] = json!({
        "status": resolution.status,
        "selectedDocumentId": resolution.document.as_ref().map(|document| &document.id),
        "selectedFilename": resolution.document.as_ref().map(|document| &document.filename),
        "candidateCount": resolution.candidates.len(),
        "candidates": resolution.candidates.iter().take(64).map(&describe).collect::<Vec<_>>(),
        "relatedRejectedEntryCount": related.len(),
        "relatedRejectedEntries": related.into_iter().take(64).map(describe).collect::<Vec<_>>(),
    });

    report
}

impl std::fmt::Debug for Diagnostics {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Diagnostics")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

pub(crate) fn target(
    source: Source,
    session_id: Option<&str>,
    descriptors: &[Descriptor],
) -> Option<usize> {
    session_id
        .and_then(|id| {
            descriptors
                .iter()
                .position(|item| item.session_id.eq_ignore_ascii_case(id))
        })
        .or_else(|| descriptors.iter().position(|item| item.source == source))
}

pub(crate) fn samples(mut sample: impl FnMut() -> Value) -> Value {
    let mut samples = Vec::with_capacity(13);

    for index in 0..13 {
        if index > 0 {
            std::thread::sleep(Duration::from_millis(250));
        }

        let captured = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let mut value = sample();

        value["capturedUnixMs"] = json!(captured);

        samples.push(value);
    }

    json!({ "sampleIntervalMs": 250, "samples": samples })
}

pub(crate) fn cache(path: &Path, suffix: &str) -> Value {
    let mut files = Vec::new();
    let mut errors = Vec::new();
    let mut count = 0;

    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => return json!({ "directory": path, "error": error.to_string() }),
    };

    for entry in entries {
        let result = entry.and_then(|entry| {
            let metadata = entry.metadata()?;
            let name = entry.file_name().to_string_lossy().into_owned();

            if metadata.is_file() && name.ends_with(suffix) {
                count += 1;

                files.push(json!({
                    "filename": name,
                    "bytes": metadata.len(),
                    "modifiedUnixMs": metadata.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map(|time| time.as_millis()),
                }));
            }

            Ok(())
        });

        if let Err(error) = result
            && errors.len() < 16
        {
            errors.push(error.to_string());
        }

        files.sort_by_key(|file| {
            std::cmp::Reverse(file["modifiedUnixMs"].as_u64().unwrap_or_default())
        });
        files.truncate(12);
    }

    json!({ "directory": path, "fileCount": count, "recentFiles": files, "errors": errors })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_actual_session_before_configured_source_and_keeps_unmatched_auto_empty() {
        let descriptors = super::super::test_descriptors();
        let index = target(Source::Auto, Some("qqmusic.EXE"), &descriptors).unwrap();

        assert_eq!(descriptors[index].source, Source::QqMusic);
        assert!(target(Source::Auto, None, &descriptors).is_none());
        assert!(target(Source::Auto, Some("Unknown.exe"), &descriptors).is_none());

        let index = target(Source::Kugou, None, &descriptors).unwrap();

        assert_eq!(descriptors[index].source, Source::Kugou);
    }

    #[test]
    fn records_missing_cache_and_limits_recent_file_metadata() {
        let directory = tempfile::tempdir().unwrap();

        for index in 0..20 {
            std::fs::write(directory.path().join(format!("{index}_qm.qrc")), b"fixture").unwrap();
        }

        let report = cache(directory.path(), "_qm.qrc");

        assert_eq!(report["fileCount"], 20);
        assert_eq!(report["recentFiles"].as_array().unwrap().len(), 12);
        assert!(cache(&directory.path().join("missing"), "_qm.qrc")["error"].is_string());
    }
}
