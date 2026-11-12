//! 对 platform adapter 的后台采样，向 coordinator 推送事件

use crate::{
    model::Playback,
    platforms::{AdapterState, CatalogUpdate, Context, Registration},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

/// capture thread 产生的事件
pub enum Event {
    /// 一轮采样：每个 adapter 的 playback 及其内嵌的歌词
    Playback(Playback, Vec<AdapterState>),

    /// 某个平台的 cache catalog 被重新扫描
    Catalog(CatalogUpdate),
}

/// 开始采样已注册的 adapter 并返回 capture 事件流
///
/// 每个 registration 的 factory 在 capture thread 上运行一次；随后 adapter 每
/// 250 ms 被采样一次，直到 `stop` 被置位；adapter 一上报 catalog 更新就立即转发
/// 当 `stop` 被置位或返回的 receiver 被丢弃时，capture 结束
pub fn spawn(
    origin: Instant,
    registrations: Vec<Registration>,
    stop: Arc<AtomicBool>,
) -> mpsc::Receiver<Event> {
    let (sender, receiver) = mpsc::channel(32);
    let (catalogs, mut updates) = mpsc::channel(32);
    let catalog_events = sender.clone();

    tokio::spawn(async move {
        while let Some(update) = updates.recv().await {
            if catalog_events.send(Event::Catalog(update)).await.is_err() {
                break;
            }
        }
    });

    std::thread::spawn(move || {
        let mut adapters: Vec<_> = registrations
            .into_iter()
            .map(|registration| {
                let adapter = (registration.create)(Context {
                    origin,
                    catalogs: catalogs.clone(),
                    stop: stop.clone(),
                });

                (registration.descriptor, adapter)
            })
            .collect();

        while !stop.load(Ordering::Relaxed) {
            // 把每个 adapter 的每轮结果合并成一个 playback 采样
            let mut playback = Playback::default();
            let mut states = Vec::new();
            let mut errors = Vec::new();

            for (descriptor, adapter) in &mut adapters {
                let sample = adapter.sample();

                playback.sessions.extend(sample.playback.sessions);

                if let Some(error) = sample.playback.error {
                    errors.push(format!("{}: {error}", descriptor.session_id));
                }

                states.push(AdapterState {
                    descriptor: *descriptor,
                    document: sample.document,
                });
            }

            playback.server_ms = origin.elapsed().as_secs_f64() * 1000.0;

            playback.error = (!errors.is_empty()).then(|| errors.join("; "));

            if sender
                .blocking_send(Event::Playback(playback, states))
                .is_err()
            {
                break;
            }

            std::thread::sleep(Duration::from_millis(250));
        }
    });

    receiver
}
