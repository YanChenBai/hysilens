//! platform adapter 约定
//!
//! 受支持的播放器由 [`Descriptor`] 描述，包含 source、用于丢弃无关 session 的 session
//! 标识，以及歌词来源；基于 cache 的 platform 还要提供 [`CacheResolver`]
//!
//! adapter 由 [`Registration::create`] 在 capture thread 上惰性构建；该闭包可以把线程绑定的
//! 资源（例如 WinRT 对象和 process handle）留在返回的 adapter 内，因为它不会在别的线程运行
//! [`Adapter::sample`] 在同一线程上反复调用，应保持低成本
//!
//! 内存扫描只在存在匹配 playback session 时进行，失败后按 timer 重试，因此崩溃或重启中的
//! 播放器不会阻塞采样；cache 中的歌词从 catalog 解析，不接触播放器进程

pub mod diagnostics;
pub mod kugou;
pub mod qq_music;
pub mod wesing;

use crate::model::Catalog;
use crate::model::{Document, Playback, Source};
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};
use tokio::sync::mpsc;

/// platform 的歌词来源
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LyricsMode {
    /// 通过 [`CacheResolver`] 从本地 cache 目录读取歌词
    Cache,

    /// 随 playback 状态一起从播放器进程提取歌词
    Embedded,
}

/// 一个受支持播放器的静态描述
#[derive(Clone, Copy, Debug)]
pub struct Descriptor {
    /// 上报给 server 的 platform
    pub source: Source,

    /// 用于过滤 session 的播放器进程或媒体 session 标识，例如 `WeSing.exe`
    pub session_id: &'static str,

    /// 该 platform 的歌词来源
    pub lyrics: LyricsMode,

    /// cache 匹配器，仅当 `lyrics` 为 [`LyricsMode::Cache`] 时存在
    pub resolve_cache: Option<CacheResolver>,
}

/// 为 playback session 挑选一份 cache 文档
///
/// `key` 是客户端覆盖自动匹配时手动选中的 catalog entry key；实现通过
/// [`CatalogResolution::status`] 报告结果，当 cache 中没有唯一可信匹配时必须返回空文档
pub type CacheResolver = fn(&Catalog, &crate::model::Session, Option<&str>) -> CatalogResolution;

/// 一次 [`CacheResolver`] 查找的结果
#[derive(Debug)]
pub struct CatalogResolution {
    /// 要发布的文档；没有唯一可信匹配时为 `None`
    pub document: Option<Arc<Document>>,

    /// 所有可能属于该 session 的 entry，供客户端选择
    pub candidates: Vec<crate::model::Entry>,

    /// 上报给客户端的匹配结果，例如 `matchedMetadata`、`ambiguous`、`missing`
    pub status: &'static str,
}

/// 一次 adapter 轮询：playback 状态以及从播放器提取的可选歌词
#[derive(Debug)]
pub struct Sample {
    /// 要转发给 server 的 session 与错误状态
    pub playback: Playback,

    /// 本次轮询产出的歌词；platform 把歌词放在 cache 中时为 `None`
    pub document: Option<Arc<Document>>,
}

/// 每个 platform 的轮询器，只允许创建它的线程使用
pub trait Adapter {
    /// 返回当前 playback 状态
    ///
    /// 每个 tick 调用一次；实现必须保持低成本，播放器退出时不得 panic，
    /// 而应通过 [`Playback::error`] 报告该状态
    fn sample(&mut self) -> Sample;
}

/// 后台 cache 扫描器产出的 catalog 替换
#[derive(Debug)]
pub struct CatalogUpdate {
    /// 拥有该 cache 的 platform
    pub source: Source,

    /// 完整 catalog snapshot；替换 `source` 之前的 snapshot
    pub catalog: Catalog,
}

/// 交给 registration 工厂的 runtime
#[derive(Debug)]
pub struct Context {
    /// 作为每个 [`Sample`] 中 `server_ms` 零点的 Instant
    pub origin: Instant,

    /// 承载后台 cache 扫描产出的 catalog snapshot 的 channel
    pub catalogs: mpsc::Sender<CatalogUpdate>,

    /// capture thread 必须关闭时置位；adapter 应停止扫描
    pub stop: Arc<AtomicBool>,
}

/// 单个 platform 的工厂，存放在 registry 中
pub struct Registration {
    /// platform 的静态描述
    pub descriptor: Descriptor,

    /// 按需导出的说明与采集器；耗时工作只在独立 worker 上执行
    pub diagnostics: diagnostics::Diagnostics,

    /// 构建 adapter；运行在 capture thread 上，因此可以把 WinRT 对象和 process handle 之类
    /// 线程绑定的资源留在返回的 adapter 内
    pub create: Box<dyn FnOnce(Context) -> Box<dyn Adapter> + Send>,
}

impl std::fmt::Debug for Registration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Registration")
            .field("descriptor", &self.descriptor)
            .finish_non_exhaustive()
    }
}

/// 与 server 共享的单个 platform 缓存状态
#[derive(Clone, Debug)]
pub struct AdapterState {
    /// platform 的静态描述
    pub descriptor: Descriptor,

    /// 最近的歌词文档，在多次轮询间保留直到被替换
    pub document: Option<Arc<Document>>,
}

/// 所有内置 descriptor，顺序与 registry 测试一致
#[cfg(test)]
pub fn test_descriptors() -> Vec<Descriptor> {
    vec![wesing::DESCRIPTOR, qq_music::DESCRIPTOR, kugou::DESCRIPTOR]
}
