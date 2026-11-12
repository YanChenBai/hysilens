//! Hysilens 的进程内 capture 与 overlay 服务
//!
//! 本 crate 通过 platform adapter 采样正在运行的音乐播放器，解析当前歌曲的歌词，
//! 通过 IPC 为桌面主窗口提供控制与状态，通过 HTTP/WebSocket 为歌词视图提供资源和状态
//! 所有内容都运行在桌面进程内部：没有独立 daemon，capture thread、HTTP server
//! 和 plugin watcher 共享同一个生命周期

mod capture;
pub mod config;
mod coordinator;
mod fonts;
pub mod model;
pub mod platforms;
pub mod plugin_registry;
pub mod plugins;
pub mod protocol;
pub mod server;
pub mod settings;
