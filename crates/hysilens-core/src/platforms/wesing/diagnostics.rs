//! 独立探针的分阶段诊断；保留定位、校验与降级证据，不导出歌词正文

use super::Probe;
use crate::protocol::Snapshot;
use serde_json::{Value, json};
use std::time::Instant;

#[cfg(windows)]
use super::{Line, Loading, memory};

#[cfg(windows)]
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Trace {
    pub sample_index: u64,

    pub stage: &'static str,

    pub result: &'static str,

    pub sample_elapsed_ms: f64,

    pub last_failure: Option<Value>,

    pub original_vector: Value,

    pub clock_evaluation: Vec<Value>,

    pub clock_decision: &'static str,

    pub duration_scan: Value,

    pub duration_evaluation: Vec<Value>,

    pub phonetic: Value,
}

#[cfg(windows)]
impl Trace {
    pub fn begin(&mut self) {
        self.sample_index += 1;
        self.stage = "read";
        self.result = "pending";
        self.clock_evaluation.clear();
        self.duration_evaluation.clear();
        self.original_vector = Value::Null;
        self.phonetic = Value::Null;
        self.clock_decision = "notEvaluated";
    }

    pub fn finish<T>(&mut self, result: &anyhow::Result<Option<T>>, elapsed: std::time::Duration) {
        self.sample_elapsed_ms = elapsed.as_secs_f64() * 1000.0;
        self.result = match result {
            Ok(Some(_)) => "ready",
            Ok(None) => "noSongOrProcess",
            Err(error) => {
                let loading = error.downcast_ref::<Loading>().is_some();
                self.last_failure = Some(json!({
                    "stage": self.stage,
                    "sampleIndex": self.sample_index,
                    "error": format!("{error:#}"),
                    "loading": loading,
                }));

                if loading { "loading" } else { "error" }
            }
        };
    }
}

pub(super) fn report(snapshot: &Snapshot) -> Value {
    let mut probe = Probe::new(Instant::now());

    #[cfg(windows)]
    {
        probe.trace = Some(Trace::default());
    }

    let mut report = super::super::diagnostics::samples(|| {
        let sample = probe.sample();

        json!({
            "playback": sample.playback,
            "memory": probe.diagnostics(),
            "document": sample.document.as_ref().map(|document| json!({
                "id": document.id,
                "tracks": document.tracks.iter().map(|track| json!({
                    "kind": track.kind,
                    "lineCount": track.lines.len(),
                    "wordTimedLineCount": track.lines.iter().filter(|line| !line.tokens.is_empty()).count(),
                    "tokenCount": track.lines.iter().map(|line| line.tokens.len()).sum::<usize>(),
                    "firstStartMs": track.lines.first().map(|line| line.start_ms),
                    "lastEndMs": track.lines.iter().filter_map(|line| line.end_ms).max(),
                })).collect::<Vec<_>>(),
                "warnings": document.warnings,
            })),
        })
    });

    report["targetPointerBytes"] = json!(4);
    report["probeMode"] = json!("independentReadOnlyProbe");
    report["runtimeSessionAtStart"] = json!(super::super::diagnostics::session(
        snapshot,
        super::DESCRIPTOR
    ));

    #[cfg(windows)]
    {
        report["processes"] = super::super::diagnostics::windows::processes(&["WeSing.exe"]);

        if let Some(process) = &probe.process {
            report["lyricModule"] = result_value(
                process
                    .module()
                    .map(|(base, size)| json!({ "base": base, "size": size }))
                    .map_err(|error| format!("{error:#}")),
            );
            report["modules"] =
                super::super::diagnostics::windows::modules(process.pid, &["KSongsLyric.dll"]);
        }
    }

    report
}

#[cfg(windows)]
pub(super) fn result_value<T: serde::Serialize>(result: Result<T, String>) -> Value {
    match result {
        Ok(value) => json!({ "value": value }),
        Err(error) => json!({ "error": error }),
    }
}

