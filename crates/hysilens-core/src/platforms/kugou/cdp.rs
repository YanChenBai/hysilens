//! 通过酷狗内嵌网页的调试端口读取播放信息（增强模式）
//!
//! 酷狗以 `--remote_debug_port` 启动后，会在回环地址上暴露 Chrome DevTools Protocol；这里只连接
//! `pc.service.kugou.com` 页面，并只调用页面自身 SDK 使用的只读接口 `external.SuperCall(864)`
//! 读取歌曲 hash、进度与时长；不读取 cookie，不改写页面全局对象，也不调用任何有副作用的接口
//!
//! CEF 不支持 `Target.createTarget`，因此只能复用酷狗已经打开的页面：推荐页优先，其次是
//! 常驻的弹窗等其他页面；当前页面关闭后自动换用下一个可用页面

use anyhow::{Context, bail, ensure};
use serde::Deserialize;
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    sync::Mutex,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket};

/// 增强模式使用的调试端口
pub const PORT: u16 = 9225;

/// 以增强模式启动酷狗所需的参数；监听地址固定为回环地址
pub const LAUNCH_ARGS: [&str; 2] = [
    "--remote_debug_port=9225",
    "--remote-debugging-address=127.0.0.1",
];

const HOST: &str = "pc.service.kugou.com";

const TIMEOUT: Duration = Duration::from_millis(1500);

/// 在页面内调用 `864` 并把回调结果统一转成 JSON 字符串；回调函数用完即删
const PLAY_INFO: &str = r#"new Promise((resolve, reject) => {
  if (typeof external !== 'object' || typeof external.SuperCall === 'undefined') {
    reject(new Error('页面没有酷狗原生接口'));
    return;
  }
  const name = '__hysilens_' + Math.random().toString(36).slice(2);
  const timer = setTimeout(() => { delete window[name]; reject(new Error('酷狗播放信息超时')); }, 1000);
  window[name] = value => {
    clearTimeout(timer);
    delete window[name];
    resolve(typeof value === 'string' ? value : JSON.stringify(value));
  };
  external.SuperCall(864, JSON.stringify({ callback: name }));
})"#;

/// `864` 返回的当前播放信息
#[derive(Clone, Debug, PartialEq)]
pub struct PlayInfo {
    /// 客户端的播放状态原值，例如 `playing`
    pub play_status: String,

    /// "歌手 - 歌名"
    pub filename: String,

    /// 当前播放资源的 hash
    pub hash: String,

    /// 原始音频 hash；与本地 KRC 文件名中的 hash 一致
    pub origin_hash: String,

    /// 酷狗曲库中的歌曲 ID
    pub mix_song_id: String,

    /// 当前播放位置，单位毫秒
    pub position_ms: f64,

    /// 曲目总时长，单位毫秒；缺失或与进度矛盾时为 `None`
    pub duration_ms: Option<f64>,
}

impl PlayInfo {
    /// 与 SMTC 状态取值对齐的播放状态；无法识别时返回 `None`
    pub fn status(&self) -> Option<&'static str> {
        let status = self.play_status.to_ascii_lowercase();

        if status == "playing" || status == "play" {
            Some("playing")
        } else if status.contains("pause") {
            Some("paused")
        } else if status.contains("stop") {
            Some("stopped")
        } else {
            None
        }
    }

    /// 系统媒体会话缺失时，用播放信息建立的酷狗会话；时间轴由调用方填入
    pub(crate) fn session(&self) -> crate::model::Session {
        let (artist, title) = self
            .filename
            .split_once(" - ")
            .map_or(("", self.filename.as_str()), |(artist, title)| {
                (artist.trim(), title.trim())
            });

        crate::model::Session {
            source_id: "kugou".into(),
            title: title.into(),
            artist: artist.into(),
            status: self.status().unwrap_or("playing").into(),
            ..Default::default()
        }
    }

    /// 同一首歌稳定不变的不透明标识
    pub fn track_id(&self) -> String {
        format!(
            "kugou:cdp:{}:{}:{}",
            self.mix_song_id, self.origin_hash, self.hash
        )
    }

    /// 用于按 hash 匹配本地 KRC 的身份
    pub(crate) fn lyric_match(&self) -> crate::model::LyricMatch {
        crate::model::LyricMatch {
            file_hashes: [&self.origin_hash, &self.hash]
                .into_iter()
                .filter(|hash| !hash.is_empty())
                .cloned()
                .collect(),
            preferred_lyric_id: None,
        }
    }
}

