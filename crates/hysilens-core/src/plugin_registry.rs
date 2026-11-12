//! 通过文件系统监听保持最新的 plugin catalog，并提供变更通知

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::Value;

use crate::plugins::PluginRoot;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use tokio::sync::broadcast;

const DEBOUNCE: Duration = Duration::from_millis(250);
const WATCH_RETRY: Duration = Duration::from_secs(5);

/// 所有已发现 plugin 的 snapshot，加上扫描期间收集的错误
#[derive(Debug, Default)]
pub struct PluginCatalog {
    /// 按 id 排序的 manifest；内置 plugin 保留其 `builtin:` 前缀
    pub plugins: Vec<Value>,

    /// 扫描失败项，每项都带有它所属的 plugin id 前缀
    pub errors: Vec<String>,

    /// `plugins` 或 `errors` 变化时递增；让客户端能区分不同的 catalog
    pub revision: u64,
}

/// 客户端缓存的任何 plugin 相关内容的变更通知
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginEvent {
    /// catalog 发生变化
    Plugins {
        /// 客户端必须重新获取的 catalog revision
        revision: u64,
    },

    /// 某个 plugin 保存的配置发生变化
    Config {
        /// 配置文件发生变化的 plugin
        id: String,
    },
}

/// plugin 发现的唯一所有者；请求读取缓存的 catalog，只有文件变化或显式 rescan
/// 才会访问磁盘
#[derive(Debug)]
pub struct PluginRegistry {
    embedded: Vec<Value>,

    roots: Vec<PluginRoot>,

    config_root: PathBuf,

    catalog: RwLock<Arc<PluginCatalog>>,

    events: broadcast::Sender<PluginEvent>,
}

impl PluginRegistry {
    /// `embedded` 列出随二进制一起发布的 manifest；磁盘上的内置根目录会替换它们
    pub fn new(embedded: Vec<Value>, roots: Vec<PluginRoot>, config_root: PathBuf) -> Arc<Self> {
        let embedded = embedded
            .into_iter()
            .filter(|plugin| {
                plugin["apiVersion"] == crate::plugins::API_VERSION && plugin["entry"].is_string()
            })
            .collect::<Vec<_>>();

        let (plugins, errors) = scan(&embedded, &roots);
        let (events, _) = broadcast::channel(64);

        Arc::new(Self {
            embedded,
            roots,
            config_root,
            catalog: RwLock::new(Arc::new(PluginCatalog {
                plugins,
                errors,
                revision: 1,
            })),
            events,
        })
    }

    /// 当前 catalog；catalog 由 `Arc` 共享，因此克隆代价很低
    pub fn catalog(&self) -> Arc<PluginCatalog> {
        self.catalog.read().expect("plugin catalog lock").clone()
    }

    /// 按完整 id 查找 plugin manifest（内置 plugin 的 id 为 `builtin:<name>`）
    pub fn find(&self, id: &str) -> Option<Value> {
        self.catalog()
            .plugins
            .iter()
            .find(|plugin| plugin["id"] == id)
            .cloned()
    }

    /// 订阅 catalog 与 config 变更；落后的 receiver 必须重新获取全部内容
    pub fn subscribe(&self) -> broadcast::Receiver<PluginEvent> {
        self.events.subscribe()
    }

    /// 重新读取每个 plugin 目录；结果不同时才通知订阅者
    pub fn rescan(&self) -> Arc<PluginCatalog> {
        let (plugins, errors) = scan(&self.embedded, &self.roots);
        let mut catalog = self.catalog.write().expect("plugin catalog lock");

        if catalog.plugins != plugins || catalog.errors != errors {
            *catalog = Arc::new(PluginCatalog {
                plugins,
                errors,
                revision: catalog.revision + 1,
            });

            let _ = self.events.send(PluginEvent::Plugins {
                revision: catalog.revision,
            });
        }

        catalog.clone()
    }

    /// 宣告 `id` 保存的配置发生变化，不重新扫描 catalog
    pub fn config_changed(&self, id: &str) {
        let _ = self.events.send(PluginEvent::Config { id: id.to_owned() });
    }

