//! 全民K歌 adapter
//!
//! 播放位置、状态和内嵌歌词模型都来自对运行中 `WeSing.exe`（仅 x86）的只读扫描
//! 歌词 host 结构由 module pattern scan 定位，缺失期间每两秒重试一次；没有歌词
//! cache，因此扫描失败会得到 unavailable session，而不是过期的歌词

// 内存布局和定位流程参考 WesingCap，MIT 声明见 THIRD_PARTY_NOTICES.md
#[cfg(windows)]
mod memory;

use crate::model::{Document, Playback, Session};
#[cfg(windows)]
use crate::model::{Line, LyricTrack, Metadata, Token};
#[cfg(windows)]
use anyhow::{Context, ensure};
#[cfg(windows)]
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Instant};

#[derive(Debug)]
struct Loading;

impl std::fmt::Display for Loading {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Waiting for the new lyric model")
    }
}

impl std::error::Error for Loading {}

pub use super::Sample;

/// WeSing 的静态 descriptor，歌词从播放器内存中提取
pub const DESCRIPTOR: super::Descriptor = super::Descriptor {
    source: crate::model::Source::Wesing,
    session_id: "WeSing.exe",
    lyrics: super::LyricsMode::Embedded,
    resolve_cache: None,
};

/// 创建 WeSing adapter 工厂，播放器进程在首次 poll 时才惰性定位，
/// 而不是在构建 registration 时
pub fn registration() -> super::Registration {
    super::Registration {
        descriptor: DESCRIPTOR,
        create: Box::new(|context| Box::new(Probe::new(context.origin))),
    }
}

impl super::Adapter for Probe {
    fn sample(&mut self) -> Sample {
        Probe::sample(self)
    }
}

/// WeSing adapter 状态：已附加的进程，以及上次 scan 定位到的所有地址
pub struct Probe {
    origin: Instant,

    #[cfg(windows)]
    process: Option<memory::Process>,

    #[cfg(windows)]
    timer_candidates: Vec<(u32, f64)>,

    #[cfg(windows)]
    timer: Option<u32>,

    #[cfg(windows)]
    title: String,

    #[cfg(windows)]
    last_sample: Instant,

    #[cfg(windows)]
    last_scan: Option<Instant>,

    #[cfg(windows)]
    previous_time: Option<f64>,

    #[cfg(windows)]
    last_moved: Instant,

    #[cfg(windows)]
    host: Option<u32>,

    #[cfg(windows)]
    duration_addresses: Vec<u32>,

    #[cfg(windows)]
    last_duration_scan: Option<Instant>,
}

impl std::fmt::Debug for Probe {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = formatter.debug_struct("Probe");

        debug.field("origin", &self.origin);

        #[cfg(windows)]
        {
            debug.field("pid", &self.process.as_ref().map(|process| process.pid));
            debug.field("title", &self.title);
            debug.field("timer", &self.timer);
        }

        debug.finish_non_exhaustive()
    }
}

impl Probe {
    /// 创建一个 adapter，其 `server_ms` 原点为 `origin`
    pub fn new(origin: Instant) -> Self {
        Self {
            origin,
            #[cfg(windows)]
            process: None,
            #[cfg(windows)]
            timer_candidates: vec![],
            #[cfg(windows)]
            timer: None,
            #[cfg(windows)]
            title: String::new(),
            #[cfg(windows)]
            last_sample: Instant::now(),
            #[cfg(windows)]
            last_scan: None,
            #[cfg(windows)]
            previous_time: None,
            #[cfg(windows)]
            last_moved: Instant::now(),
            #[cfg(windows)]
            host: None,
            #[cfg(windows)]
            duration_addresses: vec![],
            #[cfg(windows)]
            last_duration_scan: None,
        }
    }