#[derive(Deserialize)]
struct RawPlayInfo {
    #[serde(default)]
    status: String,

    #[serde(default)]
    play_status: String,

    #[serde(default)]
    filename: String,

    #[serde(default)]
    hash: String,

    #[serde(default)]
    origin_hash: String,

    #[serde(default)]
    mix_song_id: String,

    #[serde(default)]
    duration: String,

    #[serde(default)]
    progress: String,
}

/// 解析 `864` 的回调值
///
/// `progress` 的单位是 100 ns，`duration` 的单位是毫秒；两者都以字符串上报
pub fn parse_play_info(text: &str) -> anyhow::Result<PlayInfo> {
    let raw: RawPlayInfo = serde_json::from_str(text).context("酷狗播放信息不是有效 JSON")?;

    ensure!(raw.status == "1", "酷狗当前没有可用的播放信息");

    let progress: f64 = raw.progress.trim().parse().context("酷狗播放进度无效")?;
    let position_ms = progress / 10_000.0;

    ensure!(
        position_ms.is_finite() && (0.0..=86_400_000.0).contains(&position_ms),
        "酷狗播放进度超出范围 ({position_ms} ms)"
    );

    // 时长缺失或异常不影响进度；按内存方案的约定发布为 None
    let duration_ms = raw
        .duration
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|duration| duration.is_finite() && *duration > 0.0 && *duration <= 86_400_000.0)
        .filter(|duration| *duration + 2000.0 >= position_ms);

    Ok(PlayInfo {
        play_status: raw.play_status,
        filename: raw.filename,
        hash: raw.hash.to_ascii_lowercase(),
        origin_hash: raw.origin_hash.to_ascii_lowercase(),
        mix_song_id: raw.mix_song_id,
        position_ms,
        duration_ms,
    })
}

#[derive(Deserialize)]
struct Target {
    #[serde(rename = "type")]
    kind: String,

    url: String,

    #[serde(rename = "webSocketDebuggerUrl", default)]
    socket: String,
}

/// 酷狗页面在候选中的优先级；非酷狗页面返回 `None`
fn priority(url: &str) -> Option<u8> {
    let rest = url.strip_prefix("https://")?.strip_prefix(HOST)?;

    if !rest.starts_with('/') {
        return None;
    }

    Some(if rest.starts_with("/yueku/") {
        0
    } else if rest.starts_with("/search/") {
        1
    } else {
        2
    })
}

/// 按优先级排列可连接的酷狗页面；只接受指向本机同一端口的调试地址
fn candidates(list: &str, port: u16) -> anyhow::Result<Vec<String>> {
    let targets: Vec<Target> = serde_json::from_str(list).context("调试端口返回的页面列表无效")?;
    let prefix = format!("ws://127.0.0.1:{port}/devtools/page/");
    let mut pages: Vec<_> = targets
        .into_iter()
        .filter(|target| target.kind == "page" && target.socket.starts_with(&prefix))
        .filter_map(|target| priority(&target.url).map(|rank| (rank, target.socket)))
        .collect();

    pages.sort_by_key(|(rank, _)| *rank);

    Ok(pages.into_iter().map(|(_, socket)| socket).collect())
}

fn address(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

/// 调试端口是否在本机监听
pub fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(&address(port), Duration::from_millis(200)).is_ok()
}

fn http_get(port: u16, path: &str) -> anyhow::Result<String> {
    let mut stream =
        TcpStream::connect_timeout(&address(port), TIMEOUT).context("酷狗调试端口未开启")?;

    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;

    read_response(&mut stream).context("读取酷狗页面列表失败")
}

