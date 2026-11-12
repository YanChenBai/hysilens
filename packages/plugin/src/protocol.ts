/**
 * Hysilens host 页面与 plugin iframe 之间的 wire 契约
 */

/**
 * 握手与 manifest 格式的版本；host 会拒绝按其他版本构建的 plugin
 */
export const API_VERSION = 1;

/**
 * 只通过 `window.postMessage` 发送一次；其余消息都走转移过去的 port
 */
export const CONNECT_KIND = 'hysilens:connect';

/**
 * plugin 页面开始监听后发送，使 host 即使在 `load` 之后也能连上
 */
export const WAITING_KIND = 'hysilens:waiting';

/**
 * 保留给随应用发布的 plugin 的命名空间；外部 plugin 不能使用
 */
export const BUILTIN_PREFIX = 'builtin:';

/**
 * 外部 plugin 为 `<name>`，内置 plugin 为 `builtin:<name>`
 */
export const PLUGIN_ID_PATTERN = /^(?:builtin:)?[a-zA-Z0-9_-]{1,80}$/;

/**
 * `id` 是否属于随应用发布的 plugin，而不是用户安装的 plugin
 */
export function isBuiltinId(id: string): boolean {
  return id.startsWith(BUILTIN_PREFIX);
}

/**
 * id 对应的目录名与文件名主干；`:` 在 Windows 上不是合法的路径字符
 */
export function localName(id: string): string {
  return isBuiltinId(id) ? id.slice(BUILTIN_PREFIX.length) : id;
}

/**
 * clock anchor 最多外推这么远，与 host 自身的播放模型保持一致
 */
export const MAX_EXTRAPOLATION_MS = 2000;

/**
 * 行内一个音节大小的区间，用于卡拉 OK 高亮
 */
export interface LyricToken {
  /**
   * 该区间覆盖的文本：一个字素，或一个词带其尾部空白，使 token 能拼回整行文本
   */
  text: string;

  /**
   * 在歌词时间线上的闭区间起点，单位 ms
   */
  startMs: number;

  /**
   * 在歌词时间线上的开区间终点，单位 ms；始终大于 `startMs`
   */
  endMs: number;
}

/**
 * 一行歌词，可选地拆成带计时的 token
 */
export interface LyricLine {
  /**
   * 在歌词时间线上的起点，单位 ms
   */
  startMs: number;

  /**
   * 在歌词时间线上的终点，单位 ms；来源没有给出终点时为 `null`，
   * 此时该行会一直保持当前状态直到下一行开始
   */
  endMs: number | null;

  /**
   * 整行文本；某些来源会把前奏空档保留为计时行，此时为空
   */
  text: string;

  /**
   * 当来源只给整行计时时为空
   */
  tokens: LyricToken[];
}

/**
 * 同一首歌的一种呈现，例如原文或其译文
 */
export interface LyricTrack {
  /**
   * track 角色：`original`、`romanization`、`translation` 或来源相关的标签
   */
  kind: string;

  /**
   * 按 `startMs` 排序；来源的行本身重叠时这里也会重叠
   */
  lines: LyricLine[];
}

/**
 * 单个文件的解析结果；所有时间都相对于该 track 的起点
 */
export interface LyricDocument {
  /**
   * 解析后文档的稳定标识；host 用它作为视图的 key
   */
  id: string;

  /**
   * 解析来源的文件名
   */
  filename: string;

  metadata: { title: string; artist: string; album: string };

  /**
   * 存在时 `original` 在最前面，之后是各个副轨
   */
  tracks: LyricTrack[];

  /**
   * 非致命的解析问题，例如被跳过的畸形时间戳
   */
  warnings: string[];
}

/**
 * host 从当前会话转发给 plugin 的播放状态
 */
export interface Playback {
  playing: boolean;

  /**
   * host 综合连接健康度与会话状态做出的判断
   */
  visible: boolean;
}

/**
 * 发送时刻的歌词时间，offset 已应用；暂停时 `rate` 为 0
 */
export interface ClockAnchor {
  /**
   * 歌词位置，单位 ms；host 没有可用位置时为 `null`
   */
  position: number | null;

