//! Cached plugin catalog kept current by file-system watching, with change notifications.
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

#[derive(Debug, Default)]
pub struct PluginCatalog {
    pub plugins: Vec<Value>,
    pub errors: Vec<String>,
    /// Bumped whenever `plugins` or `errors` change; lets clients tell catalogs apart.
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginEvent {
    Plugins { revision: u64 },
    Config { id: String },
}

/// Single owner of plugin discovery. Requests read the cached catalog; only file changes or an
/// explicit rescan touch the disk.
pub struct PluginRegistry {
    embedded: Vec<Value>,
    roots: Vec<PluginRoot>,
    config_root: PathBuf,
    catalog: RwLock<Arc<PluginCatalog>>,
    events: broadcast::Sender<PluginEvent>,
}

impl PluginRegistry {
    /// `embedded` lists manifests shipped inside the binary; bundled roots on disk replace them.
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

    pub fn catalog(&self) -> Arc<PluginCatalog> {
        self.catalog.read().expect("plugin catalog lock").clone()
    }

    pub fn find(&self, id: &str) -> Option<Value> {
        self.catalog()
            .plugins
            .iter()
            .find(|plugin| plugin["id"] == id)
            .cloned()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PluginEvent> {
        self.events.subscribe()
    }

    /// Re-read every plugin directory; notifies subscribers only when the result differs.
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

    pub fn config_changed(&self, id: &str) {
        let _ = self.events.send(PluginEvent::Config { id: id.to_owned() });
    }

    /// Watch plugin roots, each plugin's resolved directory (links point outside the roots)
    /// and the config directory until `stop` is set.
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
        // A missing target is watched through an ancestor; once anything changes, rebuild the
        // watches so a newly created directory is watched itself.
        let missing = targets.iter().any(|target| !target.is_dir());
        for target in targets {
            watch_existing(&mut watcher, &target)?;
        }
        // Changes made between the previous scan and the new watches must not be lost.
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
                        // New or removed plugins need a fresh set of directory watches.
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

/// Discover plugins: the embedded index first, replaced by anything found on disk. Bundled
/// roots yield `builtin:<folder>` ids and user roots `<folder>`, so the namespaces never mix.
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
            // Hidden directories are install staging and backups; a plugin is discoverable
            // only after `hysilens build` or an install publishes its manifest.
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

/// `<root>/<id>.json` for external plugins, `<root>/builtin/<name>.json` for bundled ones.
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

/// Watch `path`, or its nearest existing ancestor so that creating it is noticed.
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

    /// Publish `folder` under `root`; `id` is what the manifest claims.
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

        // Same folder name in the user root is a different plugin, not an override.
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