/// 读取一个带 `Content-Length` 的 HTTP 响应体
///
/// DevTools 的 HTTP 服务不会在响应后关闭连接，因此不能读到 EOF
fn read_response(stream: &mut impl Read) -> anyhow::Result<String> {
    const LIMIT: usize = 4 * 1024 * 1024;

    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }

        ensure!(buffer.len() < 64 * 1024, "酷狗调试端口响应头过长");

        let read = stream.read(&mut chunk)?;
        ensure!(read > 0, "酷狗调试端口提前关闭连接");
        buffer.extend_from_slice(&chunk[..read]);
    };

    let head = std::str::from_utf8(&buffer[..header_end]).context("酷狗调试端口响应头无效")?;

    ensure!(
        head.starts_with("HTTP/1.1 200") || head.starts_with("HTTP/1.0 200"),
        "酷狗调试端口响应异常"
    );

    let length: usize = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .context("酷狗调试端口响应缺少长度")?;

    ensure!(length <= LIMIT, "酷狗页面列表过大");

    while buffer.len() < header_end + length {
        let read = stream.read(&mut chunk)?;
        ensure!(read > 0, "酷狗调试端口提前关闭连接");
        buffer.extend_from_slice(&chunk[..read]);
    }

    String::from_utf8(buffer[header_end..header_end + length].to_vec())
        .context("酷狗页面列表编码无效")
}

/// 到某个酷狗页面的 DevTools 连接
struct Page {
    socket: WebSocket<TcpStream>,

    next_id: u64,
}

impl Page {
    fn connect(url: &str, port: u16) -> anyhow::Result<Self> {
        let stream = TcpStream::connect_timeout(&address(port), TIMEOUT)?;

        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.set_write_timeout(Some(TIMEOUT))?;
        stream.set_nodelay(true)?;

        let (socket, _) = tungstenite::client(url, stream).context("连接酷狗页面失败")?;

        Ok(Self { socket, next_id: 0 })
    }

    fn play_info(&mut self) -> anyhow::Result<PlayInfo> {
        self.next_id += 1;

        let id = self.next_id;
        let request = serde_json::json!({
            "id": id,
            "method": "Runtime.evaluate",
            "params": {
                "expression": PLAY_INFO,
                "awaitPromise": true,
                "returnByValue": true,
                "timeout": 1000,
            },
        });

        self.socket
            .send(Message::text(request.to_string()))
            .context("发送酷狗页面请求失败")?;

        let deadline = Instant::now() + TIMEOUT;

        loop {
            ensure!(Instant::now() < deadline, "酷狗页面响应超时");

            let message = self.socket.read().context("读取酷狗页面响应失败")?;
            let Message::Text(text) = message else {
                continue;
            };
            let response: serde_json::Value = serde_json::from_str(&text)?;

            // 没有启用任何事件域，但仍跳过可能出现的事件或其他响应
            if response["id"].as_u64() != Some(id) {
                continue;
            }

            if let Some(error) = response.get("error") {
                bail!("酷狗页面拒绝请求: {error}");
            }

            let result = &response["result"];

            if let Some(details) = result.get("exceptionDetails") {
                let message = details["exception"]["description"]
                    .as_str()
                    .or_else(|| details["text"].as_str())
                    .unwrap_or("未知错误");
                bail!("{message}");
            }

            let value = result["result"]["value"]
                .as_str()
                .context("酷狗播放信息为空")?;

            return parse_play_info(value);
        }
    }
}

/// 最近一次增强模式采样的结果，供设置页显示
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// 最近一次采样是否成功
    pub connected: bool,

    /// 最近一次采样失败的原因
    pub error: Option<String>,
}

static STATUS: Mutex<Option<(Status, Instant)>> = Mutex::new(None);

fn publish(status: Status) {
    if let Ok(mut current) = STATUS.lock() {
        *current = Some((status, Instant::now()));
    }
}

/// 增强模式的最近状态；超过三秒没有采样视为未连接
pub fn status() -> Status {
    STATUS
        .lock()
        .ok()
        .and_then(|current| current.clone())
        .filter(|(_, at)| at.elapsed() < Duration::from_secs(3))
        .map(|(status, _)| status)
        .unwrap_or_default()
}

/// 带断线重连的增强模式通道
pub struct Channel {
    port: u16,

