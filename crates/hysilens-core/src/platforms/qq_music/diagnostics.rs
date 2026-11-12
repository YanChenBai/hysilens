//! QQ 音乐按需报告，只读取 SMTC 与歌词 cache 的文件元数据

use super::Probe;
use serde_json::{Value, json};
use std::{path::Path, time::Instant};

struct DiagnosticMedia(Option<Probe>);

impl Drop for DiagnosticMedia {
    fn drop(&mut self) {
        if let Some(probe) = self.0.take() {
            drop(probe);

            #[cfg(windows)]
            // SAFETY: 配平同一 worker 上成功的 Probe::new；probe 与所有 WinRT 对象已先释放
            unsafe {
                windows::Win32::System::Com::CoUninitialize()
            };
        }
    }
}

pub(super) fn report(cache: &Path, snapshot: &crate::protocol::Snapshot) -> Value {
    let probe = Probe::new(Instant::now());
    let error = probe.as_ref().err().map(|error| format!("{error:#}"));
    let probe = DiagnosticMedia(probe.ok());
    let mut report = super::super::diagnostics::samples(|| {
        let Some(probe) = probe.0.as_ref() else {
            return json!({ "error": error });
        };

        let mut playback = probe.sample();
        let source_ids: Vec<_> = playback
            .sessions
            .iter()
            .map(|session| session.source_id.clone())
            .collect();
        playback.sessions.retain(|session| {
            session
                .source_id
                .eq_ignore_ascii_case(super::DESCRIPTOR.session_id)
        });

        json!({
            "qqSessionDetected": !playback.sessions.is_empty(),
            "playback": playback,
            "observedSourceIds": source_ids,
        })
    });

    report["cache"] = super::super::diagnostics::cache(cache, "_qm.qrc");
    report["matching"] = super::super::diagnostics::matching(
        snapshot,
        super::DESCRIPTOR,
        super::catalog::Catalog::read(cache),
        |entry, session| {
            json!({
                "matchesMetadata": super::catalog::matches_metadata(entry, session),
                "matchesFilename": super::catalog::matches_filename(entry, session),
            })
        },
    );

    #[cfg(windows)]
    {
        report["processes"] = super::super::diagnostics::windows::processes(&["QQMusic.exe"]);
    }

    report
}