    /// 对播放器 poll 一次
    ///
    /// 歌词 host 仍在定位时报告 `loading` session，扫描失败时报告携带错误文本的
    /// `unavailable` session，仅 Windows 可用；其他 target 始终报告 `unavailable`
    pub fn sample(&mut self) -> Sample {
        #[cfg(windows)]
        let result = self.read();

        #[cfg(not(windows))]
        let result: anyhow::Result<Option<(Session, Option<Arc<Document>>)>> =
            Err(anyhow::anyhow!("WeSing requires Windows"));

        let (sessions, document) = match result {
            Ok(Some((session, document))) => (vec![session], document),
            Ok(None) => (vec![], None),
            Err(error) if error.downcast_ref::<Loading>().is_some() => (
                vec![Session {
                    source_id: "WeSing.exe".into(),
                    title: self.current_title(),
                    status: "loading".into(),
                    ..Session::default()
                }],
                None,
            ),
            Err(error) => (
                vec![Session {
                    source_id: "WeSing.exe".into(),
                    title: self.current_title(),
                    status: "unavailable".into(),
                    error: Some(format!("{error:#}")),
                    ..Session::default()
                }],
                None,
            ),
        };

        Sample {
            playback: Playback {
                server_ms: self.origin.elapsed().as_secs_f64() * 1000.0,
                sessions,
                error: None,
            },
            document,
        }
    }

    /// 返回已附加的 pid、定位到的 clock 地址和候选计数，供按需报告使用
    /// 非 Windows target 上为空
    pub fn diagnostics(&self) -> serde_json::Value {
        #[cfg(windows)]
        {
            serde_json::json!({
                "pid": self.process.as_ref().map(|process| process.pid),
                "clockAddress": self.timer,
                "clockCandidates": self.timer_candidates,
                "durationCandidates": self.duration_addresses.len()
            })
        }

        #[cfg(not(windows))]
        {
            serde_json::json!({})
        }
    }

    fn current_title(&self) -> String {
        #[cfg(windows)]
        {
            self.title.clone()
        }

        #[cfg(not(windows))]
        {
            String::new()
        }
    }

    #[cfg(windows)]
    fn read(&mut self) -> anyhow::Result<Option<(Session, Option<Arc<Document>>)>> {
        if self
            .process
            .as_ref()
            .is_some_and(|process| !process.alive())
        {
            self.process = None;

            self.reset();

            self.title.clear();
        }

        if self.process.is_none() {
            let Some(pid) = memory::find_process()? else {
                return Ok(None);
            };

            self.process = Some(memory::Process::open(pid)?);
        }

        let title = self.process.as_ref().unwrap().window_title()?;

        if title != self.title {
            self.reset();

            self.title = title.clone();
        }

        if title.is_empty() {
            return Ok(None);
        }

        let process = self.process.as_ref().unwrap();
        let now = Instant::now();

        if self.host.is_none()
            && self
                .last_scan
                .is_none_or(|scan| now.duration_since(scan).as_secs() >= 2)
        {
            self.last_scan = Some(now);

            let host = locate_host(process)?;
            let candidates = locate_timers(process)?;

            self.host = Some(host);

            self.timer_candidates = candidates;
        }

        let Some(host) = self.host else {
            return Ok(Some((
                Session {
                    source_id: "WeSing.exe".into(),
                    title,
                    status: "loading".into(),
                    ..Session::default()
                },
                None,
            )));
        };

        let lines = match read_lines(process, host) {
            Ok(lines) => lines,
            Err(error) => {
                self.host = None;

                self.timer = None;

                return Err(error);
            }
        };

        if self.timer.is_some()
            && self
                .last_duration_scan
                .is_none_or(|scan| now.duration_since(scan).as_secs() >= 5)
        {
            let pattern: Vec<_> = " | "
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .map(Some)
                .collect();

            self.duration_addresses = process.scan(&pattern).unwrap_or_default();

            self.last_duration_scan = Some(now);
        }

        // 大范围搜索结束后再采样播放位置，避免给旧位置附上新的时间锚点
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_sample).as_secs_f64();

        if self.timer.is_none() && !self.timer_candidates.is_empty() {
            let moving: Vec<_> = self
                .timer_candidates
                .iter()
                .filter_map(|(address, previous)| {
                    let value = process.float(*address).ok()?;

                    advancing(*previous, value, elapsed).then_some(*address)
                })
                .collect();

            if moving.len() == 1 {
                self.timer = Some(moving[0]);
            }
        }