    page: Option<Page>,

    last_attempt: Option<Instant>,

    error: Option<String>,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Channel")
            .field("port", &self.port)
            .field("connected", &self.page.is_some())
            .field("error", &self.error)
            .finish()
    }
}

impl Channel {
    /// 创建连接到本机 `port` 的通道；首次采样时才会连接
    pub fn new(port: u16) -> Self {
        Self {
            port,
            page: None,
            last_attempt: None,
            error: None,
        }
    }

    /// 读取一次播放信息；连接断开时最多每两秒重新选择页面
    pub fn sample(&mut self) -> anyhow::Result<PlayInfo> {
        let result = self.read();

        publish(Status {
            connected: result.is_ok(),
            error: result.as_ref().err().map(|error| format!("{error:#}")),
        });

        result
    }

    fn read(&mut self) -> anyhow::Result<PlayInfo> {
        if let Some(page) = &mut self.page {
            match page.play_info() {
                Ok(info) => return Ok(info),
                Err(error) => {
                    self.page = None;
                    self.error = Some(format!("{error:#}"));
                }
            }
        }

        if self
            .last_attempt
            .is_some_and(|last| last.elapsed() < Duration::from_secs(2))
        {
            bail!(
                "{}",
                self.error.as_deref().unwrap_or("正在等待酷狗增强模式")
            );
        }

        self.last_attempt = Some(Instant::now());

        match self.attach() {
            Ok(info) => {
                self.error = None;
                Ok(info)
            }
            Err(error) => {
                self.error = Some(format!("{error:#}"));
                Err(error)
            }
        }
    }

