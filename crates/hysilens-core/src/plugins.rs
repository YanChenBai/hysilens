//! 通过 renderer plugin 的 `hysilens.json` 发现它们，并为每个 plugin 持久化一个 config 文件

use anyhow::{Context, ensure};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

/// plugin 目录被发现所需的唯一文件；由 `hysilens build` 写入
pub const MANIFEST_FILE: &str = "hysilens.json";

/// 本 host 接受的 manifest `apiVersion`；更新的 plugin 会在发现阶段被拒绝
pub const API_VERSION: u64 = 1;

/// 存放用户安装 plugin 的目录；`HYSILENS_PLUGINS_DIR` 会覆盖数据根目录
pub fn root() -> PathBuf {
    if let Some(path) = std::env::var_os("HYSILENS_PLUGINS_DIR") {
        return path.into();
    }

    crate::config::data_root().join("plugins")
}

/// 为随应用发布的 plugin 保留的命名空间；外部 plugin 永远不能占用
pub const BUILTIN_PREFIX: &str = "builtin:";

/// 存放 plugin 文件夹的目录；内置根目录命名为 `builtin:<folder>`，
/// 用户根目录为 `<folder>`
#[derive(Clone, Debug)]
pub struct PluginRoot {
    /// 用于扫描 plugin 文件夹的目录
    pub path: PathBuf,

    /// 该根目录下的文件夹是否属于保留的内置命名空间
    pub builtin: bool,
}

impl PluginRoot {
    /// 本根目录中某个文件夹对应的完整 plugin id
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

/// 按查找顺序排列的所有 plugin 根目录：先是内置根目录，然后是用户根目录
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

/// plugin 的安装目录
///
/// # Errors
/// `id` 不是有效的 plugin id 时失败；未知或不存在的 plugin 返回 `Ok(None)`
pub fn directory(id: &str) -> anyhow::Result<Option<PathBuf>> {
    ensure!(valid_id(id), "插件标识无效");

    Ok(roots()
        .into_iter()
        .filter(|root| root.builtin == is_builtin(id))
        .map(|root| root.path.join(local_name(id)))
        .find(|directory| directory.join(MANIFEST_FILE).is_file()))
}

/// 移除用户安装的 plugin；开发链接只移除链接，已保存的配置保留
///
/// # Errors
/// id 无效、属于内置 plugin、目录越界或文件系统操作失败时返回错误
pub fn remove(root: &Path, id: &str) -> anyhow::Result<()> {
    ensure!(valid_id(id), "插件标识无效");
    ensure!(!is_builtin(id), "内置插件不能删除");

    let base = root.canonicalize().context("无法读取插件目录")?;
    let target = base.join(id);
    let metadata = std::fs::symlink_metadata(&target).context("插件不存在或无法读取")?;

    if metadata.file_type().is_symlink() {
        #[cfg(windows)]
        std::fs::remove_dir(&target).context("无法移除插件链接")?;
        #[cfg(not(windows))]
        std::fs::remove_file(&target).context("无法移除插件链接")?;

        return Ok(());
    }

    let resolved = target.canonicalize()?;
    ensure!(resolved.parent() == Some(base.as_path()), "插件目录越界");
    ensure!(metadata.is_dir(), "插件路径不是文件夹");

    std::fs::remove_dir_all(&resolved).context("无法删除插件目录")?;

    Ok(())
}

/// 存放每个 plugin 一个 config 文件的目录；`HYSILENS_PLUGIN_CONFIGS_DIR` 会覆盖它
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

/// 外部 plugin 为 `<name>`，内置 plugin 为 `builtin:<name>`
pub fn valid_id(id: &str) -> bool {
    valid_name(local_name(id))
}

/// `id` 是否使用保留的内置命名空间
pub fn is_builtin(id: &str) -> bool {
    id.starts_with(BUILTIN_PREFIX)
}

/// id 对应的文件夹名与文件名主干：`:` 在 Windows 上不是合法的路径字符，因此内置
/// plugin 去掉命名空间前缀，放在隐含该命名空间的根目录中
pub fn local_name(id: &str) -> &str {
    id.strip_prefix(BUILTIN_PREFIX).unwrap_or(id)
}

/// 内置 config 放在独立文件夹中，使 `builtin:x` 永不与外部 `x` 冲突
pub fn config_path(id: &str) -> PathBuf {
    if is_builtin(id) {
        config_root()
            .join("builtin")
            .join(format!("{}.json", local_name(id)))
    } else {
        config_root().join(format!("{id}.json"))
    }
}

/// 使用正斜杠的相对路径，且每一段都留在 plugin 内部
fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// overlay 获取 plugin 资源所用的 URL
///
/// # Errors
/// `id` 无效，或 `path` 为绝对路径、使用 URL scheme 或逃出 plugin 目录时失败
pub fn resource_url(id: &str, path: &str) -> anyhow::Result<String> {
    ensure!(valid_id(id), "插件标识无效");
    ensure!(safe_relative(path), "插件资源路径无效");

    Ok(format!("/plugins/{id}/{path}"))
}

/// 解析 `root` 内的 `path`，拒绝任何 canonicalize 之后逃出该目录的路径
///
/// # Errors
/// `path` 不是安全的相对路径，或 `root`、目标无法 canonicalize 时失败
pub fn local_path(root: &Path, path: &str) -> anyhow::Result<PathBuf> {
    ensure!(safe_relative(path), "插件路径越界");

    let base = root.canonicalize()?;
    let candidate = base.join(path).canonicalize()?;

    ensure!(candidate.starts_with(&base), "插件路径越界");

    Ok(candidate)
}

/// 已安装 plugin 的 manifest
///
/// # Errors
/// plugin 未安装，或其 manifest 未通过 [`manifest_at`] 校验时失败
pub fn manifest(id: &str) -> anyhow::Result<Value> {
    let directory = directory(id)?.context("插件不存在")?;

    manifest_at(&directory, id)
}

/// 按与 `hysilens validate` 相同的规则加载并检查 manifest
///
/// # Errors
/// id 或引用的路径无效、manifest 不是有效 JSON、其 `apiVersion`/`id` 与目录不符，
/// 或必填字段缺失或无效时失败
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