    /// 监听 plugin 根目录、每个 plugin 解析后的目录（链接指向根目录之外）
    /// 以及 config 目录，直到 `stop` 被置位
    pub fn spawn_watcher(self: &Arc<Self>, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
        let registry = self.clone();

        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if let Err(error) = registry.watch_until_layout_changes(&stop) {
                    eprintln!("插件目录监听失败: {error}");

                    sleep_unless_stopped(WATCH_RETRY, &stop);
                }
            }
        })
    }

    fn watch_until_layout_changes(&self, stop: &AtomicBool) -> anyhow::Result<()> {
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(sender)?;
        let mut targets: Vec<PathBuf> = self.roots.iter().map(|root| root.path.clone()).collect();

        targets.push(self.config_root.clone());
        targets.push(self.config_root.join("builtin"));
        targets.extend(self.plugin_directories());

        // 缺失的目标通过其祖先目录来监听；一旦有变化就重建监视线，
        // 让新创建的目录被直接监听
        let missing = targets.iter().any(|target| !target.is_dir());

        for target in targets {
            watch_existing(&mut watcher, &target)?;
        }

        // 上一次扫描与新监听建立之间发生的改动不能丢失
        self.rescan();

        let mut plugins_dirty = false;
        let mut configs = BTreeSet::new();

        while !stop.load(Ordering::Relaxed) {
            match receiver.recv_timeout(DEBOUNCE) {
                Ok(event) => self.classify(event?, &mut plugins_dirty, &mut configs),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    for id in std::mem::take(&mut configs) {
                        self.config_changed(&id);
                    }

                    if std::mem::take(&mut plugins_dirty) {
                        let before = self.catalog().revision;

                        // 新增或移除 plugin 需要重新建立一整套目录监听
                        if self.rescan().revision != before || missing {
                            return Ok(());
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => anyhow::bail!("监听通道已关闭"),
            }
        }

        Ok(())
    }

    fn classify(&self, event: Event, plugins_dirty: &mut bool, configs: &mut BTreeSet<String>) {
        for path in event.paths {
            match config_id(&self.config_root, &path) {
                Some(id) => {
                    configs.insert(id);
                }
                None if !path.starts_with(&self.config_root) => *plugins_dirty = true,
                None => {}
            }
        }
    }

    fn plugin_directories(&self) -> Vec<PathBuf> {
        self.catalog()
            .plugins
            .iter()
            .filter_map(|plugin| plugin["id"].as_str())
            .flat_map(|id| {
                self.roots
                    .iter()
                    .filter(move |root| root.builtin == crate::plugins::is_builtin(id))
                    .map(move |root| root.path.join(crate::plugins::local_name(id)))
            })
            .filter_map(|directory| directory.canonicalize().ok())
            .collect()
    }
}

/// 发现 plugin：先放入内嵌索引，再用磁盘上找到的内容替换；内置根目录产生
/// `builtin:<folder>` 形式的 id，用户根目录产生 `<folder>`，两个命名空间不会混用
pub fn scan(embedded: &[Value], roots: &[PluginRoot]) -> (Vec<Value>, Vec<String>) {
    let mut plugins = embedded.to_vec();
    let mut errors = Vec::new();

    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root.path) else {
            continue;
        };

        let mut entries: Vec<_> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .collect();
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let folder = entry.file_name().to_string_lossy().into_owned();

            // 隐藏目录是安装暂存区与备份；只有在 `hysilens build` 或安装操作发布
            // manifest 之后 plugin 才可被发现
            if folder.starts_with('.')
                || !entry.path().join(crate::plugins::MANIFEST_FILE).is_file()
            {
                continue;
            }

            let id = root.id_for(&folder);
            plugins.retain(|old| old["id"] != id);
            errors.retain(|old: &String| !old.starts_with(&format!("{id}: ")));

            match crate::plugins::manifest_at(&entry.path(), &id) {
                Ok(manifest) => plugins.push(manifest),
                Err(error) => errors.push(format!("{id}: {error:#}")),
            }
        }
    }

    plugins.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

    (plugins, errors)
}

/// 外部 plugin 为 `<root>/<id>.json`，内置 plugin 为 `<root>/builtin/<name>.json`
fn config_id(config_root: &Path, path: &Path) -> Option<String> {
    if path.extension()? != "json" {
        return None;
    }

    let stem = path.file_stem()?.to_str()?;
    let id = if path.parent() == Some(config_root) {
        stem.to_owned()
    } else if path.parent() == Some(config_root.join("builtin").as_path()) {
        format!("{}{stem}", crate::plugins::BUILTIN_PREFIX)
    } else {
        return None;
    };

    (crate::plugins::valid_id(&id) && crate::plugins::local_name(&id) == stem).then_some(id)
}

/// 监听 `path`，若其不存在则监听最近的已存在祖先目录，以便察觉它的创建
fn watch_existing(watcher: &mut RecommendedWatcher, path: &Path) -> anyhow::Result<()> {
    if let Some(existing) = path.ancestors().find(|ancestor| ancestor.is_dir()) {
        watcher.watch(existing, RecursiveMode::NonRecursive)?;
    }

    Ok(())
}

