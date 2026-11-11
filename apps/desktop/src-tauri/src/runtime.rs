use hysilens_core::{
    config::Settings,
    server::{self, RuntimeOptions},
};
use tokio::sync::{mpsc, watch};

fn start(
    listener: tokio::net::TcpListener,
    settings: Settings,
    restart: mpsc::Sender<Settings>,
) -> tokio::task::JoinHandle<anyhow::Result<()>> {
    tokio::spawn(server::serve_on(
        listener,
        RuntimeOptions { settings, restart },
        std::future::pending(),
    ))
}

pub async fn supervise(
    listener: tokio::net::TcpListener,
    mut settings: Settings,
    restart: mpsc::Sender<Settings>,
    mut requests: mpsc::Receiver<Settings>,
    urls: watch::Sender<String>,
) {
    let mut task = start(listener, settings.clone(), restart.clone());
    loop {
        let next = tokio::select! {
            next = requests.recv() => { let Some(next) = next else { task.abort(); break; }; next },
            result = &mut task => { eprintln!("本机服务已停止: {result:?}"); break; },
        };
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        if let Err(error) = next.save() {
            eprintln!("保存设置失败: {error}");
            continue;
        }
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
        task = start(listener, settings.clone(), restart.clone());
    }
}
