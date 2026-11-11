use crate::{model::Source, settings::OverlaySettings};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod migration;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub source: Source,
    pub port: u16,
    pub cache_dir: PathBuf,
    pub kugou_cache_dir: PathBuf,
    pub overlay: OverlaySettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            source: Source::Auto,
            port: 38473,
            cache_dir: "D:/QQMusicCache/QQMusicLyricNew".into(),
            kugou_cache_dir: "D:/Kugou/Lyric".into(),
            overlay: OverlaySettings::default(),
        }
    }
}

pub fn data_root() -> PathBuf {
    std::env::var_os("HYSILENS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("Hysilens")
        })
}

impl Settings {
    pub fn load() -> anyhow::Result<Self> {
        let directory = data_root();
        let legacy = if std::env::var_os("HYSILENS_DATA_DIR").is_none() {
            directory.parent().map(|parent| parent.join("HysilensNext"))
        } else {
            None
        };
        migration::migrate(legacy.as_deref(), &directory)?;
        let path = data_root().join("settings.json");
        let settings: Self = if path.is_file() {
            serde_json::from_slice(&std::fs::read(path)?)?
        } else {
            Self::default()
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.version == 1 && self.port > 0, "配置版本或端口无效");
        anyhow::ensure!(
            !self.cache_dir.as_os_str().is_empty() && !self.kugou_cache_dir.as_os_str().is_empty(),
            "歌词缓存目录不能为空"
        );
        self.overlay.validate()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        static WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = WRITE
            .lock()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        self.validate()?;
        let directory = data_root();
        std::fs::create_dir_all(&directory)?;
        let temporary = directory.join("settings.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, directory.join("settings.json"))?;
        Ok(())
    }
}
