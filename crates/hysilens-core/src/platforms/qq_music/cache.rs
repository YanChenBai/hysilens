//! 监视 QQ Music 歌词 cache 并重新发布其 catalog
//!
//! watcher 通过监视最近的已存在祖先目录来跟随被重建的 cache 目录，并在目录出现后重新注册
//! 到该目录上；扫描经过 debounce，未完成的 catalog 会重试若干次，因为文件可能正在被写入
//! 和解码

use super::{DESCRIPTOR, catalog};
use crate::model::{Catalog, Source};
use crate::platforms::{CatalogUpdate, Context};
use anyhow::Context as _;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::{Path, PathBuf},
    sync::{atomic::Ordering, mpsc},
    time::{Duration, Instant},
};

const DEBOUNCE: Duration = Duration::from_millis(250);
const STOP_CHECK: Duration = Duration::from_millis(250);
const WATCH_RETRY: Duration = Duration::from_secs(5);
const READ_RETRIES: u32 = 3;

fn publish(context: &Context, catalog: catalog::Catalog, source: Source) -> bool {
    let mut update = CatalogUpdate { source, catalog };

    while !context.stop.load(Ordering::Relaxed) {
        match context.catalogs.try_send(update) {
            Ok(()) => return true,
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => return false,
            Err(tokio::sync::mpsc::error::TrySendError::Full(pending)) => {
                update = pending;
                std::thread::sleep(STOP_CHECK);
            }
        }
    }

    false
}

fn watch(
    path: &Path,
    sender: mpsc::Sender<notify::Result<Event>>,
) -> anyhow::Result<(RecommendedWatcher, PathBuf)> {
    let mut watcher = notify::recommended_watcher(sender)?;
    let parent = path.ancestors().skip(1).find(|ancestor| ancestor.is_dir());

    // 父目录负责发现缓存目录的删除和重建；目录尚不存在时逐层跟随创建事件
    if let Some(parent) = parent {
        watcher.watch(parent, RecursiveMode::NonRecursive)?;
    }

    anyhow::ensure!(
        parent.is_some() || path.is_dir(),
        "缓存目录没有可监听的父目录"
    );

    let anchor = if path.is_dir() {
        watcher.watch(path, RecursiveMode::NonRecursive)?;
        path
    } else {
        parent.context("缓存目录没有可监听的父目录")?
    };

    Ok((watcher, anchor.to_path_buf()))
}

fn affects_cache_with_extension(event: &Event, path: &Path, extension: &str) -> bool {
    if event.need_rescan() {
        return true;
    }

    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }

    event.paths.iter().any(|changed| {
        (path.starts_with(changed)
            && !(changed == path
                && path.is_dir()
                && matches!(
                    event.kind,
                    EventKind::Modify(
                        notify::event::ModifyKind::Any | notify::event::ModifyKind::Metadata(_)
                    )
                )))
            || (changed.parent() == Some(path)
                && changed
                    .extension()
                    .is_some_and(|value| value.eq_ignore_ascii_case(extension)))
    })
}

#[cfg(test)]
fn affects_cache(event: &Event, path: &Path) -> bool {
    affects_cache_with_extension(event, path, "qrc")
}

pub(super) fn spawn(path: PathBuf, context: Context) -> std::thread::JoinHandle<()> {
    spawn_for(
        path,
        context,
        DESCRIPTOR.source,
        "qrc",
        catalog::Catalog::read,
    )
}

