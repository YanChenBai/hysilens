//! 主控制窗口的 IPC commands、二进制安装与可取消的状态 channel

use hysilens_core::server::control::{ControlHandle, ControlRequest};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};
use tauri::{
    Manager, WebviewWindow,
    ipc::{Channel, InvokeBody, Request},
};
use tokio::{sync::watch, task::AbortHandle};

/// 当前服务句柄和主窗口的唯一订阅；句柄随 supervisor 重启更新
pub struct ControlIpc {
    services: watch::Receiver<Option<ControlHandle>>,

    subscription: Mutex<Option<(u64, AbortHandle)>>,

    next_id: AtomicU64,
}

impl ControlIpc {
    /// 创建尚可等待核心服务就绪的 IPC 状态
    pub fn new(services: watch::Receiver<Option<ControlHandle>>) -> Self {
        Self {
            services,
            subscription: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    fn handle(&self) -> Result<ControlHandle, String> {
        self.services
            .borrow()
            .clone()
            .ok_or_else(|| "核心服务尚未就绪，请稍后重试".into())
    }

    /// 释放匹配 id 的订阅；窗口销毁时传入 None 释放所有订阅
    ///
    /// # Errors
    /// 订阅锁损坏时返回错误
    pub fn cancel(&self, id: Option<u64>) -> Result<(), String> {
        let mut subscription = self
            .subscription
            .lock()
            .map_err(|error| error.to_string())?;

        if subscription
            .as_ref()
            .is_some_and(|(current, _)| id.is_none_or(|id| *current == id))
            && let Some((_, task)) = subscription.take()
        {
            task.abort();
        }

        Ok(())
    }
}

impl Drop for ControlIpc {
    fn drop(&mut self) {
        if let Ok(subscription) = self.subscription.get_mut()
            && let Some((_, task)) = subscription.take()
        {
            task.abort();
        }
    }
}

/// 只允许当前主控制窗口调用白名单中的核心操作
///
/// # Errors
/// 权限、服务就绪或核心操作失败时返回错误
#[tauri::command]
pub async fn control_request(
    window: WebviewWindow,
    request: ControlRequest,
) -> Result<Value, String> {
    crate::local_overlay::authorize_control(&window).map_err(|error| error.to_string())?;
    let service = window.state::<ControlIpc>().handle()?;

    service
        .request(request)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// 直接接收 ZIP 二进制；不把归档展开成 JSON 数字数组
///
/// # Errors
/// 权限、请求格式、归档大小或安装校验失败时返回错误
#[tauri::command]
pub async fn control_install_plugin(
    window: WebviewWindow,
    request: Request<'_>,
) -> Result<Value, String> {
    crate::local_overlay::authorize_control(&window).map_err(|error| error.to_string())?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("安装插件需要 ZIP 二进制请求".into());
    };

    if bytes.len() > hysilens_core::plugins::MAX_PACKAGE_BYTES {
        return Err("插件包过大".into());
    }

    let bytes = bytes.clone();
    let service = window.state::<ControlIpc>().handle()?;

    service
        .install(bytes)
        .await
        .map_err(|error| format!("{error:#}"))
}

/// 订阅核心状态；同一主窗口再次订阅会释放旧任务
///
/// # Errors
/// 调用窗口未授权或订阅锁损坏时返回错误
#[tauri::command]
pub async fn control_subscribe(
    window: WebviewWindow,
    channel: Channel<Value>,
) -> Result<u64, String> {
    crate::local_overlay::authorize_control(&window).map_err(|error| error.to_string())?;
    let state = window.state::<ControlIpc>();
    let mut subscription = state
        .subscription
        .lock()
        .map_err(|error| error.to_string())?;

    if let Some((_, previous)) = subscription.take() {
        previous.abort();
    }

    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let services = state.services.clone();
    let task = tauri::async_runtime::spawn(forward(services, channel));
    *subscription = Some((id, task.inner().abort_handle()));

    Ok(id)
}

/// 释放指定订阅；旧页面的清理不会取消新页面的订阅
///
/// # Errors
/// 调用窗口未授权或订阅锁损坏时返回错误
#[tauri::command]
pub async fn control_unsubscribe(window: WebviewWindow, id: u64) -> Result<(), String> {
    crate::local_overlay::authorize_control(&window).map_err(|error| error.to_string())?;
    window.state::<ControlIpc>().cancel(Some(id))
}

async fn forward(mut services: watch::Receiver<Option<ControlHandle>>, channel: Channel<Value>) {
    loop {
        let service = services.borrow_and_update().clone();

        if channel
            .send(json!({ "kind": "connection", "connected": service.is_some() }))
            .is_err()
        {
            return;
        }

        if let Some(service) = service {
            if channel.send(json!({ "kind": "resync" })).is_err() {
                return;
            }

            tokio::select! {
                changed = services.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
                result = service.forward(|value| channel.send(value).is_ok()) => {
                    if let Err(error) = result {
                        let _ = channel.send(json!({ "kind": "error", "message": error.to_string() }));
                    }
                    if channel.send(json!({ "kind": "connection", "connected": false })).is_err()
                        || services.changed().await.is_err()
                    {
                        return;
                    }
                }
            }
        } else if services.changed().await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stale_cleanup_cannot_cancel_a_newer_subscription() {
        let (_services, receiver) = watch::channel(None);
        let state = ControlIpc::new(receiver);
        let task = tokio::spawn(std::future::pending::<()>());
        *state.subscription.lock().unwrap() = Some((2, task.abort_handle()));

        state.cancel(Some(1)).unwrap();
        assert!(state.subscription.lock().unwrap().is_some());
        state.cancel(Some(2)).unwrap();

        assert!(task.await.unwrap_err().is_cancelled());
        assert!(state.subscription.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn window_destruction_releases_the_current_subscription() {
        let (_services, receiver) = watch::channel(None);
        let state = ControlIpc::new(receiver);
        let task = tokio::spawn(std::future::pending::<()>());
        *state.subscription.lock().unwrap() = Some((5, task.abort_handle()));

        state.cancel(None).unwrap();

        assert!(task.await.unwrap_err().is_cancelled());
        assert!(state.subscription.lock().unwrap().is_none());
    }
}
