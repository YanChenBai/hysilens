import { project } from './clock';
import { API_VERSION, CONNECT_KIND, WAITING_KIND } from './protocol';
import type {
  ClockAnchor,
  ConnectMessage,
  HostMessage,
  InitPayload,
  LyricDocument,
  Playback,
  PluginMessage,
} from './protocol';

/**
 * 当 plugin 的投影偏差超过该值时就重发一次 clock anchor
 */
export const CLOCK_DRIFT_MS = 30;

/**
 * 时间推进期间，至少按该频率刷新 anchor
 * plugin 在超过 `MAX_EXTRAPOLATION_MS` 后停止外推（这是 host 失联的保护），
 * 所以准确但静默的 anchor 会每隔两秒让歌词短暂冻结再跳一下
 */
export const CLOCK_HEARTBEAT_MS = 1_000;

/**
 * 等待 `ready` 的最长时间，超时则让该实例失败，单位 ms
 */
export const READY_TIMEOUT_MS = 10_000;

/**
 * `dispose()` 等待 plugin 回复 `disposed` 的最长时间，超时后仍会关闭 port，单位 ms
 */
export const DISPOSE_TIMEOUT_MS = 1_000;

/**
 * 单个 host 实例的生命周期：`connect` 之前是 `idle`，之后是 `connecting`，
 * 直到 plugin 报告 `ready`
 * `error` 表示致命失败，仍可被 dispose；`disposed` 是终态
 */
export type HostPhase = 'idle' | 'connecting' | 'ready' | 'error' | 'disposing' | 'disposed';

/**
 * host 阶段与错误文本的快照，用于向 `onStatus` 汇报
 */
export interface HostStatus {
  phase: HostPhase;

  /**
   * 最近的致命错误；仍在运行则是最新一次非致命报告
   */
  error?: string;
}

/**
 * 握手中接收消息的一侧；`iframe.contentWindow` 即满足该接口
 */
export interface ConnectTarget {
  postMessage(message: ConnectMessage, targetOrigin: string, transfer: Transferable[]): void;
}

/**
 * 单个实例的接线方式；`initial` 提供握手状态，其余项调整时序与上报
 */
export interface PluginHostOptions {
  /**
   * 复制进 `init` 消息的状态；后续 setter 会替换该副本中的某个分片
   */
  initial: InitPayload;

  /**
   * 用于投影与 heartbeat 计时的单调时钟，单位 ms；默认 `performance.now`
   */
  now?: () => number;

  /**
   * 覆盖 `READY_TIMEOUT_MS`
   */
  readyTimeoutMs?: number;

  /**
   * 覆盖 `DISPOSE_TIMEOUT_MS`
   */
  disposeTimeoutMs?: number;

  /**
   * 每次状态变化时同步调用，包括 `error` 与 `disposed`
   */
  onStatus?: (status: HostStatus) => void;
}

interface SentClock {
  anchor: ClockAnchor;

  at: number;
}

/**
 * 单个 plugin 实例的 host 侧：持有 port、为握手保留最新状态，
 * 并且只转发真正有意义的变更；一个实例服务一个 iframe 文档
 */
export class PluginHost {
  #state: InitPayload;
  #status: HostStatus = { phase: 'idle' };
  #port: MessagePort | undefined;
  #sentClock: SentClock | undefined;
  #readyTimer: ReturnType<typeof setTimeout> | undefined;
  #disposed: Promise<void> | undefined;
  #resolveDisposed: (() => void) | undefined;
  readonly #now: () => number;
  readonly #options: PluginHostOptions;

  constructor(options: PluginHostOptions) {
    this.#options = options;
    this.#state = { ...options.initial };
    this.#now = options.now ?? (() => performance.now());
  }

  /**
   * 最新的状态对象；与传给 `onStatus` 的是同一个实例
   */
  get status(): HostStatus {
    return this.#status;
  }

