//! 让内嵌的 HTTP server 在桌面 host 的整个生命周期内保持运行
//!
//! [`supervise`] 由 `main` spawn 一次，是唯一启动、重启或放弃 server task 的地方：
//!
//! - 重启请求从 `mpsc` channel 到达，携带完整的新设置
//! - 新设置先持久化再 drop 旧 listener，因此写入失败不会影响运行中的 server
//! - 重新绑定成功后，新的 control URL 通过 `watch` channel 发布，`main` 用它
//!   重新指向两个 WebView 窗口

use hysilens_core::{
    config::Settings,
    server::{self, RuntimeOptions, control::ControlHandle},
};
use tokio::sync::{mpsc, watch};

/// 在 `listener` 上启动一个 server 实例
///
/// `restart` 交给核心服务，使通过 IPC 做出的设置变更能到达 [`supervise`]
/// 关闭不是 graceful 的：调用方通过 abort 返回的 task 来停止实例，这也是
/// shutdown future 始终 pending 的原因
fn start(
    listener: tokio::net::TcpListener,
    settings: Settings,
    restart: mpsc::Sender<Settings>,
    control: watch::Sender<Option<ControlHandle>>,
) -> tokio::task::JoinHandle<anyhow::Result<()>> {
    tokio::spawn(server::serve_on(
        listener,
        RuntimeOptions {
            settings,
            restart,
            control,
        },
        std::future::pending(),
    ))
}

/// 运行 server 并应用重启请求，直到进程关闭
///
/// `requests` 是作为 `restart` 交给 server 的 channel 的接收端，`urls` 在每次
/// 重启成功后发布新绑定 server 的 `/control` URL
///
/// 所有 restart sender 都被 drop、重启时新端口与旧端口都绑定失败，或 server
/// task 自身停止时循环结束；调用方 abort 整个 task 来停止当前 server
pub async fn supervise(
    listener: tokio::net::TcpListener,
    mut settings: Settings,
    restart: mpsc::Sender<Settings>,
    mut requests: mpsc::Receiver<Settings>,
    urls: watch::Sender<String>,
    control: watch::Sender<Option<ControlHandle>>,
) {
    let mut task = start(listener, settings.clone(), restart.clone(), control.clone());

    loop {
        let next = tokio::select! {
            next = requests.recv() => {
                let Some(next) = next else {
                    control.send_replace(None);
                    task.abort();
                    break;
                };
                next
            }
            result = &mut task => {
                control.send_replace(None);
                eprintln!("本机服务已停止: {result:?}");
                break;
            }
        };

        // 让触发重启的请求先写完响应，之后再 drop listener
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        if let Err(error) = next.save() {
            eprintln!("保存设置失败: {error}");
            continue;
        }

        control.send_replace(None);
        task.abort();

        let _ = task.await;
        let listener =
            match tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, next.port)).await {
                Ok(listener) => {
                    settings = next;
                    urls.send_replace(format!("http://127.0.0.1:{}/control", settings.port));
                    listener
                }
                Err(error) => {
                    eprintln!("服务重启失败: {error}");

                    let _ = settings.save();

                    match tokio::net::TcpListener::bind((
                        std::net::Ipv4Addr::LOCALHOST,
                        settings.port,
                    ))
                    .await
                    {
                        Ok(previous) => previous,
                        Err(error) => {
                            eprintln!("服务恢复失败: {error}");
                            break;
                        }
                    }
                }
            };

        task = start(listener, settings.clone(), restart.clone(), control.clone());
    }
}