        for (address, previous) in &mut self.timer_candidates {
            *previous = process.float(*address).unwrap_or(f64::NAN);
        }

        self.last_sample = now;

        let position = self.timer.map(|address| process.float(address)).transpose();
        let position = match position {
            Ok(position) => position,
            Err(error) => {
                self.reset();

                return Err(error);
            }
        };

        let moving = position
            .zip(self.previous_time)
            .is_some_and(|(current, previous)| current != previous);

        if moving || (position.is_some() && self.previous_time.is_none()) {
            self.last_moved = now;
        }

        let rate = position
            .zip(self.previous_time)
            .and_then(|(current, previous)| advancing(previous, current, elapsed).then_some(1.0));

        self.previous_time = position;

        // 歌词结束不等于歌曲结束，不用最后一句时间猜测总时长
        let status = if position.is_none() {
            "unknown"
        } else if now.duration_since(self.last_moved).as_millis() >= 750 {
            "paused"
        } else {
            "playing"
        };

        let mut tracks = vec![LyricTrack {
            kind: "original".into(),
            lines,
        }];

        let mut warnings = Vec::new();

        if let Some(timer) = self.timer {
            match timer
                .checked_add(0xe4)
                .context("Phonetic model address overflow")
                .and_then(|capacity_at| {
                    read_timeline(process, capacity_at - 8, capacity_at - 4, capacity_at)
                }) {
                Ok(phonetic) if aligned(&tracks[0].lines, &phonetic) => {
                    if phonetic.iter().any(|line| !line.text.trim().is_empty()) {
                        tracks.push(LyricTrack {
                            kind: "romanization".into(),
                            lines: phonetic,
                        });
                    }
                }
                Ok(_) => warnings.push("注音模型与原文时间轴不一致，暂不显示注音。".into()),
                Err(error) if error.downcast_ref::<Loading>().is_some() => {}
                Err(_) => warnings.push("当前注音模型不可用，原文继续显示。".into()),
            }
        }

        let duration_ms = if let Some(position) = position {
            let candidates: Vec<_> = self
                .duration_addresses
                .iter()
                .filter_map(|address| read_progress_text(process, *address).ok())
                .collect();

            let lyric_end = tracks[0]
                .lines
                .iter()
                .filter_map(|line| line.end_ms)
                .max()
                .unwrap_or(0);

            select_duration(&candidates, position * 1000.0, lyric_end)
        } else {
            None
        };

        if duration_ms.is_none() {
            warnings.push("当前没有唯一可信的歌曲总时长。".into());
        }

        if tracks[0]
            .lines
            .iter()
            .any(|line| !line.text.is_empty() && line.tokens.is_empty())
        {
            warnings.push("部分歌词没有可信的逐词时间，按逐行显示。".into());
        }

        ensure!(
            process.window_title()? == title,
            "Song changed during memory read"
        );

        let id = format!("wesing:{:x}", Sha256::digest(serde_json::to_vec(&tracks)?));
        let document = Arc::new(Document {
            id,
            filename: "WeSing memory".into(),
            metadata: Metadata {
                title: title.clone(),
                ..Metadata::default()
            },
            tracks,
            warnings,
        });

        Ok(Some((
            Session {
                source_id: "WeSing.exe".into(),
                title,
                status: status.into(),
                position_ms: position.map(|seconds| seconds * 1000.0),
                duration_ms,
                source_position_ms: position.map(|seconds| seconds * 1000.0),
                rate,
                rate_basis: if rate.is_some() {
                    "memoryInferredNormal"
                } else {
                    "memorySample"
                }
                .into(),
                ..Session::default()
            },
            Some(document),
        )))
    }

    #[cfg(windows)]
    fn reset(&mut self) {
        self.host = None;

        self.timer = None;

        self.timer_candidates.clear();

        self.previous_time = None;

        self.last_scan = None;

        self.last_moved = Instant::now();

        self.duration_addresses.clear();

        self.last_duration_scan = None;
    }
}