    /// 依次尝试每个酷狗页面，使用第一个能返回播放信息的页面
    fn attach(&mut self) -> anyhow::Result<PlayInfo> {
        let list = http_get(self.port, "/json/list")?;
        let pages = candidates(&list, self.port)?;

        ensure!(
            !pages.is_empty(),
            "酷狗没有可连接的内嵌页面，请打开酷狗主界面"
        );

        let mut last = None;

        for url in pages {
            let attempt = Page::connect(&url, self.port).and_then(|mut page| {
                let info = page.play_info()?;
                Ok((page, info))
            });

            match attempt {
                Ok((page, info)) => {
                    self.page = Some(page);
                    return Ok(info);
                }
                Err(error) => last = Some(error),
            }
        }

        Err(last.unwrap_or_else(|| anyhow::anyhow!("酷狗页面不可用")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-12 在酷狗 20.1.51 上实测的 864 回调（截取）
    const SAMPLE: &str = r#"{"QueueSource":"{}","album_id":"1961713","duration":"227709","filename":"卫兰 - 验伤","hash":"C17361632E968AE3D3F820E3CB88B0AF","mix_song_id":"53304519","origin_hash":"83141bebb8506c8d2c591258dac45feb","play_status":"playing","progress":"2039560431","status":"1","type":"song"}"#;

    #[test]
    #[ignore = "需要以增强模式运行的酷狗"]
    fn reads_running_kugou() {
        let mut channel = Channel::new(PORT);
        let first = channel.sample().unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let second = channel.sample().unwrap();

        println!("{first:?}\n{second:?}");
        assert!(!first.origin_hash.is_empty());
        assert!(status().connected);
    }

    #[test]
    fn reads_body_by_content_length_without_waiting_for_close() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length:5\r\nContent-Type: application/json\r\n\r\n[1,2]trailing";
        assert_eq!(read_response(&mut &response[..]).unwrap(), "[1,2]");

        assert!(
            read_response(&mut &b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"[..])
                .is_err()
        );
        assert!(read_response(&mut &b"HTTP/1.1 200 OK\r\n\r\n"[..]).is_err());
        assert!(
            read_response(&mut &b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n[1]"[..]).is_err()
        );
    }

    #[test]
    fn parses_play_info_units_and_identity() {
        let info = parse_play_info(SAMPLE).unwrap();

        assert_eq!(info.position_ms, 203956.0431);
        assert_eq!(info.duration_ms, Some(227709.0));
        assert_eq!(info.status(), Some("playing"));
        assert_eq!(info.hash, "c17361632e968ae3d3f820e3cb88b0af");
        assert_eq!(
            info.lyric_match().file_hashes,
            [
                "83141bebb8506c8d2c591258dac45feb",
                "c17361632e968ae3d3f820e3cb88b0af"
            ]
        );
        assert_eq!(
            info.track_id(),
            "kugou:cdp:53304519:83141bebb8506c8d2c591258dac45feb:c17361632e968ae3d3f820e3cb88b0af"
        );
    }

    #[test]
    fn builds_session_from_filename() {
        let session = parse_play_info(SAMPLE).unwrap().session();

        assert_eq!(session.source_id, "kugou");
        assert_eq!(session.artist, "卫兰");
        assert_eq!(session.title, "验伤");
        assert_eq!(session.status, "playing");

        let untitled = SAMPLE.replace("卫兰 - 验伤", "纯音乐");
        let session = parse_play_info(&untitled).unwrap().session();
        assert_eq!(
            (session.artist.as_str(), session.title.as_str()),
            ("", "纯音乐")
        );
    }

    #[test]
    fn keeps_progress_when_duration_is_missing_or_shorter() {
        let missing = SAMPLE.replace(r#""duration":"227709""#, r#""duration":"""#);
        assert_eq!(parse_play_info(&missing).unwrap().duration_ms, None);

        let short = SAMPLE.replace(r#""duration":"227709""#, r#""duration":"528""#);
        let info = parse_play_info(&short).unwrap();
        assert_eq!(info.duration_ms, None);
        assert_eq!(info.position_ms, 203956.0431);
    }

    #[test]
    fn rejects_unavailable_or_invalid_progress() {
        assert!(parse_play_info(&SAMPLE.replace(r#""status":"1""#, r#""status":"0""#)).is_err());
        assert!(
            parse_play_info(&SAMPLE.replace(r#""progress":"2039560431""#, r#""progress":"-1""#))
                .is_err()
        );
        assert!(
            parse_play_info(&SAMPLE.replace(r#""progress":"2039560431""#, r#""progress":"x""#))
                .is_err()
        );
        assert!(parse_play_info("not json").is_err());
    }

    #[test]
    fn maps_play_status() {
        let status = |value: &str| {
            parse_play_info(&SAMPLE.replace("\"playing\"", &format!("\"{value}\"")))
                .unwrap()
                .status()
        };

        assert_eq!(status("playing"), Some("playing"));
        assert_eq!(status("paused"), Some("paused"));
        assert_eq!(status("stopped"), Some("stopped"));
        assert_eq!(status("unknown"), None);
    }

    #[test]
    fn orders_kugou_pages_and_rejects_foreign_targets() {
        let list = r#"[
          {"type":"page","url":"https://pc.service.kugou.com/apps/pcwv-desktop-popup/dist/index.html#/boot_ad","webSocketDebuggerUrl":"ws://127.0.0.1:9225/devtools/page/POPUP"},
          {"type":"page","url":"https://pc.service.kugou.com/yueku/2024/find/index.html#/recommend","webSocketDebuggerUrl":"ws://127.0.0.1:9225/devtools/page/RECOMMEND"},
          {"type":"page","url":"https://pc.service.kugou.com.evil.example/yueku/","webSocketDebuggerUrl":"ws://127.0.0.1:9225/devtools/page/EVIL"},
          {"type":"page","url":"https://example.com/","webSocketDebuggerUrl":"ws://127.0.0.1:9225/devtools/page/OTHER"},
          {"type":"service_worker","url":"https://pc.service.kugou.com/sw.js","webSocketDebuggerUrl":"ws://127.0.0.1:9225/devtools/page/SW"},
          {"type":"page","url":"https://pc.service.kugou.com/search/2024/","webSocketDebuggerUrl":"ws://192.168.1.2:9225/devtools/page/REMOTE"}
        ]"#;

        assert_eq!(
            candidates(list, 9225).unwrap(),
            [
                "ws://127.0.0.1:9225/devtools/page/RECOMMEND",
                "ws://127.0.0.1:9225/devtools/page/POPUP"
            ]
        );
    }
}