  /**
   * 播放速率倍数；`0` 会冻结投影
   */
  rate: number;
}

/**
 * 第一条 host 消息携带的状态；之后的消息只替换其中的单个分片
 */
export interface InitPayload {
  /**
   * 校验通过且已套用默认值的 plugin config；之后变更时整体替换
   */
  config: Record<string, unknown>;

  /**
   * 当前会话的歌词；尚未加载时为 `null`
   */
  lyrics: LyricDocument | null;

  playback: Playback;

  /**
   * 握手时 host 最近一次已知的 anchor
   */
  clock: ClockAnchor;

  /**
   * 在 dev 预览中为 true，此时播放由 host UI 模拟
   */
  preview: boolean;
}

/**
 * host 到 plugin：`init` 在 port 转移后立即发送一次；其他 kind 都是增量更新
 */
export type HostMessage =
  | ({ kind: 'init' } & InitPayload)
  | { kind: 'config'; config: Record<string, unknown> }
  | { kind: 'lyrics'; document: LyricDocument | null }
  | ({ kind: 'playback' } & Playback)
  | ({ kind: 'clock' } & ClockAnchor)
  | { kind: 'dispose' };

/**
 * plugin 到 host：mounted hook 全部结束后发送一次 `ready`；`fatal` 错误会结束该实例，
 * 非致命错误只上报并继续播放
 */
export type PluginMessage =
  | { kind: 'ready' }
  | { kind: 'error'; message: string; fatal: boolean }
  | { kind: 'disposed' };

/**
 * window 级消息，在 transfer 列表中携带 port
 */
export interface ConnectMessage {
  kind: typeof CONNECT_KIND;

  apiVersion: typeof API_VERSION;
}

/**
 * overlay 表面的期望尺寸；desktop host 在 plugin ready 后请求它，
 * manifest 未提供时回退到 960×320
 */
export interface PluginViewport {
  /**
   * 逻辑像素宽度，必须是正整数
   */
  width: number;

  height: number;
}

/**
 * 可选启用的能力；每个启用的标志都会注入一个由 host 应用的对应 config 字段
 */
export interface PluginFeatures {
  /**
   * host 侧缩放，归一化为 `1`；plugin 永远不会收到它
   */
  scale?: boolean;

  /**
   * 注入 `fontFamily`；对象形式用于列出候选字体族并设定初始值
   */
  font?: boolean | { families?: string[]; default?: string };

  /**
   * 注入 `revealMode`，在逐词与逐行显示之间选择
   */
  reveal?: boolean;

  romanization?: boolean;

  translation?: boolean;
}

/**
 * `dist/hysilens.json`，host 发现 plugin 时读取的唯一文件
 */
export interface PluginManifest {
  apiVersion: typeof API_VERSION;

  /**
   * 必须匹配 `PLUGIN_ID_PATTERN`，并与安装目录名一致
   */
  id: string;

  name: string;

  version: string;

  description?: string;

  /**
   * 入口模块，相对 plugin 根目录，且始终是 `.js` 文件
   */
  entry: string;

  /**
   * 入口 import 的样式表，相对 plugin 根目录；随 frame 一起加载
   */
  styles: string[];

  viewport?: PluginViewport;

  features: PluginFeatures;

  /**
   * 存储配置的 Draft 2020-12 JSON Schema，包含注入的 feature 字段
   */
  configSchema: Record<string, unknown>;

  /**
   * 已发布文件的指纹；manifest 不是由构建产出时为空
   */
  revision: string;
}

/**
 * `value` 是否为使用本 plugin API 版本的 host 发来的握手
 */
export function isConnectMessage(value: unknown): value is ConnectMessage {
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as ConnectMessage).kind === CONNECT_KIND &&
    (value as ConnectMessage).apiVersion === API_VERSION
  );
}

/**
 * 为捕获到的值生成尽力而为的消息文本，该值可能是 `Error`、字符串或其他任何东西
 */
export function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message || error.name;
  }

  return typeof error === 'string' ? error : (JSON.stringify(error) ?? String(error));
}