#[cfg(windows)]
fn locate_host(process: &memory::Process) -> anyhow::Result<u32> {
    let (base, size) = process.module()?;
    let inside = |address: u32, count: u32| -> anyhow::Result<u32> {
        ensure!(
            address >= base && (address as u64 + count as u64) <= base as u64 + size as u64,
            "Unsupported KSongsLyric PE layout"
        );

        Ok(address)
    };

    ensure!(process.read(base, 2)? == b"MZ", "Invalid DOS header");

    let pe = inside(
        base.checked_add(process.u32(base + 0x3c)?)
            .context("PE overflow")?,
        0x80,
    )?;

    ensure!(
        process.read(pe, 4)? == b"PE\0\0" && process.read(pe + 0x18, 2)? == [0x0b, 0x01],
        "Only PE32 lyrics modules are supported"
    );

    let export = inside(
        base.checked_add(process.u32(pe + 0x78)?)
            .context("Export overflow")?,
        40,
    )?;

    let names = process.u32(export + 0x18)?;
    let functions = process.u32(export + 0x14)?;

    ensure!(names <= 4096 && functions <= 4096, "Invalid export count");

    let names_table = inside(
        base.checked_add(process.u32(export + 0x20)?)
            .context("Names overflow")?,
        names * 4,
    )?;

    let ordinals = inside(
        base.checked_add(process.u32(export + 0x24)?)
            .context("Ordinals overflow")?,
        names * 2,
    )?;

    let function_table = inside(
        base.checked_add(process.u32(export + 0x1c)?)
            .context("Functions overflow")?,
        functions * 4,
    )?;

    let mut create = None;

    for index in 0..names {
        let address = inside(
            base.checked_add(process.u32(names_table + index * 4)?)
                .context("Name overflow")?,
            64,
        )?;

        let name = process.read(address, 64)?;

        if name.starts_with(b"CreateLyricHost\0") {
            let ordinal =
                u16::from_le_bytes(process.read(ordinals + index * 2, 2)?.try_into().unwrap())
                    as u32;

            ensure!(ordinal < functions, "Invalid export ordinal");

            create = Some(inside(
                base.checked_add(process.u32(function_table + ordinal * 4)?)
                    .context("Function overflow")?,
                128,
            )?);
        }
    }

    let create = create.context("CreateLyricHost export not found")?;
    let code = process.read(create, 128)?;
    let constructor = code
        .windows(5)
        .enumerate()
        .find_map(|(offset, bytes)| {
            if bytes[0] != 0xe8 {
                return None;
            }

            let relative = i32::from_le_bytes(bytes[1..5].try_into().unwrap());
            let target = create as i64 + offset as i64 + 5 + relative as i64;

            u32::try_from(target)
                .ok()
                .filter(|address| inside(*address, 200).is_ok())
        })
        .context("Unsupported LyricHost constructor")?;

    let code = process.read(constructor, 200)?;
    let vtable = code
        .windows(6)
        .find_map(|bytes| {
            if bytes[..2] != [0xc7, 0x07] {
                return None;
            }

            let address = u32::from_le_bytes(bytes[2..6].try_into().unwrap());

            inside(address, 4).ok()
        })
        .context("Unsupported LyricHost vtable assignment")?;

    let pattern: Vec<_> = vtable.to_le_bytes().into_iter().map(Some).collect();
    let hits = process.scan(&pattern)?;
    let mut valid = Vec::new();
    let mut issues = Vec::new();
    let mut loading = false;

    for address in hits {
        match read_lines(process, address) {
            Ok(_) => valid.push(address),
            Err(error) => {
                loading |= error.downcast_ref::<Loading>().is_some();

                issues.push(format!("{address:#x}: {error:#}"));
            }
        }
    }

    if valid.is_empty() && loading {
        return Err(Loading.into());
    }

    ensure!(
        valid.len() == 1,
        "Expected one valid LyricHost, found {}; candidates: {}",
        valid.len(),
        issues.join("; ")
    );

    Ok(valid[0])
}

#[cfg(windows)]
trait LyricMemory {
    fn u32(&self, address: u32) -> anyhow::Result<u32>;

    fn read(&self, address: u32, size: usize) -> anyhow::Result<Vec<u8>>;

    fn string(&self, address: u32) -> anyhow::Result<String>;
}

