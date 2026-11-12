import type { OverlayOptions } from './config';

/**
 * desktop UI 与本地 overlay server 之间的 wire 协议
 */

/**
 * 要跟随的平台；`auto` 跟随正在播放的那个平台
 */
export type PlaybackSource = 'auto' | 'wesing' | 'qqMusic' | 'kugou';

/**
 * 状态与命令形状的版本；带其他值的 snapshot 会被客户端拒绝
 */
export const PROTOCOL_VERSION = 1;

/**
 * UI 通过 event socket 发送的命令
 */
export type ClientMessage =
  /**
   * 替换 overlay 的外观设置；校验失败时按单条消息拒绝
   */
  | { kind: 'overlay'; settings: OverlayOptions }
  /**
   * 改为跟随另一平台；`auto` 恢复自动选择
   */
  | { kind: 'source'; value: PlaybackSource }
  /**
   * 用于估算 client/server 偏差的时钟探测，`clientMs` 单位 ms
   */
  | { kind: 'clock'; clientMs: number }
  /**
   * 为当前歌曲挑一条 cache 条目；`generation` 必须与当前值一致，否则选择被拒绝，
   * `key: null` 让平台自动选择
   */
  | { kind: 'select'; generation: number; key: string | null }
  /**
   * 设置歌词偏移，单位 ms，由 server 限制在 `-10000..10000`
   */
  | { kind: 'offset'; value: number }
  /**
   * 切换可选的歌词呈现
   */
  | { kind: 'display'; romanization: boolean; translation: boolean };

/**
 * adapter 采样到的一个播放器窗口
 * 时间线取值的单位是毫秒，在播放器上报之前保持 `null`
 */
export type Session = {
  /**
   * 产生该采样值的 adapter
   */
  sourceId: string;

  title: string;

  artist: string;

  album: string;

  /**
   * 播放器状态，例如 `playing`、`paused`、`opened`、`closed` 或 `stopped`
   */
  status: string;

  /**
   * 当前播放位置
   */
  positionMs: number | null;

  /**
   * 曲目总时长；也用于区分不同的 cache 条目
   */
  durationMs: number | null;

  /**
   * 平台上报的原始位置，尚未经过客户端时钟平滑
   */
  sourcePositionMs: number | null;

  /**
   * 采样 `sourcePositionMs` 时的 Unix 时间，单位 ms
   */
  sourceUpdatedUnixMs: number | null;

  /**
   * 客户端对流逝时间应用的速率倍数；`1` 为正常播放
   */
  rate: number | null;

  /**
   * 记录 `rate` 来源的自由格式标签
   */
  rateBasis: string;

  /**
   * 最近一次采样失败的原因，若有
   */
  error: string | null;

  /**
   * 不透明的来源曲目标识；只做相等比较，不要解析
   */
  trackId?: string;
};

export type { LyricLine, LyricDocument } from '@hysilens/plugin';
import type { LyricDocument } from '@hysilens/plugin';

/**
 * 平台 cache 中的一个歌词文件，列给手动选择器使用
 */
export type CacheEntry = {
  /**
   * 随 `select` 命令回传的 cache key
   */
  key: string;

  /**
   * cache 目录内的文件名
   */
  filename: string;

  title: string;

  artist: string;

  album: string;

  /**
   * 歌词行数，在选择器中展示
   */
  lineCount: number;
};

/**
 * 一次发布时 overlay server 的完整状态
 */
export type Snapshot = {
  /**
   * overlay 渲染所用的外观设置；server 发布之前不存在
   */
  overlay?: OverlayOptions;

  /**
   * 正在跟随的平台；`auto` 跟随正在播放的平台
   */
  source: PlaybackSource;

  /**
   * 对本次构建而言始终是 `PROTOCOL_VERSION`
   */
  version: number;

  /**
   * 服务进程的不透明标识，由其 pid 与启动时间推导
   */
  epoch: string;

  /**
   * 发布计数器；客户端忽略更小的值
   */
  seq: number;

  /**
   * 正在播放的曲目变化时递增；`select` 必须回传当前值
   */
  generation: number;

  /**
   * 每个 adapter 的最新采样，均取同一 server 时钟
   */
  playback: { serverMs: number; sessions: Session[]; error: string | null };

  /**
   * 从 `playback` 中为 `source` 选出的会话
   */
  session: Session | null;

  /**
   * 歌词的解析状态，例如 `waiting`、`loading`、`missing`、`ambiguous`、
   * `manual`、`matchedMetadata` 或 `captureError`
   */
  lyricsStatus: string;

  /**
   * 可能与 `session` 匹配的 cache 条目，供手动选择
   */
  candidates: CacheEntry[];

  /**
   * `session` 解析后的歌词；歌词解析完成前为 `null`
   */
  document: LyricDocument | null;

  /**
   * 应用到时间线的歌词偏移，单位 ms（`-10000..10000`）
   */
  offsetMs: number;

  /**
   * overlay 在可用时是否绘制 romanization 呈现
   */
  showRomanization: boolean;

  /**
   * overlay 在可用时是否绘制 translation 呈现
   */
  showTranslation: boolean;

  /**
   * 扫描 cache 目录时收集到的错误
   */
  cacheIssues: string[];
};

/**
 * server 通过 event socket 推送的消息
 */
export type ServerMessage =
  /**
   * 新发布的 snapshot；`document` 未变化时省略
   */
  | { kind: 'state'; state: Omit<Snapshot, 'document'> & { document?: LyricDocument | null } }
  /**
   * 对时钟探测的回复：`clientMs` 原样回传，`serverMs` 是发送时的 server 时钟
   */
  | { kind: 'clock'; clientMs: number; serverMs: number }
  /**
   * 某条命令被拒绝；文本会展示给用户
   */
  | { kind: 'error'; message: string }
  /**
   * plugin 目录发生变化；重新请求 `/api/plugins`
   */
  | { kind: 'plugins'; revision: number }
  /**
   * 某个 plugin 已保存的 config 发生变化；重新请求 `/api/plugins/{id}/config`
   */
  | { kind: 'pluginConfig'; id: string };

/**
 * 持久化的应用设置，存放在数据根目录下的 `settings.json`
 */
export interface AppSettings {
  /**
   * 写入磁盘的 schema 版本；只接受 `1`
   */
  version: number;

  /**
   * 启动时 overlay 跟随的平台
   */
  source: PlaybackSource;

  /**
   * overlay server 监听的回环 HTTP port
   */
  port: number;

  /**
   * QQ 音乐歌词 cache 目录
   */
  cacheDir: string;

  /**
   * 酷狗歌词 cache 目录
   */
  kugouCacheDir: string;

  /**
   * 酷狗增强模式：优先通过酷狗调试端口读取播放信息，失败时回退到内存读取
   */
  kugouCdp: boolean;

  /**
   * 与 overlay UI 共用的外观设置
   */
  overlay: OverlayOptions;
}
