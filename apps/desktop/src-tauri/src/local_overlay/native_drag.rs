//! overlay 窗口的 OS 级右键 drag
//!
//! 透明 WebView 可能吞掉或无法投递 DOM pointer event，嵌套 plugin frame 还会让
//! 情况更糟，所以 drag 改为从桌面侧检测：[`tick`] 用 `GetAsyncKeyState` 采样右键、
//! 用 `GetCursorPos` 采样 cursor，然后在按键保持期间移动窗口；这里的坐标全是
//! **physical pixel**，调用方只在持久化 geometry 时才换算成 logical pixel
//!
//! [`watch`] 从 async runtime 驱动采样器，但每次采样都通过 `run_on_main_thread`
//! 在主线程执行，因为它要读写 [`DesktopState`] 和窗口；overlay 隐藏、启用
//! click-through 或应用正在关闭时停止采样

use super::{LABEL, SharedDesktop};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};
use windows::Win32::{
    Foundation::POINT,
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_RBUTTON},
        WindowsAndMessaging::GetCursorPos,
    },
};

/// 从 OS 采样得到的右键 drag 状态
///
/// 由 [`DesktopState`] 持有，只在主线程访问；它跨 tick 保留，使一次按下和后续
/// 移动构成同一次 drag
#[derive(Default)]
pub(super) struct RightDrag {
    /// 上一次采样的按键状态，用于检测按下边沿
    was_down: bool,

    /// 按下时的 cursor 与窗口原点，单位为 physical pixel；没有 drag 时是 `None`
    origin: Option<(PhysicalPosition<i32>, PhysicalPosition<i32>)>,
}

/// 一次采样的结果：窗口应移动到的位置，以及 drag 是否结束
struct DragStep {
    position: PhysicalPosition<i32>,

    finished: bool,
}

impl RightDrag {
    /// 丢弃正在进行的 drag，例如设置在其下方发生变化之后
    pub(super) fn cancel(&mut self) {
        self.origin = None;
    }

    /// 把一次采样送入 drag 状态机
    ///
    /// `down` 是当前右键状态，`enabled` 表示当前是否允许 drag，`cursor` 是指针
    /// 位置，`position`/`size` 是窗口外框，单位为 physical pixel；没有进行中的
    /// drag 或 drag 刚被禁用时返回 `None`，否则返回窗口应取的位置以及本次采样
    /// 是否释放了按键
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

/// 启动轮询循环直到应用关闭
///
/// 每轮迭代跳到主线程采样鼠标并移动窗口，然后按 [`tick`] 报告的延迟休眠：
/// overlay 显示时约 60 Hz，隐藏时 100 ms；app handle 消失、状态正在关闭或窗口
/// 不见时循环结束
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

/// 采样一次鼠标并应用得到的窗口移动
///
/// 在持有状态锁的情况下于主线程运行；返回距下一次采样的毫秒延迟，应用正在关闭
/// 或 overlay 窗口已不存在时返回 `None` 表示停止轮询
fn tick(app: &AppHandle) -> anyhow::Result<Option<u64>> {
    let shared = app.state::<SharedDesktop>();
    // WebView2 创建期间会重入 UI event loop，跳过持锁中的 tick，避免主线程等待自身
    let mut state = match shared.try_lock() {
        Ok(state) => state,
        Err(std::sync::TryLockError::WouldBlock) => return Ok(Some(100)),
        Err(error) => return Err(anyhow::anyhow!(error.to_string())),
    };

    if state.closing {
        return Ok(None);
    }

    // 直接读取桌面坐标：透明 WebView 和嵌套 plugin frame 不必投递 DOM pointer
    // event 也能完成拖动
    // SAFETY: `GetAsyncKeyState` 只查询全局按键状态，不接收指针，在任何 thread
    // 上都是安全的；这里只使用符号位
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

    // SAFETY: `GetCursorPos` 写入 `cursor`，这是调用期间存活的栈上 `POINT`，且
    // 调用运行在 UI thread 上；返回状态由 `?` 转成错误
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