    // 显式 revision 优先；否则用入口文件的 mtime 标识这次 build
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

/// 读取 plugin 已保存的 config，尚未保存过任何内容时返回空对象
///
/// # Errors
/// `id` 无效或已存储的文件不是 JSON 对象时失败
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

/// 用 `schema` 校验 `value` 并把它写成该 plugin 的 config 文件
///
/// # Errors
/// id 无效、`value` 不是 JSON 对象、`value` 不符合 `schema`，或文件无法写入时失败
pub fn save_config(id: &str, schema: &Value, value: &Value) -> anyhow::Result<()> {
    ensure!(valid_id(id) && value.is_object(), "插件配置无效");

    let validator = jsonschema::validator_for(schema)?;

    validator
        .validate(value)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    let path = config_path(id);

    std::fs::create_dir_all(path.parent().context("配置路径无效")?)?;

    // 每次写入带独立后缀，让并发的 save 各自准备临时文件
    static NEXT_WRITE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let serial = NEXT_WRITE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{serial}.tmp", std::process::id()));

    std::fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(temporary, path)?;

    Ok(())
}

/// 安装时接受的最大压缩包大小
pub const MAX_PACKAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;

/// 把 `hysilens pack` 压缩包安装到 `root/<id>`，原子地替换之前的副本
/// 压缩包先解到隐藏的暂存目录，并用与发现阶段相同的规则校验，之后才改动可见内容
///
/// # Errors
/// 压缩包不是有效 zip、包含不安全路径或过大的条目、其 manifest 无效，或已安装目录
/// 无法替换时失败
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

/// `hysilens pack` 把 manifest 放在压缩包根目录；它的 id 决定安装目录名
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

        // 最多读到声明大小之后一个字节，使谎报的头部无法放大写入量
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
        // junction 在这里也算符号链接；永不替换 `hysilens link` 的开发副本
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

    /// 由 `hysilens pack`（fflate）写出；用于保证与 zip crate 的格式兼容
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
    fn removes_only_the_selected_external_plugin() {
        let root = tempfile::tempdir().unwrap();

        publish(&root.path().join("example"), &sample());
        std::fs::create_dir(root.path().join("other")).unwrap();

        remove(root.path(), "example").unwrap();

        assert!(!root.path().join("example").exists());
        assert!(root.path().join("other").is_dir());
        assert!(remove(root.path(), "example").is_err());
    }

    #[test]
    fn refuses_to_remove_builtin_plugins_and_unsafe_paths() {
        let root = tempfile::tempdir().unwrap();

        publish(&root.path().join("example"), &sample());

        for id in ["builtin:example", "../example", "", ".", "example/.."] {
            assert!(remove(root.path(), id).is_err(), "{id}");
        }

        assert!(root.path().join("example/index.js").is_file());
    }

    #[test]
    fn removes_development_links_without_deleting_the_source() {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let link = root.path().join("example");

        publish(source.path(), &sample());

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

        remove(root.path(), "example").unwrap();

        assert!(std::fs::symlink_metadata(link).is_err());
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
