//! Discover renderer plugins by their `hysilens.json` and persist one config file per plugin.
use anyhow::{Context, ensure};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

/// The only file a plugin directory needs for discovery; written by `hysilens build`.
pub const MANIFEST_FILE: &str = "hysilens.json";
pub const API_VERSION: u64 = 1;

pub fn root() -> PathBuf {
    if let Some(path) = std::env::var_os("HYSILENS_PLUGINS_DIR") {
        return path.into();
    }
    crate::config::data_root().join("plugins")
}

/// Namespace reserved for plugins shipped with the app; external plugins can never claim it.
pub const BUILTIN_PREFIX: &str = "builtin:";

/// A directory of plugin folders. Bundled roots name `builtin:<folder>`, user roots `<folder>`.
#[derive(Clone, Debug)]
pub struct PluginRoot {
    pub path: PathBuf,
    pub builtin: bool,
}

impl PluginRoot {
    pub fn id_for(&self, folder: &str) -> String {
        if self.builtin {
            format!("{BUILTIN_PREFIX}{folder}")
        } else {
            folder.to_owned()
        }
    }
}

fn bundled_root() -> PathBuf {
    if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/public/plugins")
    } else {
        std::env::current_exe()
            .unwrap_or_default()
            .with_file_name("plugins")
    }
}

pub fn roots() -> Vec<PluginRoot> {
    vec![
        PluginRoot {
            path: bundled_root(),
            builtin: true,
        },
        PluginRoot {
            path: root(),
            builtin: false,
        },
    ]
}

pub fn directory(id: &str) -> anyhow::Result<Option<PathBuf>> {
    ensure!(valid_id(id), "插件标识无效");
    Ok(roots()
        .into_iter()
        .filter(|root| root.builtin == is_builtin(id))
        .map(|root| root.path.join(local_name(id)))
        .find(|directory| directory.join(MANIFEST_FILE).is_file()))
}

