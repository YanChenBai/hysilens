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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LyricsMode {
    Cache,
    Embedded,
}

#[derive(Clone, Copy, Debug)]
pub struct Descriptor {
    pub source: Source,
    pub session_id: &'static str,
    pub lyrics: LyricsMode,
    pub resolve_cache: Option<CacheResolver>,
}

pub type CacheResolver = fn(&Catalog, &crate::model::Session, Option<&str>) -> CatalogResolution;

pub struct CatalogResolution {
    pub document: Option<Arc<Document>>,
    pub candidates: Vec<crate::model::Entry>,
    pub status: &'static str,
}

pub struct Sample {
    pub playback: Playback,
    pub document: Option<Arc<Document>>,
}

pub trait Adapter {
    fn sample(&mut self) -> Sample;
}

pub struct CatalogUpdate {
    pub source: Source,
    pub catalog: Catalog,
}

pub struct Context {
    pub origin: Instant,
    pub catalogs: mpsc::Sender<CatalogUpdate>,
    pub stop: Arc<AtomicBool>,
}

pub struct Registration {
    pub descriptor: Descriptor,
    // 工厂在采集线程内执行，WinRT 和进程句柄不跨线程搬运。
    pub create: Box<dyn FnOnce(Context) -> Box<dyn Adapter> + Send>,
}

#[derive(Clone)]
pub struct AdapterState {
    pub descriptor: Descriptor,
    pub document: Option<Arc<Document>>,
}

#[cfg(test)]
pub fn test_descriptors() -> Vec<Descriptor> {
    vec![wesing::DESCRIPTOR, qq_music::DESCRIPTOR, kugou::DESCRIPTOR]
}