fn sleep_unless_stopped(duration: Duration, stop: &AtomicBool) {
    let deadline = std::time::Instant::now() + duration;

    while std::time::Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(DEBOUNCE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(path: &Path, builtin: bool) -> PluginRoot {
        PluginRoot {
            path: path.to_path_buf(),
            builtin,
        }
    }

    /// 在 `root` 下发布 `folder`；`id` 是 manifest 声称的 id
    fn publish(root: &Path, folder: &str, id: &str, name: &str) {
        let directory = root.join(folder);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("index.js"), "export {};").unwrap();

        let manifest = serde_json::json!({
            "apiVersion": 1, "id": id, "name": name, "version": "1.0.0",
            "entry": "index.js", "styles": [], "features": {},
            "configSchema": { "type": "object" }, "revision": name
        });

        std::fs::write(
            directory.join(crate::plugins::MANIFEST_FILE),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn expect_event(
        receiver: &mut broadcast::Receiver<PluginEvent>,
        predicate: impl Fn(&PluginEvent) -> bool,
    ) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);

        while std::time::Instant::now() < deadline {
            match receiver.try_recv() {
                Ok(event) if predicate(&event) => return,
                Ok(_) | Err(broadcast::error::TryRecvError::Lagged(_)) => {}
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }

        panic!("expected plugin event was not delivered");
    }

    #[test]
    fn keeps_bundled_and_external_namespaces_apart() {
        let bundled = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();

        publish(bundled.path(), "example", "builtin:example", "Bundled");

        let registry = PluginRegistry::new(
            vec![serde_json::json!({ "id": "legacy", "main": "index.js" })],
            vec![root(bundled.path(), true), root(user.path(), false)],
            user.path().join("configs"),
        );

        let mut events = registry.subscribe();

        assert_eq!(registry.find("builtin:example").unwrap()["name"], "Bundled");
        assert!(registry.find("example").is_none());
        assert!(registry.find("legacy").is_none());

        assert_eq!(registry.rescan().revision, 1);
        assert!(events.try_recv().is_err());

        // 用户根目录中的同名文件夹是另一个 plugin，而不是覆盖
        publish(user.path(), "example", "example", "User");

        let catalog = registry.rescan();

        assert_eq!(catalog.revision, 2);
        assert_eq!(registry.find("builtin:example").unwrap()["name"], "Bundled");
        assert_eq!(registry.find("example").unwrap()["name"], "User");
        assert_eq!(
            events.try_recv().unwrap(),
            PluginEvent::Plugins { revision: 2 }
        );
    }

    #[test]
    fn external_plugins_cannot_claim_the_builtin_namespace() {
        let bundled = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();

        publish(bundled.path(), "plain", "plain", "Unprefixed");
        publish(user.path(), "spoof", "builtin:spoof", "Spoof");

        let registry = PluginRegistry::new(
            vec![],
            vec![root(bundled.path(), true), root(user.path(), false)],
            user.path().join("configs"),
        );

        let catalog = registry.catalog();

        assert!(catalog.plugins.is_empty());
        assert_eq!(catalog.errors.len(), 2);
        assert!(registry.find("builtin:spoof").is_none());
    }

    #[test]
    fn reports_invalid_plugins_without_hiding_valid_ones() {
        let user = tempfile::tempdir().unwrap();

        publish(user.path(), "good", "good", "Good");
        publish(user.path(), "bad", "bad", "Bad");
        std::fs::remove_file(user.path().join("bad/index.js")).unwrap();

        let registry = PluginRegistry::new(
            vec![],
            vec![root(user.path(), false)],
            user.path().join("c"),
        );

        let catalog = registry.catalog();

        assert_eq!(catalog.plugins.len(), 1);
        assert_eq!(catalog.errors.len(), 1);
        assert!(catalog.errors[0].starts_with("bad: "));
    }

    #[test]
    fn watcher_announces_new_plugins_and_config_files() {
        let data = tempfile::tempdir().unwrap();
        let configs = data.path().join("plugin-configs");
        let plugins = data.path().join("plugins");

        std::fs::create_dir_all(configs.join("builtin")).unwrap();
        std::fs::create_dir_all(&plugins).unwrap();

        let registry = PluginRegistry::new(vec![], vec![root(&plugins, false)], configs.clone());
        let mut events = registry.subscribe();
        let stop = Arc::new(AtomicBool::new(false));
        let watcher = registry.spawn_watcher(stop.clone());

        std::thread::sleep(Duration::from_millis(300));

        publish(&plugins, "fresh", "fresh", "Fresh");

        expect_event(&mut events, |event| {
            matches!(event, PluginEvent::Plugins { .. })
        });

        assert!(registry.find("fresh").is_some());

        std::fs::write(configs.join("fresh.json"), "{}").unwrap();
        std::fs::write(configs.join("fresh.json.1.tmp"), "{}").unwrap();

        expect_event(&mut events, |event| {
            event == &PluginEvent::Config { id: "fresh".into() }
        });

        std::fs::write(configs.join("builtin/scroll.json"), "{}").unwrap();

        expect_event(&mut events, |event| {
            event
                == &PluginEvent::Config {
                    id: "builtin:scroll".into(),
                }
        });

        stop.store(true, Ordering::Relaxed);
        watcher.join().unwrap();
    }
}