pub fn config_root() -> PathBuf {
    std::env::var_os("HYSILENS_PLUGIN_CONFIGS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::data_root().join("plugin-configs"))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

/// `<name>` for external plugins, `builtin:<name>` for bundled ones.
pub fn valid_id(id: &str) -> bool {
    valid_name(local_name(id))
}

pub fn is_builtin(id: &str) -> bool {
    id.starts_with(BUILTIN_PREFIX)
}

/// Folder and file stem for an id. `:` is not a valid path character on Windows, so bundled
/// plugins drop the namespace and live in a root that implies it.
pub fn local_name(id: &str) -> &str {
    id.strip_prefix(BUILTIN_PREFIX).unwrap_or(id)
}

/// Bundled configs sit in their own folder so `builtin:x` can never collide with external `x`.
pub fn config_path(id: &str) -> PathBuf {
    if is_builtin(id) {
        config_root()
            .join("builtin")
            .join(format!("{}.json", local_name(id)))
    } else {
        config_root().join(format!("{id}.json"))
    }
}

/// Relative forward-slash path whose every component stays inside the plugin.
fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

pub fn resource_url(id: &str, path: &str) -> anyhow::Result<String> {
    ensure!(valid_id(id), "插件标识无效");
    ensure!(safe_relative(path), "插件资源路径无效");
    Ok(format!("/plugins/{id}/{path}"))
}

pub fn local_path(root: &Path, path: &str) -> anyhow::Result<PathBuf> {
    ensure!(safe_relative(path), "插件路径越界");
    let base = root.canonicalize()?;
    let candidate = base.join(path).canonicalize()?;
    ensure!(candidate.starts_with(&base), "插件路径越界");
    Ok(candidate)
}

pub fn manifest(id: &str) -> anyhow::Result<Value> {
    let directory = directory(id)?.context("插件不存在")?;
    manifest_at(&directory, id)
}

/// Load and check a manifest with the same rules as `hysilens validate`.
pub fn manifest_at(directory: &Path, id: &str) -> anyhow::Result<Value> {
    ensure!(valid_id(id), "插件标识无效");
    let path = local_path(directory, MANIFEST_FILE)?;
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure!(manifest["apiVersion"] == API_VERSION, "插件接口版本不支持");
    ensure!(manifest["id"] == id, "插件 id 与目录名 {id} 不一致");
    ensure!(
        manifest["name"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty()),
        "插件名称缺失"
    );
    let entry = manifest["entry"].as_str().context("插件入口缺失")?;
    ensure!(entry.ends_with(".js"), "插件入口必须是 JS");
    let entry = local_path(directory, entry).context("插件入口不存在")?;
    for style in manifest["styles"].as_array().context("styles 必须是数组")? {
        let style = style.as_str().context("样式路径无效")?;
        local_path(directory, style).with_context(|| format!("样式文件不存在：{style}"))?;
    }
    ensure!(manifest["configSchema"].is_object(), "配置 Schema 缺失");
    jsonschema::validator_for(&manifest["configSchema"]).context("配置 Schema 无效")?;
    if !manifest["revision"].as_str().is_some_and(|s| !s.is_empty()) {
        manifest["revision"] = std::fs::metadata(entry)?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
            .to_string()
            .into();
    }
    Ok(manifest)
}

pub fn read_config(id: &str) -> anyhow::Result<Value> {
    ensure!(valid_id(id), "插件标识无效");
    let path = config_path(id);
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure!(value.is_object(), "插件配置必须是 JSON 对象");
    Ok(value)
}

pub fn save_config(id: &str, schema: &Value, value: &Value) -> anyhow::Result<()> {
    ensure!(valid_id(id) && value.is_object(), "插件配置无效");
    let validator = jsonschema::validator_for(schema)?;
    validator
        .validate(value)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let path = config_path(id);
    std::fs::create_dir_all(path.parent().context("配置路径无效")?)?;
    static NEXT_WRITE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = NEXT_WRITE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{serial}.tmp", std::process::id()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

/// Largest archive accepted for installation.
pub const MAX_PACKAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;

/// Install a `hysilens pack` archive into `root/<id>`, replacing an earlier copy atomically.
/// The archive is unpacked to a hidden staging directory and validated with the same rules as
/// discovery before anything visible changes.
pub fn install_package(root: &Path, bytes: &[u8]) -> anyhow::Result<Value> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("不是有效的 zip 压缩包")?;
    ensure!(archive.len() <= MAX_ENTRIES, "压缩包文件数量过多");
    let id = package_id(&mut archive)?;
    std::fs::create_dir_all(root)?;
    let staging = root.join(format!(".install-{id}-{}", std::process::id()));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir(&staging)?;
    let staged = unpack(&mut archive, &staging).and_then(|()| manifest_at(&staging, &id));
    let manifest = match staged {
        Ok(manifest) => manifest,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    if let Err(error) = replace_directory(root, &id, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    Ok(manifest)
}

/// `hysilens pack` puts the manifest at the archive root; its id names the install directory.
fn package_id<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> anyhow::Result<String> {
    let mut file = archive
        .by_name(MANIFEST_FILE)
        .with_context(|| format!("压缩包根目录缺少 {MANIFEST_FILE}"))?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(&mut file, 1024 * 1024), &mut text)?;
    let manifest: Value = serde_json::from_str(&text).context("插件清单不是有效的 JSON")?;
    let id = manifest["id"].as_str().context("插件 id 缺失")?;
    ensure!(valid_id(id), "插件标识无效：{id}");
    ensure!(
        !is_builtin(id),
        "{id} 使用了内置插件保留的 {BUILTIN_PREFIX} 前缀，外部插件不能安装"
    );
    Ok(id.to_owned())
}

fn unpack<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    destination: &Path,
) -> anyhow::Result<()> {
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let label = String::from_utf8_lossy(entry.name_raw()).into_owned();
        let name = entry
            .enclosed_name()
            .with_context(|| format!("压缩包包含越界路径：{label}"))?;
        ensure!(!entry.is_symlink(), "压缩包不能包含符号链接：{label}");
        let target = destination.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        total += entry.size();
        ensure!(total <= MAX_UNPACKED_BYTES, "压缩包解压后过大");
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Read at most one byte past the declared size, so a lying header cannot inflate it.
        let declared = entry.size();
        let mut output = std::fs::File::create(&target)?;
        let written = std::io::copy(
            &mut std::io::Read::take(&mut entry, declared + 1),
            &mut output,
        )?;
        ensure!(
            written == declared,
            "压缩包条目大小不符：{}",
            target.display()
        );
    }
    Ok(())
}

fn replace_directory(root: &Path, id: &str, staging: &Path) -> anyhow::Result<()> {
    let target = root.join(id);
    match std::fs::symlink_metadata(&target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::rename(staging, &target)?;
        }
        Err(error) => return Err(error.into()),
        // Junctions count as symlinks here; never replace a `hysilens link` development copy.
        Ok(metadata) if metadata.file_type().is_symlink() => {
            anyhow::bail!("{id} 当前链接到开发目录（hysilens link），请先移除该链接再安装");
        }
        Ok(_) => {
            let backup = root.join(format!(".backup-{id}-{}", std::process::id()));
            if backup.exists() {
                std::fs::remove_dir_all(&backup)?;
            }
            std::fs::rename(&target, &backup).context("旧版本正在使用，无法替换")?;
            if let Err(error) = std::fs::rename(staging, &target) {
                std::fs::rename(&backup, &target)?;
                return Err(error.into());
            }
            std::fs::remove_dir_all(&backup)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publish(directory: &Path, manifest: &Value) {
        std::fs::create_dir_all(directory.join("assets")).unwrap();
        std::fs::write(directory.join("index.js"), "export {};").unwrap();
        std::fs::write(directory.join("assets/index.css"), "").unwrap();
        std::fs::write(
            directory.join(MANIFEST_FILE),
            serde_json::to_vec(manifest).unwrap(),
        )
        .unwrap();
    }

    fn sample() -> Value {
        serde_json::json!({
            "apiVersion": 1, "id": "example", "name": "Example", "version": "1.0.0",
            "entry": "index.js", "styles": ["assets/index.css"], "features": {},
            "configSchema": { "type": "object", "properties": { "size": { "type": "number", "default": 1 } } },
            "revision": "fixed-revision"
        })
    }

    #[test]
    fn loads_a_published_plugin_directory() {
        let directory = tempfile::tempdir().unwrap();
        publish(directory.path(), &sample());
        let loaded = manifest_at(directory.path(), "example").unwrap();
        assert_eq!(loaded["id"], "example");
        assert_eq!(loaded["revision"], "fixed-revision");
        assert!(manifest_at(directory.path(), "../example").is_err());
    }

    #[test]
    fn falls_back_to_entry_mtime_without_a_revision() {
        let directory = tempfile::tempdir().unwrap();
        let mut manifest = sample();
        manifest.as_object_mut().unwrap().remove("revision");
        publish(directory.path(), &manifest);
        let loaded = manifest_at(directory.path(), "example").unwrap();
        assert!(loaded["revision"].as_str().is_some_and(|s| !s.is_empty()));
    }

    #[test]
    fn rejects_manifests_the_host_cannot_serve() {
        let cases: [(&str, Value); 7] = [
            ("apiVersion", serde_json::json!(2)),
            ("id", serde_json::json!("other")),
            ("name", serde_json::json!(" ")),
            ("entry", serde_json::json!("../index.js")),
            ("entry", serde_json::json!("index.css")),
            ("styles", serde_json::json!(["missing.css"])),
            ("configSchema", serde_json::json!({ "type": 12 })),
        ];
        for (field, value) in cases {
            let directory = tempfile::tempdir().unwrap();
            let mut manifest = sample();
            manifest[field] = value;
            publish(directory.path(), &manifest);
            assert!(
                manifest_at(directory.path(), "example").is_err(),
                "{field} should be rejected"
            );
        }
    }

    #[test]
    fn resolves_packaged_resources_without_allowing_path_escapes() {
        assert_eq!(
            resource_url("example", "assets/font.woff2").unwrap(),
            "/plugins/example/assets/font.woff2"
        );
        for path in [
            "../secret",
            "/absolute.js",
            "javascript:alert(1)",
            "https://cdn.example.com/lib.js",
            "a\\b.js",
        ] {
            assert!(resource_url("example", path).is_err());
        }
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in entries {
            writer.start_file(*name, options).unwrap();
            std::io::Write::write_all(&mut writer, content).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn package(name: &str) -> Vec<u8> {
        let mut manifest = sample();
        manifest["name"] = name.into();
        let manifest = serde_json::to_vec(&manifest).unwrap();
        archive(&[
            (MANIFEST_FILE, &manifest),
            ("index.js", b"export {};"),
            ("assets/index.css", b""),
        ])
    }

    #[test]
    fn installs_and_replaces_packed_plugins() {
        let root = tempfile::tempdir().unwrap();
        let installed = install_package(root.path(), &package("First")).unwrap();
        assert_eq!(installed["id"], "example");
        assert!(root.path().join("example/assets/index.css").is_file());

        std::fs::write(root.path().join("example/stale.js"), "").unwrap();
        install_package(root.path(), &package("Second")).unwrap();
        assert_eq!(
            manifest_at(&root.path().join("example"), "example").unwrap()["name"],
            "Second"
        );
        assert!(!root.path().join("example/stale.js").exists());
        let leftovers: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("example")]);
    }

    #[test]
    fn rejects_unsafe_or_invalid_archives_without_touching_the_installed_copy() {
        let root = tempfile::tempdir().unwrap();
        install_package(root.path(), &package("Kept")).unwrap();
        let manifest = serde_json::to_vec(&sample()).unwrap();
        let cases: Vec<Vec<u8>> = vec![
            b"not a zip".to_vec(),
            archive(&[("index.js", b"")]),
            archive(&[(MANIFEST_FILE, &manifest), ("../escape.js", b"")]),
            archive(&[(MANIFEST_FILE, &manifest)]),
            archive(&[(MANIFEST_FILE, br#"{"id":"../x"}"#)]),
        ];
        for bytes in cases {
            assert!(install_package(root.path(), &bytes).is_err());
        }
        assert_eq!(
            manifest_at(&root.path().join("example"), "example").unwrap()["name"],
            "Kept"
        );
        assert!(!root.path().join("escape.js").exists());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    /// Written by `hysilens pack` (fflate); guards format compatibility with the zip crate.
    #[test]
    fn installs_archives_written_by_hysilens_pack() {
        let root = tempfile::tempdir().unwrap();
        let bytes = include_bytes!("../tests/fixtures/hysilens-pack.zip");
        let manifest = install_package(root.path(), bytes).unwrap();
        assert_eq!(manifest["id"], "basic");
        assert_eq!(manifest["apiVersion"], API_VERSION);
        assert!(root.path().join("basic/index.js").is_file());
        assert!(manifest_at(&root.path().join("basic"), "basic").is_ok());
    }

    #[test]
    fn never_replaces_a_linked_development_copy() {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        publish(source.path(), &sample());
        let link = root.path().join("example");
        #[cfg(windows)]
        assert!(
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(source.path())
                .output()
                .unwrap()
                .status
                .success()
        );
        #[cfg(unix)]
        std::os::unix::fs::symlink(source.path(), &link).unwrap();

        let error = install_package(root.path(), &package("Packed")).unwrap_err();
        assert!(error.to_string().contains("hysilens link"));
        assert_eq!(manifest_at(&link, "example").unwrap()["name"], "Example");
        assert!(source.path().join("index.js").is_file());
    }

    #[test]
    fn rejects_paths_outside_plugin_and_invalid_ids() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("index.js"), "export default {}").unwrap();
        assert!(local_path(directory.path(), "index.js").is_ok());
        for path in ["../index.js", "/index.js", "C:/index.js", "a\\index.js"] {
            assert!(local_path(directory.path(), path).is_err());
        }
        for id in [
            "",
            "../x",
            "@scope/name",
            "foo.json/",
            "builtin:",
            "builtin:../x",
            "other:x",
            "builtin:builtin:x",
        ] {
            assert!(!valid_id(id), "{id}");
        }
    }

    #[test]
    fn namespaces_bundled_ids_without_colons_in_paths() {
        assert!(valid_id("builtin:scroll") && is_builtin("builtin:scroll"));
        assert!(valid_id("scroll") && !is_builtin("scroll"));
        assert_eq!(local_name("builtin:scroll"), "scroll");
        assert_eq!(local_name("scroll"), "scroll");
        assert!(config_path("builtin:scroll").ends_with("builtin/scroll.json"));
        assert!(config_path("scroll").ends_with("scroll.json"));
        assert!(!config_path("scroll").ends_with("builtin/scroll.json"));
        assert_eq!(
            resource_url("builtin:scroll", "index.js").unwrap(),
            "/plugins/builtin:scroll/index.js"
        );
    }

    #[test]
    fn refuses_to_install_packages_claiming_the_builtin_namespace() {
        let root = tempfile::tempdir().unwrap();
        let mut manifest = sample();
        manifest["id"] = "builtin:example".into();
        let manifest = serde_json::to_vec(&manifest).unwrap();
        let bytes = archive(&[(MANIFEST_FILE, &manifest), ("index.js", b"")]);
        let error = install_package(root.path(), &bytes).unwrap_err();
        assert!(error.to_string().contains(BUILTIN_PREFIX));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