#[cfg(windows)]
impl LyricMemory for memory::Process {
    fn u32(&self, address: u32) -> anyhow::Result<u32> {
        self.u32(address)
    }

    fn read(&self, address: u32, size: usize) -> anyhow::Result<Vec<u8>> {
        self.read(address, size)
    }

    fn string(&self, address: u32) -> anyhow::Result<String> {
        self.string(address)
    }
}

#[cfg(windows)]
fn vector(
    process: &impl LyricMemory,
    begin_at: u32,
    end_at: u32,
    maximum: u32,
) -> anyhow::Result<(u32, u32)> {
    let begin = process.u32(begin_at)?;
    let end = process.u32(end_at)?;
    let count = vector_count(begin, end, maximum).with_context(|| {
        format!("Vector fields {begin_at:#x}/{end_at:#x}, begin={begin:#x}, end={end:#x}")
    })?;

    Ok((begin, count))
}

fn vector_count(begin: u32, end: u32, maximum: u32) -> anyhow::Result<u32> {
    anyhow::ensure!(
        (begin == 0 && end == 0)
            || (begin >= 0x10000
                && end >= begin
                && (end - begin).is_multiple_of(4)
                && (end - begin) / 4 <= maximum),
        "Invalid lyric pointer vector"
    );

    Ok((end - begin) / 4)
}

#[cfg(windows)]
fn read_lines(process: &impl LyricMemory, host: u32) -> anyhow::Result<Vec<Line>> {
    ensure!(host <= u32::MAX - 0x100, "Invalid host pointer");

    read_timeline(process, host + 0x54, host + 0x58, host + 0x5c)
}

#[cfg(windows)]
fn read_timeline(
    process: &impl LyricMemory,
    begin_at: u32,
    end_at: u32,
    capacity_at: u32,
) -> anyhow::Result<Vec<Line>> {
    // std::vector 依次存放 begin、end、capacity，不遍历尚未初始化的容量
    let (begin, count) = vector(process, begin_at, end_at, 1000)?;
    let end = begin + count * 4;
    let capacity = process.u32(capacity_at)?;

    vector_count(begin, capacity, 1000)?;

    ensure!(end <= capacity, "Lyric vector exceeds capacity");

    if count == 0 {
        return Err(Loading.into());
    }

    let pointers = process.read(begin, count as usize * 4)?;
    let mut lines: Vec<Line> = Vec::new();

    for bytes in pointers.as_chunks::<4>().0 {
        let entry = u32::from_le_bytes(*bytes);

        ensure!(entry <= u32::MAX - 0x10, "Invalid line pointer");

        let Some(start_ms) = lyric_timestamp(f32::from_bits(process.u32(entry)?))? else {
            continue;
        };

        let (chars, count) = vector(process, entry + 8, entry + 12, 500)?;
        let mut text = String::new();
        let mut tokens = Vec::new();

        for index in 0..count {
            let character = process.u32(chars + index * 4)?;
            let render = process.u32(character)?;
            let word = process.string(render)?;

            text.push_str(&word);

            if let Some((start, end)) = process
                .u32(character + 4)
                .ok()
                .zip(process.u32(character + 8).ok())
                .and_then(|(start, duration)| {
                    interval(f32::from_bits(start), f32::from_bits(duration))
                })
            {
                tokens.push(Token {
                    text: word,
                    start_ms: start,
                    end_ms: end,
                });
            }
        }

        // 部分单元没有可信时间时整行退回逐行，避免丢字或伪造平均分配时间
        if tokens.len() != count as usize {
            tokens.clear();
        }

        ensure!(!text.contains('\u{fffd}'), "Invalid lyric text");

        lines.push(Line {
            start_ms,
            end_ms: process
                .u32(entry + 4)
                .ok()
                .and_then(|duration| interval(start_ms as f32 / 1000.0, f32::from_bits(duration)))
                .filter(|(start, end)| end > start)
                .map(|(_, end)| end),
            text,
            tokens,
        });
    }

    lines.sort_by_key(|line| line.start_ms);

    for index in 0..lines.len() {
        if lines[index].end_ms.is_some() {
            continue;
        }

        lines[index].end_ms = lines
            .iter()
            .skip(index + 1)
            .find(|line| line.start_ms > lines[index].start_ms)
            .map(|line| line.start_ms);
    }

    if lines.is_empty() {
        return Err(Loading.into());
    }

    ensure!(
        process.u32(begin_at)? == begin && process.u32(end_at)? == end,
        "Lyric model changed during read"
    );

    Ok(lines)
}

