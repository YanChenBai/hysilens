//! 从旧安装布局一次性导入 settings、plugin config 与 plugin

use anyhow::Context;
use std::path::Path;

/// 一次性导入旧版数据，原目录与被替换的文件都保持可恢复
///
/// # Errors
/// 目标目录无法创建或写入，或旧文件无法复制时失败
pub(super) fn migrate(legacy: Option<&Path>, destination: &Path) -> anyhow::Result<()> {
    let marker = destination.join(".data-migration-v1");

    if marker.exists() {
        return Ok(());
    }

    let source = legacy.filter(|path| path.is_dir() && *path != destination);

    if source.is_none() && !destination.join("themes").is_dir() {
        return Ok(());
    }

    std::fs::create_dir_all(destination)?;

    if let Some(source) = source {
        for (old, new) in [
            ("settings.json", "settings.json"),
            ("plugins", "plugins"),
            ("themes", "plugin-configs"),
            ("plugin-configs", "plugin-configs"),
        ] {
            merge(&source.join(old), &destination.join(new), destination)?;
        }
    }

    // 自定义数据根目录在改名之前把主题配置存在 `themes` 下
    merge(
        &destination.join("themes"),
        &destination.join("plugin-configs"),
        destination,
    )?;

    std::fs::write(marker, b"Hysilens data migration v1\n")?;

    Ok(())
}

/// 递归地把 `source` 复制到 `destination`，保留较新的那个文件
fn merge(source: &Path, destination: &Path, data_root: &Path) -> anyhow::Result<()> {
    let metadata = match std::fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };

    // 不遍历外部 plugin 包中的链接
    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    if metadata.is_dir() {
        std::fs::create_dir_all(destination)?;

        for entry in std::fs::read_dir(source)? {
            let entry = entry?;

            merge(
                &entry.path(),
                &destination.join(entry.file_name()),
                data_root,
            )?;
        }

        return Ok(());
    }

    if !metadata.is_file() {
        return Ok(());
    }

    // 保留已存在的较新文件，但在替换之前先备份
    if destination.is_file() {
        if std::fs::metadata(destination)?.modified()? >= metadata.modified()? {
            return Ok(());
        }

        let backup = data_root
            .join("migration-backup")
            .join(destination.strip_prefix(data_root)?);

        if !backup.exists() {
            std::fs::create_dir_all(backup.parent().context("配置备份路径无效")?)?;
            std::fs::copy(destination, backup)?;
        }
    }

    std::fs::create_dir_all(destination.parent().context("配置迁移路径无效")?)?;

    let temporary = destination.with_extension("migration.tmp");
    std::fs::copy(source, &temporary).with_context(|| format!("迁移失败: {}", source.display()))?;

    std::fs::File::options()
        .write(true)
        .open(&temporary)?
        .set_times(std::fs::FileTimes::new().set_modified(metadata.modified()?))?;
    std::fs::rename(temporary, destination)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{File, FileTimes},
        time::{Duration, UNIX_EPOCH},
    };

    fn write(path: &Path, text: &str, timestamp: u64) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();

        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(timestamp)))
            .unwrap();
    }

    #[test]
    fn imports_settings_plugin_configs_and_external_plugins_once_with_backups() {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("HysilensNext");
        let new = directory.path().join("Hysilens");

        write(&old.join("settings.json"), "latest", 200);
        write(&new.join("settings.json"), "previous", 100);
        write(&old.join("themes/ink-verse.json"), "config", 200);
        write(&old.join("plugins/example/dist/index.js"), "plugin", 200);

        migrate(Some(&old), &new).unwrap();

        assert_eq!(
            std::fs::read_to_string(new.join("settings.json")).unwrap(),
            "latest"
        );
        assert_eq!(
            std::fs::read_to_string(new.join("migration-backup/settings.json")).unwrap(),
            "previous"
        );
        assert_eq!(
            std::fs::read_to_string(new.join("plugin-configs/ink-verse.json")).unwrap(),
            "config"
        );
        assert_eq!(
            std::fs::read_to_string(new.join("plugins/example/dist/index.js")).unwrap(),
            "plugin"
        );
        assert!(old.join("settings.json").is_file());

        write(&old.join("settings.json"), "changed legacy", 300);

        migrate(Some(&old), &new).unwrap();

        assert_eq!(
            std::fs::read_to_string(new.join("settings.json")).unwrap(),
            "latest"
        );
    }

    #[test]
    fn renames_theme_storage_in_custom_roots_without_overwriting_newer_configs() {
        let directory = tempfile::tempdir().unwrap();

        write(&directory.path().join("themes/builtin.json"), "old", 100);
        write(
            &directory.path().join("plugin-configs/builtin.json"),
            "new",
            200,
        );
        write(
            &directory.path().join("themes/tilted.json"),
            "migrated",
            100,
        );

        migrate(None, directory.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(directory.path().join("plugin-configs/builtin.json")).unwrap(),
            "new"
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("plugin-configs/tilted.json")).unwrap(),
            "migrated"
        );
    }

    #[test]
    fn picks_the_newest_plugin_config_across_both_legacy_locations() {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("HysilensNext");
        let new = directory.path().join("Hysilens");

        write(&old.join("themes/builtin.json"), "older location", 100);
        write(&new.join("themes/builtin.json"), "newer location", 200);

        migrate(Some(&old), &new).unwrap();

        assert_eq!(
            std::fs::read_to_string(new.join("plugin-configs/builtin.json")).unwrap(),
            "newer location"
        );
    }
}