  /**
   * 把一个新的 port 交给 plugin 页面；之后的调用会被忽略
   */
  connect(target: ConnectTarget): void {
    if (this.#status.phase !== 'idle') {
      return;
    }

    const channel = new MessageChannel();
    this.#port = channel.port1;
    this.#port.addEventListener('message', event => this.#receive(event.data as PluginMessage));
    this.#port.start();

    target.postMessage({ kind: CONNECT_KIND, apiVersion: API_VERSION }, '*', [channel.port2]);
    this.#setStatus({ phase: 'connecting' });

    this.#send({ kind: 'init', ...this.#state });
    this.#sentClock = { anchor: this.#state.clock, at: this.#now() };
    this.#readyTimer = setTimeout(
      () => this.#fail('插件未在规定时间内就绪'),
      this.#options.readyTimeoutMs ?? READY_TIMEOUT_MS,
    );
  }

  /**
   * 替换 config，除非传入的是同一个对象；plugin 会整体看到新对象
   */
  setConfig(config: Record<string, unknown>): void {
    if (config === this.#state.config) {
      return;
    }

    this.#state.config = config;
    this.#send({ kind: 'config', config });
  }

  /**
   * 替换歌词，除非传入的是同一个对象
   */
  setLyrics(document: LyricDocument | null): void {
    if (document === this.#state.lyrics) {
      return;
    }

    this.#state.lyrics = document;
    this.#send({ kind: 'lyrics', document });
  }

  /**
   * 仅在 `playing` 或 `visible` 确实变化时发送
   */
  setPlayback(playback: Playback): void {
    const current = this.#state.playback;

    if (current.playing === playback.playing && current.visible === playback.visible) {
      return;
    }

    this.#state.playback = { ...playback };
    this.#send({ kind: 'playback', ...playback });
  }

  /**
   * 记录权威 clock；仅当 plugin 的投影会出现偏差时才发送
   */
  syncClock(anchor: ClockAnchor): void {
    this.#state.clock = { ...anchor };

    if (!this.#port || !this.#isLive()) {
      return;
    }

    if (!this.#clockDrifted(anchor) && !this.#heartbeatDue(anchor)) {
      return;
    }

    this.#send({ kind: 'clock', ...anchor });
    this.#sentClock = { anchor: { ...anchor }, at: this.#now() };
  }

  /**
   * 请 plugin 卸载；在收到 `disposed` 或超时后 resolve
   */
  dispose(): Promise<void> {
    if (this.#disposed) {
      return this.#disposed;
    }

    clearTimeout(this.#readyTimer);

    if (!this.#port) {
      this.#setStatus({ phase: 'disposed' });
      this.#disposed = Promise.resolve();

      return this.#disposed;
    }

    this.#setStatus({ ...this.#status, phase: 'disposing' });

    this.#disposed = new Promise<void>(resolve => {
      const finish = () => {
        clearTimeout(timer);
        this.#port?.close();
        this.#port = undefined;
        this.#setStatus({ ...this.#status, phase: 'disposed' });
        resolve();
      };

      const timer = setTimeout(finish, this.#options.disposeTimeoutMs ?? DISPOSE_TIMEOUT_MS);
      this.#resolveDisposed = finish;
    });

    this.#port.postMessage({ kind: 'dispose' } satisfies HostMessage);

    return this.#disposed;
  }

  #heartbeatDue(anchor: ClockAnchor): boolean {
    const sent = this.#sentClock;

    return (
      anchor.rate !== 0 &&
      anchor.position !== null &&
      (!sent || this.#now() - sent.at >= CLOCK_HEARTBEAT_MS)
    );
  }

  #clockDrifted(anchor: ClockAnchor): boolean {
    const sent = this.#sentClock;

    if (!sent || sent.anchor.rate !== anchor.rate) {
      return true;
    }

    const predicted = project(sent.anchor, sent.at, this.#now());

    if (predicted === null || anchor.position === null) {
      return predicted !== anchor.position;
    }

    return Math.abs(predicted - anchor.position) > CLOCK_DRIFT_MS;
  }

  #isLive(): boolean {
    return this.#status.phase === 'connecting' || this.#status.phase === 'ready';
  }

  #send(message: HostMessage): void {
    if (this.#port && this.#isLive()) {
      this.#port.postMessage(message);
    }
  }

  #receive(message: PluginMessage): void {
    switch (message?.kind) {
      case 'ready': {
        clearTimeout(this.#readyTimer);

        if (this.#status.phase === 'connecting') {
          this.#setStatus({ phase: 'ready' });
        }

        break;
      }

      case 'error': {
        if (message.fatal) {
          this.#fail(message.message);
        } else {
          this.#setStatus({ ...this.#status, error: message.message });
        }

        break;
      }

      case 'disposed': {
        this.#resolveDisposed?.();
        break;
      }
    }
  }

  #fail(error: string): void {
    clearTimeout(this.#readyTimer);

    if (this.#isLive()) {
      this.#setStatus({ phase: 'error', error });
    }
  }

  #setStatus(status: HostStatus): void {
    this.#status = status;
    this.#options.onStatus?.(status);
  }
}

/**
 * 在 frame 的 `load` 或 plugin 页面宣告已开始监听这两者中较早的那个时刻，
 * 把 `host` 连接到该 iframe
 * 要在设置 `src` 之前调用，返回解绑函数
 */
export function attachFrame(host: PluginHost, frame: HTMLIFrameElement): () => void {
  const connect = () => {
    if (frame.contentWindow) {
      host.connect(frame.contentWindow);
    }
  };

  const onMessage = (event: MessageEvent) => {
    if (event.source === frame.contentWindow && event.data?.kind === WAITING_KIND) {
      connect();
    }
  };

  frame.addEventListener('load', connect);
  window.addEventListener('message', onMessage);

  return () => {
    frame.removeEventListener('load', connect);
    window.removeEventListener('message', onMessage);
  };
}

export type { ClockAnchor, InitPayload, LyricDocument, Playback } from './protocol';