#[cfg(windows)]
fn locate_timers(process: &memory::Process) -> anyhow::Result<Vec<(u32, f64)>> {
    let mut candidates = Vec::new();

    for (font, height) in [(30u32, 45u32), (28, 42)] {
        let mut pattern = vec![None; 8];

        pattern.extend(font.to_le_bytes().into_iter().map(Some));

        pattern.extend(height.to_le_bytes().into_iter().map(Some));

        for address in process.scan(&pattern)? {
            if let Ok(time) = process.float(address) {
                candidates.push((address, time));
            }
        }
    }

    candidates.sort_by_key(|candidate| candidate.0);

    candidates.dedup_by_key(|candidate| candidate.0);

    Ok(candidates)
}

fn advancing(previous: f64, current: f64, elapsed: f64) -> bool {
    elapsed >= 0.03
        && previous.is_finite()
        && current.is_finite()
        && (0.7..=1.3).contains(&((current - previous) / elapsed))
}

fn lyric_timestamp(seconds: f32) -> anyhow::Result<Option<i32>> {
    anyhow::ensure!(
        seconds.is_finite() && seconds.abs() < 86400.0,
        "Invalid lyric timestamp"
    );

    // 负时间的前置信息不参与同步，零时间和重叠行都是合法内容
    Ok((seconds >= 0.0).then_some((seconds as f64 * 1000.0).round() as i32))
}

fn interval(start: f32, duration: f32) -> Option<(i32, i32)> {
    let end = start as f64 + duration as f64;

    (start.is_finite() && duration.is_finite() && start >= 0.0 && duration >= 0.0 && end < 86400.0)
        .then_some((
            (start as f64 * 1000.0).round() as i32,
            (end * 1000.0).round() as i32,
        ))
}

fn aligned(original: &[Line], phonetic: &[Line]) -> bool {
    !original.is_empty()
        && original.len() == phonetic.len()
        && original.iter().zip(phonetic).all(|(left, right)| {
            (left.start_ms - right.start_ms).abs() <= 2
                && left
                    .end_ms
                    .zip(right.end_ms)
                    .is_some_and(|(left, right)| (left - right).abs() <= 2)
        })
}

#[cfg(windows)]
fn read_progress_text(process: &memory::Process, delimiter: u32) -> anyhow::Result<(u32, u32)> {
    let bytes = process.read(
        delimiter
            .checked_sub(16)
            .context("Progress text address overflow")?,
        38,
    )?;

    let units: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| u16::from_le_bytes(*bytes))
        .collect();

    let text = String::from_utf16(&units)?;

    parse_progress(&text).context("Invalid progress text")
}

fn parse_progress(text: &str) -> Option<(u32, u32)> {
    let (left, right) = text.split_once(" | ")?;
    let left: String = left
        .chars()
        .rev()
        .take_while(|character| character.is_ascii_digit() || *character == ':')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    let right: String = right
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == ':')
        .collect();

    let current = parse_time(&left)?;
    let duration = parse_time(&right)?;

    (duration > 0 && current <= duration).then_some((current, duration))
}

fn parse_time(text: &str) -> Option<u32> {
    let parts: Vec<_> = text
        .split(':')
        .map(|part| {
            (!part.is_empty() && part.chars().all(|character| character.is_ascii_digit()))
                .then(|| part.parse::<u32>().ok())
                .flatten()
        })
        .collect::<Option<_>>()?;

    let seconds = match parts.as_slice() {
        [minutes, seconds] if *seconds < 60 => minutes.checked_mul(60)?.checked_add(*seconds)?,
        [hours, minutes, seconds] if *minutes < 60 && *seconds < 60 => hours
            .checked_mul(3600)?
            .checked_add(minutes * 60)?
            .checked_add(*seconds)?,
        _ => return None,
    };

    (seconds < 86400).then_some(seconds)
}