#[cfg(windows)]
pub(super) fn vector_snapshot(process: &memory::Process, begin_at: Option<u32>) -> Value {
    let result = (|| -> anyhow::Result<Value> {
        let begin_at = begin_at
            .filter(|address| *address <= u32::MAX - 8)
            .ok_or_else(|| anyhow::anyhow!("Vector address overflow"))?;
        let begin = process.u32(begin_at)?;
        let end = process.u32(begin_at + 4)?;
        let capacity = process.u32(begin_at + 8)?;

        Ok(json!({
            "address": begin_at,
            "begin": begin,
            "end": end,
            "capacity": capacity,
            "usedCount": result_value(super::vector_count(begin, end, 1000).map_err(|error| error.to_string())),
            "capacityCount": result_value(super::vector_count(begin, capacity, 1000).map_err(|error| error.to_string())),
            "endWithinCapacity": end <= capacity,
        }))
    })();

    result_value(result.map_err(|error| format!("{error:#}")))
}

#[cfg(windows)]
pub(super) fn clocks(
    process: &memory::Process,
    candidates: &[(u32, f64)],
    elapsed: f64,
) -> Vec<Value> {
    candidates.iter().take(256).map(|(address, previous)| {
        let current = process.float(*address);

        json!({
            "address": address,
            "previousSeconds": previous.is_finite().then_some(previous),
            "elapsedSeconds": elapsed,
            "advancing": current.as_ref().is_ok_and(|current| super::advancing(*previous, *current, elapsed)),
            "currentSeconds": result_value(current.map_err(|error| format!("{error:#}"))),
        })
    }).collect()
}

#[cfg(windows)]
pub(super) fn clock_decision(selected: Option<u32>, candidates: &[Value]) -> &'static str {
    if selected.is_some() {
        return "selected";
    }

    if candidates.is_empty() {
        return "noCandidates";
    }

    let advancing = candidates
        .iter()
        .filter(|candidate| candidate["advancing"] == true)
        .count();

    match advancing {
        0 => "waitingForPlaybackOrFreshCandidates",
        1 => "singleAdvancingCandidate",
        _ => "ambiguousAdvancingCandidates",
    }
}

#[cfg(windows)]
pub(super) fn phonetic(
    process: &memory::Process,
    timer: u32,
    original: &[Line],
    result: &anyhow::Result<Vec<Line>>,
) -> Value {
    json!({
        "vector": vector_snapshot(process, timer.checked_add(0xdc)),
        "result": result_value(result.as_ref().map(|lines| json!({
            "lineCount": lines.len(),
            "aligned": super::aligned(original, lines),
            "nonemptyLineCount": lines.iter().filter(|line| !line.text.trim().is_empty()).count(),
        })).map_err(|error| format!("{error:#}"))),
    })
}

#[cfg(windows)]
pub(super) fn durations(
    process: &memory::Process,
    addresses: &[u32],
    position_ms: f64,
    lyric_end_ms: i32,
) -> Vec<Value> {
    addresses.iter().take(256).map(|address| {
        let result = super::read_progress_text(process, *address);

        json!({
            "address": address,
            "positionMs": position_ms,
            "lyricEndMs": lyric_end_ms,
            "result": result_value(result.map(|(current, duration)| json!({
                "currentSeconds": current,
                "durationSeconds": duration,
                "positionMatches": (current as f64 * 1000.0 - position_ms).abs() <= 1500.0,
                "coversPosition": duration as f64 * 1000.0 >= position_ms,
                "coversLyrics": duration as i64 * 1000 + 1000 >= lyric_end_ms as i64,
                "eligible": super::select_duration(&[(current, duration)], position_ms, lyric_end_ms).is_some(),
            })).map_err(|error| format!("{error:#}"))),
        })
    }).collect()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_unselected_stationary_and_ambiguous_clocks() {
        assert_eq!(clock_decision(None, &[]), "noCandidates");
        assert_eq!(
            clock_decision(None, &[json!({ "advancing": false })]),
            "waitingForPlaybackOrFreshCandidates"
        );
        assert_eq!(
            clock_decision(
                None,
                &[json!({ "advancing": true }), json!({ "advancing": true })]
            ),
            "ambiguousAdvancingCandidates"
        );
        assert_eq!(clock_decision(Some(123), &[]), "selected");
    }
}
