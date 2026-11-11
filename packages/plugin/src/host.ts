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

/** Resend a clock anchor once the plugin's projection drifts further than this. */
export const CLOCK_DRIFT_MS = 30;

/**
 * While time advances, refresh the anchor at least this often. Plugins stop extrapolating
 * after `MAX_EXTRAPOLATION_MS` (a lost-host guard), so an accurate but silent anchor would
 * otherwise freeze lyrics briefly every two seconds and then jump.
 */
export const CLOCK_HEARTBEAT_MS = 1_000;

export const READY_TIMEOUT_MS = 10_000;

export const DISPOSE_TIMEOUT_MS = 1_000;

export type HostPhase = 'idle' | 'connecting' | 'ready' | 'error' | 'disposing' | 'disposed';

export interface HostStatus {
  phase: HostPhase;
  /** Latest fatal error, or the most recent non-fatal report while still running. */
  error?: string;
}

export interface ConnectTarget {
  postMessage(message: ConnectMessage, targetOrigin: string, transfer: Transferable[]): void;
}

export interface PluginHostOptions {
  initial: InitPayload;
  now?: () => number;
  readyTimeoutMs?: number;
  disposeTimeoutMs?: number;
  onStatus?: (status: HostStatus) => void;
}

interface SentClock {
  anchor: ClockAnchor;
  at: number;
}

/**
 * Host side of one plugin instance: owns the port, keeps the latest state for the handshake,
 * and forwards only meaningful changes. One instance serves one iframe document.
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

  get status(): HostStatus {
    return this.#status;
  }

  /** Hand a fresh port to the plugin page. Later calls are ignored. */
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

  setConfig(config: Record<string, unknown>): void {
    if (config === this.#state.config) {
      return;
    }

    this.#state.config = config;
    this.#send({ kind: 'config', config });
  }

  setLyrics(document: LyricDocument | null): void {
    if (document === this.#state.lyrics) {
      return;
    }

    this.#state.lyrics = document;
    this.#send({ kind: 'lyrics', document });
  }

  setPlayback(playback: Playback): void {
    const current = this.#state.playback;

    if (current.playing === playback.playing && current.visible === playback.visible) {
      return;
    }

    this.#state.playback = { ...playback };
    this.#send({ kind: 'playback', ...playback });
  }

  /** Record the authoritative clock; transmit only when the plugin's projection would drift. */
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

  /** Ask the plugin to unmount; resolves after `disposed` or the timeout. */
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
 * Connect `host` to an iframe on whichever comes first: the frame's `load`, or the plugin page
 * announcing it is listening. Call before assigning `src`. Returns a detach function.
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