fn select_duration(candidates: &[(u32, u32)], position_ms: f64, lyric_end_ms: i32) -> Option<f64> {
    let mut durations: Vec<_> = candidates
        .iter()
        .filter(|(current, duration)| {
            (*current as f64 * 1000.0 - position_ms).abs() <= 1500.0
                && *duration as f64 * 1000.0 >= position_ms
                && *duration as i64 * 1000 + 1000 >= lyric_end_ms as i64
        })
        .map(|(_, duration)| *duration)
        .collect();

    durations.sort_unstable();

    durations.dedup();

    (durations.len() == 1).then(|| durations[0] as f64 * 1000.0)
}

fn matches(bytes: &[u8], pattern: &[Option<u8>]) -> Vec<usize> {
    if pattern.is_empty() || bytes.len() < pattern.len() {
        return vec![];
    }

    let Some((anchor, byte)) = pattern
        .iter()
        .enumerate()
        .find_map(|(index, byte)| byte.map(|byte| (index, byte)))
    else {
        return (0..=bytes.len() - pattern.len()).collect();
    };

    memchr::memchr_iter(byte, bytes)
        .filter_map(|hit| {
            let index = hit.checked_sub(anchor)?;
            let window = bytes.get(index..index + pattern.len())?;

            window
                .iter()
                .zip(pattern)
                .all(|(byte, expected)| expected.is_none_or(|expected| expected == *byte))
                .then_some(index)
        })
        .collect()
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn rejects_corrupt_vectors_and_non_normal_clocks() {
        assert!(vector_count(0xfffffff0, 0x10, 1000).is_err());

        assert!(vector_count(0x10000, 0x10005, 1000).is_err());

        assert!(vector_count(0x10000, 0x20000, 1000).is_err());

        assert_eq!(vector_count(0x10000, 0x10008, 1000).unwrap(), 2);

        assert_eq!(vector_count(0, 0, 1000).unwrap(), 0);

        assert_eq!(vector_count(0x10000, 0x10000, 1000).unwrap(), 0);

        assert!(vector_count(0, 0x10000, 1000).is_err());

        assert!(advancing(1.0, 1.25, 0.25));

        for current in [1.0, 50.0, 0.0, f64::NAN] {
            assert!(!advancing(1.0, current, 0.25));
        }
    }

    #[test]
    fn finds_overlapping_patterns_and_short_buffers() {
        assert_eq!(matches(&[1, 1, 1], &[Some(1), None]), vec![0, 1]);

        assert!(matches(&[1], &[Some(1), None]).is_empty());

        assert!(matches(&[1], &[]).is_empty());
    }

    #[test]
    fn preserves_zero_time_and_rejects_invalid_used_entries() {
        assert_eq!(lyric_timestamp(1.25).unwrap(), Some(1250));

        assert_eq!(lyric_timestamp(0.0).unwrap(), Some(0));

        assert_eq!(lyric_timestamp(-1.0).unwrap(), None);

        for invalid in [f32::NAN, f32::INFINITY, 1e30] {
            assert!(lyric_timestamp(invalid).is_err());
        }
    }

    #[test]
    fn uses_absolute_word_intervals_and_rejects_stale_duration_candidates() {
        assert_eq!(interval(16.920, 0.746), Some((16920, 17666)));

        assert_eq!(interval(0.0, 0.0), Some((0, 0)));

        assert_eq!(interval(1.0, -1.0), None);

        assert_eq!(interval(f32::NAN, 1.0), None);

        assert_eq!(parse_progress("\0\0 1:02 | 3:48\0abc"), Some((62, 228)));

        assert_eq!(parse_progress("1:02:03 | 2:00:00\0"), Some((3723, 7200)));

        assert_eq!(parse_progress("00:65 | 03:48"), None);

        assert_eq!(
            select_duration(&[(62, 228), (62, 228), (10, 500)], 62500.0, 220000),
            Some(228000.0)
        );

        assert_eq!(
            select_duration(&[(62, 228), (62, 250)], 62500.0, 220000),
            None
        );

        assert_eq!(select_duration(&[(10, 228)], 62500.0, 220000), None);

        assert_eq!(select_duration(&[(62, 100)], 62500.0, 220000), None);
    }

    #[cfg(windows)]
    #[test]
    fn reads_used_vector_after_song_switch_and_preserves_empty_gap() {
        use std::collections::BTreeMap;

        struct Fixture(BTreeMap<u32, u8>);

        impl Fixture {
            fn put(&mut self, address: u32, bytes: &[u8]) {
                for (offset, byte) in bytes.iter().enumerate() {
                    self.0.insert(address + offset as u32, *byte);
                }
            }

            fn pointer(&mut self, address: u32, value: u32) {
                self.put(address, &value.to_le_bytes());
            }
        }

        impl LyricMemory for Fixture {
            fn read(&self, address: u32, size: usize) -> anyhow::Result<Vec<u8>> {
                (0..size)
                    .map(|offset| {
                        self.0
                            .get(&(address + offset as u32))
                            .copied()
                            .context("Uninitialized capacity or invalid pointer was read")
                    })
                    .collect()
            }

            fn u32(&self, address: u32) -> anyhow::Result<u32> {
                Ok(u32::from_le_bytes(
                    self.read(address, 4)?.try_into().unwrap(),
                ))
            }

            fn string(&self, address: u32) -> anyhow::Result<String> {
                let mut units = Vec::new();

                for offset in 0..128 {
                    let unit =
                        u16::from_le_bytes(self.read(address + offset * 2, 2)?.try_into().unwrap());

                    if unit == 0 {
                        return Ok(String::from_utf16(&units)?);
                    }

                    units.push(unit);
                }

                anyhow::bail!("Unterminated fixture text")
            }
        }

        let host = 0x10000;
        let mut fixture = Fixture(BTreeMap::new());

        fixture.pointer(host + 0x54, 0x20000);

        fixture.pointer(host + 0x58, 0x20008);

        fixture.pointer(host + 0x5c, 0x2000c);

        fixture.pointer(0x20000, 0x30000);

        fixture.pointer(0x20004, 0x31000);

        // 切歌后 capacity 中残留的第三项不可读，不能将其视作有效歌词
        fixture.pointer(0x20008, 0xbde600b5);

        fixture.put(0x30000, &0.0f32.to_le_bytes());

        fixture.pointer(0x30008, 0);

        fixture.pointer(0x3000c, 0);

        fixture.put(0x31000, &1.25f32.to_le_bytes());

        fixture.put(0x31004, &1.5f32.to_le_bytes());

        fixture.pointer(0x31008, 0x40000);

        fixture.pointer(0x3100c, 0x40004);

        fixture.pointer(0x40000, 0x50000);

        fixture.pointer(0x50000, 0x60000);

        fixture.put(0x50004, &1.25f32.to_le_bytes());

        fixture.put(0x50008, &0.33f32.to_le_bytes());

        let text: Vec<_> = "测试🎵\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();

        fixture.put(0x60000, &text);

        let lines = read_lines(&fixture, host).unwrap();

        assert_eq!(lines.len(), 2);

        assert_eq!(lines[0].text, "");

        assert_eq!(lines[0].end_ms, Some(1250));

        assert_eq!(lines[1].text, "测试🎵");

        assert_eq!(lines[1].start_ms, 1250);

        assert_eq!(lines[1].end_ms, Some(2750));

        assert_eq!(lines[1].tokens[0].start_ms, 1250);

        assert_eq!(lines[1].tokens[0].end_ms, 1580);

        assert!(aligned(&lines, &lines));

        let mut stale = lines.clone();

        stale[1].start_ms += 1000;

        assert!(!aligned(&lines, &stale));

        fixture.put(0x50008, &f32::NAN.to_le_bytes());

        let fallback = read_lines(&fixture, host).unwrap();

        assert_eq!(fallback[1].text, "测试🎵");

        assert!(fallback[1].tokens.is_empty());

        fixture.pointer(host + 0x58, 0x20000);

        assert!(
            read_lines(&fixture, host)
                .unwrap_err()
                .downcast_ref::<Loading>()
                .is_some()
        );
    }
}
