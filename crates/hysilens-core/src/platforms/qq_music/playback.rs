use crate::model::{Playback, Session};
use std::{
    cell::RefCell,
    collections::HashMap,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub struct Probe {
    origin: Instant,
    previous: RefCell<HashMap<String, Session>>,
    #[cfg(windows)]
    manager: windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager,
}

impl Probe {
    pub fn new(origin: Instant) -> anyhow::Result<Self> {
        #[cfg(windows)]
        {
            use windows::{
                Media::Control::GlobalSystemMediaTransportControlsSessionManager,
                Win32::System::Com::*,
            };

            // 探针始终在同一个工作线程使用 WinRT 对象。
            unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            }

            let manager = match GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .and_then(|request| request.join())
            {
                Ok(manager) => manager,
                Err(error) => {
                    // Balance successful initialization even when no probe is returned.
                    unsafe { CoUninitialize() };
                    return Err(error.into());
                }
            };

            Ok(Self {
                origin,
                manager,
                previous: RefCell::new(HashMap::new()),
            })
        }

        #[cfg(not(windows))]
        {
            let _ = origin;

            anyhow::bail!("Playback probe currently supports Windows only")
        }
    }

    pub fn sample(&self) -> Playback {
        let mut playback = Playback {
            server_ms: self.origin.elapsed().as_secs_f64() * 1000.0,
            ..Playback::default()
        };

        #[cfg(windows)]
        match self.read_sessions() {
            Ok(sessions) => playback.sessions = sessions,
            Err(error) => playback.error = Some(error.to_string()),
        }

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|time| time.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);

        let mut previous = self.previous.borrow_mut();

        for session in &mut playback.sessions {
            infer_rate(session, previous.get(&session.source_id));

            if session.status == "playing"
                && let (Some(position), Some(updated), Some(rate), Some(duration)) = (
                    session.source_position_ms,
                    session.source_updated_unix_ms,
                    session.rate,
                    session.duration_ms,
                )
            {
                session.position_ms =
                    Some((position + (now_ms - updated).max(0.0) * rate).clamp(0.0, duration));
            }

            previous.insert(session.source_id.clone(), session.clone());
        }

        previous.retain(|key, _| {
            playback
                .sessions
                .iter()
                .any(|session| &session.source_id == key)
        });

        playback.server_ms = self.origin.elapsed().as_secs_f64() * 1000.0;

        playback
    }

    #[cfg(windows)]
    fn read_sessions(&self) -> anyhow::Result<Vec<Session>> {
        let mut result = Vec::new();

        for session in self.manager.GetSessions()? {
            let source_id = session.SourceAppUserModelId()?.to_string();

            match read_session(&session, source_id.clone()) {
                Ok(value) => result.push(value),
                Err(error) => result.push(Session {
                    source_id,
                    error: Some(error.to_string()),
                    ..Session::default()
                }),
            }
        }

        Ok(result)
    }
}

#[cfg(windows)]
fn read_session(
    session: &windows::Media::Control::GlobalSystemMediaTransportControlsSession,
    source_id: String,
) -> anyhow::Result<Session> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status;

    let media = session.TryGetMediaPropertiesAsync()?.join()?;

    let playback = session.GetPlaybackInfo()?;

    let status = playback.PlaybackStatus()?;

    let timeline = session.GetTimelineProperties()?;

    let source_position_ms = timeline.Position()?.Duration as f64 / 10_000.0;

    let duration_ms =
        (timeline.EndTime()?.Duration - timeline.StartTime()?.Duration) as f64 / 10_000.0;

    let updated = timeline.LastUpdatedTime()?.UniversalTime;

    let updated_unix_ms = updated as f64 / 10_000.0 - 11_644_473_600_000.0;

    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64() * 1000.0;

    let rate = playback
        .PlaybackRate()
        .ok()
        .and_then(|value| value.Value().ok());

    let age_ms = now_ms - updated_unix_ms;

    // 实测 QQ 每约 330–550 ms 更新时间轴，三秒未更新时停止外推。
    let is_fresh = status != Status::Playing
        || !source_id.eq_ignore_ascii_case("QQMusic.exe")
        || age_ms <= 3000.0;

    let has_timeline = updated > 0
        && duration_ms > 0.0
        && age_ms >= -1000.0
        && source_position_ms >= 0.0
        && is_fresh;

    let position_ms = has_timeline.then_some(source_position_ms.clamp(0.0, duration_ms.max(0.0)));

    Ok(Session {
        source_id,
        title: media.Title()?.to_string(),
        artist: media.Artist()?.to_string(),
        album: media.AlbumTitle()?.to_string(),
        status: match status {
            Status::Playing => "playing",
            Status::Paused => "paused",
            Status::Stopped => "stopped",
            Status::Closed => "closed",
            Status::Opened => "opened",
            _ => "unknown",
        }
        .into(),
        position_ms,
        duration_ms: (duration_ms > 0.0).then_some(duration_ms),
        source_position_ms: has_timeline.then_some(source_position_ms),
        source_updated_unix_ms: (updated > 0).then_some(updated_unix_ms),
        rate,
        rate_basis: if rate.is_some() {
            "reported"
        } else {
            "unavailable"
        }
        .into(),
        error: None,
        ..Session::default()
    })
}

fn infer_rate(session: &mut Session, previous: Option<&Session>) {
    if session.rate.is_some() || session.status != "playing" {
        return;
    }

    let Some(previous) = previous else {
        return;
    };

    if previous.status != "playing"
        || previous.title != session.title
        || previous.artist != session.artist
        || previous.album != session.album
    {
        return;
    }

    let (Some(position), Some(updated), Some(old_position), Some(old_updated)) = (
        session.source_position_ms,
        session.source_updated_unix_ms,
        previous.source_position_ms,
        previous.source_updated_unix_ms,
    ) else {
        return;
    };

    let elapsed = updated - old_updated;

    if elapsed == 0.0 && position == old_position && previous.rate_basis == "inferredNormal" {
        session.rate = previous.rate;

        session.rate_basis = previous.rate_basis.clone();

        return;
    }

    // 只确认连续正常速度；拖动或未知倍速时保留原始采样，不猜测时钟。
    if elapsed >= 50.0 && (0.8..=1.2).contains(&((position - old_position) / elapsed)) {
        session.rate = Some(1.0);

        session.rate_basis = "inferredNormal".into();
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn infers_normal_speed_but_rejects_seek_and_track_change() {
        let previous = Session {
            status: "playing".into(),
            title: "A".into(),
            source_position_ms: Some(1000.0),
            source_updated_unix_ms: Some(2000.0),
            ..Session::default()
        };

        let mut next = Session {
            source_position_ms: Some(1500.0),
            source_updated_unix_ms: Some(2500.0),
            ..previous.clone()
        };

        infer_rate(&mut next, Some(&previous));

        assert_eq!(next.rate, Some(1.0));

        next.rate = None;

        next.source_position_ms = Some(50_000.0);

        infer_rate(&mut next, Some(&previous));

        assert_eq!(next.rate, None);

        next.source_position_ms = Some(1500.0);

        next.title = "B".into();

        infer_rate(&mut next, Some(&previous));

        assert_eq!(next.rate, None);
    }
}