pub(crate) fn spawn_for(
    path: PathBuf,
    context: Context,
    source: Source,
    extension: &'static str,
    read: fn(&Path) -> anyhow::Result<Catalog>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let path = match std::path::absolute(path) {
            Ok(path) => path,
            Err(error) => {
                publish(
                    &context,
                    catalog::Catalog {
                        issues: vec![format!("Cache: {error:#}")],
                        ..Default::default()
                    },
                    source,
                );

                return;
            }
        };

        let (sender, receiver) = mpsc::channel();

        // 监视与扫描状态；每个 deadline 都是 `Instant`，循环可以直接在它上面休眠
        let mut watcher = None;
        let mut watched_root = None;
        let mut watch_error = None;
        let mut watch_at = Instant::now();
        let mut scan_at = Some(Instant::now());
        let mut burst_at = None;
        let mut retries = 0;

        while !context.stop.load(Ordering::Relaxed) {
            // 被重建的目录树可能在 watcher 重新注册前就已完整；一次成功扫描并不证明
            // 最终目录已被监视
            let existing = path.ancestors().find(|ancestor| ancestor.is_dir());

            if watcher.is_some() && existing != watched_root.as_deref() {
                watcher = None;
                watch_at = Instant::now();
            }

            if watcher.is_none() && Instant::now() >= watch_at {
                match watch(&path, sender.clone()) {
                    Ok((active, anchor)) => {
                        watcher = Some(active);
                        watched_root = Some(anchor);
                        watch_error = None;
                    }
                    Err(error) => {
                        watch_error = Some(format!("缓存目录监听失败: {error:#}"));
                        watch_at = Instant::now() + WATCH_RETRY;
                    }
                }

                scan_at.get_or_insert_with(Instant::now);
            }

            if scan_at.is_some_and(|deadline| Instant::now() >= deadline) {
                let mut catalog = read(&path).unwrap_or_else(|error| catalog::Catalog {
                    issues: vec![format!("Cache: {error:#}")],
                    ..Default::default()
                });

                let incomplete = !catalog.issues.is_empty()
                    || catalog
                        .entries
                        .iter()
                        .any(|entry| !entry.document.warnings.is_empty());

                // 写入与解码可能竞争，因此做有限次重试，而不是反复重扫一个安静的目录
                scan_at = if incomplete && retries < READ_RETRIES {
                    retries += 1;
                    Some(Instant::now() + Duration::from_millis(500 * u64::from(retries)))
                } else {
                    None
                };
                burst_at = None;

                if let Some(error) = &watch_error {
                    catalog.issues.push(error.clone());
                }

                if !publish(&context, catalog, source) {
                    break;
                }
            }

            let wait = scan_at
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .unwrap_or(STOP_CHECK)
                .min(STOP_CHECK);

            match receiver.recv_timeout(wait) {
                Ok(Ok(event)) if affects_cache_with_extension(&event, &path, extension) => {
                    if event.paths.iter().any(|changed| path.starts_with(changed)) {
                        watcher = None;
                        watch_at = Instant::now();
                    }

                    let now = Instant::now();
                    let start = *burst_at.get_or_insert(now);
                    scan_at = Some((now + DEBOUNCE).min(start + Duration::from_secs(1)));
                    retries = 0;
                }
                Ok(Err(error)) => {
                    watch_error = Some(format!("缓存目录监听失败: {error}"));
                    watcher = None;
                    watch_at = Instant::now() + WATCH_RETRY;
                    scan_at = Some(Instant::now());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                _ => {}
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, Flag, ModifyKind, RenameMode};
    use std::{
        fs,
        sync::{Arc, atomic::AtomicBool},
    };

    fn wait_for(
        receiver: &mut tokio::sync::mpsc::Receiver<CatalogUpdate>,
        predicate: impl Fn(&catalog::Catalog) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(8);

        while Instant::now() < deadline {
            if let Ok(update) = receiver.try_recv()
                && predicate(&update.catalog)
            {
                return;
            }

            std::thread::sleep(Duration::from_millis(20));
        }

        panic!("没有收到预期的缓存更新");
    }

    fn start(
        path: PathBuf,
    ) -> (
        Arc<AtomicBool>,
        tokio::sync::mpsc::Receiver<CatalogUpdate>,
        std::thread::JoinHandle<()>,
    ) {
        let (catalogs, receiver) = tokio::sync::mpsc::channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let task = spawn(
            path,
            Context {
                origin: Instant::now(),
                catalogs,
                stop: stop.clone(),
            },
        );

        (stop, receiver, task)
    }

    #[test]
    fn kugou_events_use_their_own_extension() {
        let path = PathBuf::from("cache");
        let krc = Event::new(EventKind::Create(CreateKind::File)).add_path(path.join("song.krc"));
        let qrc =
            Event::new(EventKind::Create(CreateKind::File)).add_path(path.join("song_qm.qrc"));
        assert!(affects_cache_with_extension(&krc, &path, "krc"));
        assert!(!affects_cache_with_extension(&qrc, &path, "krc"));
    }

    #[test]
    fn filters_access_and_unrelated_files_but_keeps_companions_and_rescans() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("cache");

        for name in ["song_qm.qrc", "song_qmts.qrc", "song_qmRoma.qrc"] {
            let event = Event::new(EventKind::Create(CreateKind::File)).add_path(path.join(name));
            assert!(affects_cache(&event, &path));
        }

        let unrelated =
            Event::new(EventKind::Create(CreateKind::File)).add_path(path.join("index.db"));
        assert!(!affects_cache(&unrelated, &path));

        let access =
            Event::new(EventKind::Access(AccessKind::Any)).add_path(path.join("song_qm.qrc"));
        assert!(!affects_cache(&access, &path));

        let rename = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(path.join("temporary"))
            .add_path(path.join("song_qm.qrc"));
        assert!(affects_cache(&rename, &path));
        assert!(affects_cache(
            &Event::new(EventKind::Other).set_flag(Flag::Rescan),
            &path
        ));
    }

    #[test]
    fn native_events_refresh_catalog_and_recover_after_directory_recreation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("cache");
        fs::create_dir(&path).unwrap();

        let (stop, mut receiver, task) = start(path.clone());
        wait_for(&mut receiver, |catalog| catalog.issues.is_empty());

        fs::write(path.join("index.db"), "unrelated").unwrap();
        std::thread::sleep(Duration::from_millis(600));
        assert!(receiver.try_recv().is_err(), "无关文件不应触发扫描");

        let companion = path.join("song_qmRoma.qrc");
        fs::write(&companion, "first").unwrap();
        wait_for(&mut receiver, |catalog| catalog.issues.is_empty());
        fs::write(&companion, "other").unwrap();
        wait_for(&mut receiver, |catalog| catalog.issues.is_empty());
        fs::remove_file(&companion).unwrap();
        wait_for(&mut receiver, |catalog| catalog.issues.is_empty());

        // 损坏的 QRC 仍必须被报告；该测试避免依赖 QQ 的加密样本
        fs::write(path.join("first_qm.qrc"), "incomplete").unwrap();
        wait_for(&mut receiver, |catalog| {
            catalog
                .issues
                .iter()
                .any(|issue| issue.starts_with("first_qm.qrc:"))
        });
        fs::rename(path.join("first_qm.qrc"), path.join("second_qm.qrc")).unwrap();
        wait_for(&mut receiver, |catalog| {
            catalog
                .issues
                .iter()
                .any(|issue| issue.starts_with("second_qm.qrc:"))
        });
        fs::remove_file(path.join("second_qm.qrc")).unwrap();
        wait_for(&mut receiver, |catalog| catalog.issues.is_empty());

        fs::remove_dir_all(&path).unwrap();
        wait_for(&mut receiver, |catalog| {
            catalog
                .issues
                .iter()
                .any(|issue| issue.starts_with("Cache:"))
        });
        fs::create_dir(&path).unwrap();
        fs::write(path.join("restored_qm.qrc"), "incomplete").unwrap();
        wait_for(&mut receiver, |catalog| {
            catalog
                .issues
                .iter()
                .any(|issue| issue.starts_with("restored_qm.qrc:"))
        });

        stop.store(true, Ordering::Relaxed);
        task.join().unwrap();
    }

    #[test]
    fn discovers_cache_created_below_initially_missing_parents() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("new").join("nested").join("cache");
        let (stop, mut receiver, task) = start(path.clone());
        wait_for(&mut receiver, |catalog| !catalog.issues.is_empty());
        fs::create_dir_all(&path).unwrap();
        wait_for(&mut receiver, |catalog| catalog.issues.is_empty());
        fs::write(path.join("created_qm.qrc"), "incomplete").unwrap();
        wait_for(&mut receiver, |catalog| {
            catalog
                .issues
                .iter()
                .any(|issue| issue.starts_with("created_qm.qrc:"))
        });

        stop.store(true, Ordering::Relaxed);
        task.join().unwrap();
    }

    #[test]
    fn stops_when_catalog_channel_is_full() {
        let (catalogs, _receiver) = tokio::sync::mpsc::channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let context = Context {
            origin: Instant::now(),
            catalogs,
            stop: stop.clone(),
        };
        assert!(publish(
            &context,
            catalog::Catalog::default(),
            Source::QqMusic
        ));

        let (finished, completion) = mpsc::channel();
        let task = std::thread::spawn(move || {
            finished
                .send(publish(
                    &context,
                    catalog::Catalog::default(),
                    Source::QqMusic,
                ))
                .unwrap();
        });
        stop.store(true, Ordering::Relaxed);
        assert!(!completion.recv_timeout(Duration::from_secs(2)).unwrap());
        task.join().unwrap();
    }
}
