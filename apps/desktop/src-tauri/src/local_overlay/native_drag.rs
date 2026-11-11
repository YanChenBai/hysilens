use super::{LABEL, SharedDesktop};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};
use windows::Win32::{
    Foundation::POINT,
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_RBUTTON},
        WindowsAndMessaging::GetCursorPos,
    },
};

#[derive(Default)]
pub(super) struct RightDrag {
    was_down: bool,
    origin: Option<(PhysicalPosition<i32>, PhysicalPosition<i32>)>,
}

struct DragStep {
    position: PhysicalPosition<i32>,
    finished: bool,
}

impl RightDrag {
    pub(super) fn cancel(&mut self) {
        self.origin = None;
    }

    fn sample(
        &mut self,
        down: bool,
        enabled: bool,
        cursor: PhysicalPosition<i32>,
        position: PhysicalPosition<i32>,
        size: PhysicalSize<u32>,
    ) -> Option<DragStep> {
        let pressed = down && !self.was_down;
        self.was_down = down;
        if !enabled {
            self.cancel();
            return None;
        }
        let inside = cursor.x >= position.x
            && cursor.y >= position.y
            && i64::from(cursor.x) < i64::from(position.x) + i64::from(size.width)
            && i64::from(cursor.y) < i64::from(position.y) + i64::from(size.height);
        if pressed && inside {
            self.origin = Some((cursor, position));
        }
        let (start, origin) = self.origin?;
        let offset = |origin: i32, current: i32, start: i32| {
            (i64::from(origin) + i64::from(current) - i64::from(start))
                .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
        };
        let next = PhysicalPosition::new(
            offset(origin.x, cursor.x, start.x),
            offset(origin.y, cursor.y, start.y),
        );
        if !down {
            self.cancel();
        }
        Some(DragStep {
            position: next,
            finished: !down,
        })
    }
}

pub(super) fn watch(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let (reply, result) = tokio::sync::oneshot::channel();
            let ui = app.clone();
            if app
                .run_on_main_thread(move || {
                    let _ = reply.send(tick(&ui));
                })
                .is_err()
            {
                break;
            }
            let delay = match result.await {
                Ok(Ok(Some(delay))) => delay,
                Ok(Ok(None)) | Err(_) => break,
                Ok(Err(error)) => {
                    eprintln!("悬浮窗拖动失败: {error}");
                    break;
                }
            };
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        }
    });
}

fn tick(app: &AppHandle) -> anyhow::Result<Option<u64>> {
    let shared = app.state::<SharedDesktop>();
    let mut state = shared
        .lock()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if state.closing {
        return Ok(None);
    }
    // Read desktop coordinates directly: transparent WebView pixels and nested
    // plugin frames need not deliver DOM pointer events for dragging to work.
    // SAFETY: this only queries the current button state; it takes no pointers.
    let down = unsafe { GetAsyncKeyState(i32::from(VK_RBUTTON.0)) < 0 };
    let enabled = state.settings.enabled && !state.settings.preferences.click_through;
    if !enabled {
        state.native_drag.sample(
            down,
            false,
            PhysicalPosition::new(0, 0),
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(0, 0),
        );
        return Ok(Some(100));
    }
    let Some(window) = app.get_webview_window(LABEL) else {
        return Ok(Some(100));
    };
    let mut cursor = POINT::default();
    // SAFETY: GetCursorPos writes to our initialized, valid POINT on the UI thread.
    unsafe {
        GetCursorPos(&mut cursor)?;
    }
    let position = window.outer_position()?;
    let size = window.outer_size()?;
    if let Some(step) = state.native_drag.sample(
        down,
        true,
        PhysicalPosition::new(cursor.x, cursor.y),
        position,
        size,
    ) {
        if position != step.position {
            window.set_position(step.position)?;
            let logical = step.position.to_logical::<i32>(window.scale_factor()?);
            state.settings.preferences.geometry.x = logical.x;
            state.settings.preferences.geometry.y = logical.y;
        }
        if step.finished {
            state.settings.preferences.save()?;
        }
    }
    Ok(Some(16))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(drag: &mut RightDrag, down: bool, enabled: bool, x: i32, y: i32) -> Option<DragStep> {
        drag.sample(
            down,
            enabled,
            PhysicalPosition::new(x, y),
            PhysicalPosition::new(-100, 50),
            PhysicalSize::new(960, 320),
        )
    }

    #[test]
    fn tracks_right_drag_outside_the_window_and_finishes_on_release() {
        let mut drag = RightDrag::default();
        sample(&mut drag, true, true, 0, 100);
        let movement = sample(&mut drag, true, true, -500, -200).unwrap();
        assert_eq!(movement.position, PhysicalPosition::new(-600, -250));
        assert!(!movement.finished);
        let released = sample(&mut drag, false, true, -510, -210).unwrap();
        assert_eq!(released.position, PhysicalPosition::new(-610, -260));
        assert!(released.finished);
        assert!(sample(&mut drag, false, true, -510, -210).is_none());
    }

    #[test]
    fn entering_window_with_button_already_held_does_not_start_drag() {
        let mut drag = RightDrag::default();
        assert!(sample(&mut drag, true, true, -500, -200).is_none());
        assert!(sample(&mut drag, true, true, 0, 100).is_none());
    }

    #[test]
    fn enabling_click_through_cancels_drag_until_a_fresh_press() {
        let mut drag = RightDrag::default();
        sample(&mut drag, true, true, 0, 100);
        assert!(sample(&mut drag, true, false, 50, 120).is_none());
        assert!(sample(&mut drag, true, true, 50, 120).is_none());
        sample(&mut drag, false, true, 50, 120);
        assert!(sample(&mut drag, true, true, 50, 120).is_some());
    }
}
