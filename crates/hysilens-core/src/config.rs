//! 持久化的应用 settings 及其所在的数据目录

use crate::{model::Source, settings::OverlaySettings};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

mod migration;

/// 存放在数据根目录下 `settings.json` 中的 settings
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    // 文件 schema
    /// 写入磁盘的 schema 版本；只接受 `1`
    pub version: u32,

    // capture
    /// 启动时 overlay 跟随的平台
    pub source: Source,

    /// QQ 音乐歌词 cache 目录
    pub cache_dir: PathBuf,

    /// 酷狗歌词 cache 目录
    pub kugou_cache_dir: PathBuf,

    /// 酷狗增强模式：优先通过酷狗调试端口读取播放信息，失败时回退到内存读取
    pub kugou_cdp: bool,

    // server
    /// overlay server 监听的 loopback HTTP 端口
    pub port: u16,

    // 渲染
    /// 与 web UI 共享的外观 settings
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
            kugou_cdp: false,
            overlay: OverlaySettings::default(),
        }
    }
}

/// 存放 settings、plugin config 与用户安装 plugin 的目录
///
/// `HYSILENS_DATA_DIR` 会覆盖默认值；否则使用平台的本地应用数据目录，
/// 再退回到系统临时目录
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
    /// 从数据根目录加载 settings，先导入旧安装布局中的数据
    ///
    /// # Errors
    /// 数据目录无法迁移、文件无法读取或解析，或结果未通过 [`Settings::validate`] 时失败
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

    /// 拒绝 server 无法运行的 settings
    ///
    /// # Errors
    /// 版本不是 `1`、端口为 `0`、某个 cache 目录为空，或 overlay settings 无效时失败
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.version == 1 && self.port > 0, "配置版本或端口无效");

        anyhow::ensure!(
            !self.cache_dir.as_os_str().is_empty() && !self.kugou_cache_dir.as_os_str().is_empty(),
            "歌词缓存目录不能为空"
        );

        self.overlay.validate()
    }

    /// 把 settings 写入 `settings.json`，原子地替换之前的文件
    ///
    /// # Errors
    /// settings 无效、数据目录无法创建，或文件无法序列化、无法改名到位时失败
    pub fn save(&self) -> anyhow::Result<()> {
        // 串行化写入，避免并发的 save 交错写临时文件
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
