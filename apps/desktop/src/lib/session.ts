import type { ClientMessage, ServerMessage, Snapshot } from '@hysilens/protocol';
import { parseOverlayConfig } from '@hysilens/protocol/config';
import { useEffect, useSyncExternalStore } from 'react';

import { isControlPage, sendControl, subscribeControl } from './control';

export interface SessionState {
  /**
   * 服务端最新的 snapshot，首次更新前为 null
   */
  snapshot: Snapshot | null;

  connection: 'connecting' | 'connected' | 'disconnected';

  /**
   * socket 或服务端上报的最近一次错误；下一个 snapshot 会清空它
   */
  error: string;

  /**
   * 最近一次 snapshot 的 `performance.now()` 时间戳，单位毫秒
   */
  lastStateAt: number;

  /**
   * 服务端 clock 减本地 clock，单位毫秒；未测出往返时间前为 null
   */
  clockOffset: number | null;
}

let state: SessionState = {
  snapshot: null,
  connection: 'disconnected',
  error: '',
  lastStateAt: 0,
  clockOffset: null,
};

/**
 * `resync`：socket 重新连接，可能漏掉了部分通知
 */
export type PluginEvent =
  | { kind: 'plugins' }
  | { kind: 'pluginConfig'; id: string }
  | { kind: 'resync' };

const listeners = new Set<() => void>();
const pluginListeners = new Set<(event: PluginEvent) => void>();
let socket: WebSocket | null = null;
let users = 0;
let retry = 0;
let ping = 0;
let bestRoundTrip = Infinity;
let releaseIpc: (() => void) | null = null;
let ipcGeneration = 0;

function publish(next: Partial<SessionState>) {
  state = { ...state, ...next };
  listeners.forEach(listener => listener());
}

export function readSession(): SessionState {
  return state;
}

export function send(message: ClientMessage): void {
  if (state.connection !== 'connected') {
    throw new Error('本机服务尚未连接');
  }

  const generation = ipcGeneration;

  void sendControl(message)
    .then(response => {
      if (response && generation === ipcGeneration && users) {
        apply(response);
      }
    })
    .catch(error => {
      if (generation === ipcGeneration && users) {
        publish({ error: String(error) });
      }
    });
}

export function sendOverlay(
  patch: Partial<ReturnType<typeof parseOverlayConfig>['options']>,
): void {
  send({
    kind: 'overlay',
    settings: { ...parseOverlayConfig(state.snapshot?.overlay).options, ...patch },
  });
}

function emitPlugin(event: PluginEvent) {
  pluginListeners.forEach(listener => listener(event));
}

/**
 * 通过 event socket 推送的 plugin catalog 与 config 变更通知
 */
export function onPluginEvent(listener: (event: PluginEvent) => void): () => void {
  pluginListeners.add(listener);

  return () => {
    pluginListeners.delete(listener);
  };
}

function apply(message: ServerMessage) {
  if (message.kind === 'plugins') {
    emitPlugin({ kind: 'plugins' });

    return;
  }

  if (message.kind === 'pluginConfig') {
    emitPlugin({ kind: 'pluginConfig', id: message.id });

    return;
  }

  if (message.kind === 'error') {
    publish({ error: message.message });

    return;
  }

  if (message.kind === 'clock') {
    const now = performance.now();
    const trip = now - message.clientMs;

    if (trip >= 0 && trip < bestRoundTrip) {
      bestRoundTrip = trip;
      publish({ clockOffset: message.serverMs - (message.clientMs + now) / 2 });
    }

    return;
  }

  applySnapshot(message.state);
}

function applySnapshot(next: Extract<ServerMessage, { kind: 'state' }>['state']) {
  if (next.version !== 1) {
    throw new Error('不支持的数据协议');
  }

  const previous = state.snapshot;

  if (previous?.epoch === next.epoch && next.seq < previous.seq) {
    return;
  }

  const sameSong = previous?.epoch === next.epoch && previous.generation === next.generation;
  let document = next.document;

  if (document === undefined) {
    document = sameSong ? (previous?.document ?? null) : null;
  }

  publish({ snapshot: { ...next, document }, lastStateAt: performance.now(), error: '' });
}

function connectIpc() {
  releaseIpc = subscribeControl(
    event => {
      if (event.kind === 'connection') {
        ipcGeneration++;
        window.clearInterval(ping);
        publish({ connection: event.connected ? 'connected' : 'disconnected', clockOffset: null });

        if (event.connected) {
          bestRoundTrip = Infinity;
          const sendPing = () => send({ kind: 'clock', clientMs: performance.now() });
          sendPing();
          ping = window.setInterval(sendPing, 1000);
        }

        return;
      }

      if (event.kind === 'resync') {
        emitPlugin({ kind: 'resync' });

        return;
      }

      apply(event);
    },
    error => {
      if (!users) {
        return;
      }

      window.clearInterval(ping);
      ipcGeneration++;
      publish({ connection: 'disconnected', error: error.message });
      retry = window.setTimeout(() => {
        releaseIpc?.();
        releaseIpc = null;
        connect();
      }, 1000);
    },
  );
}

function connect() {
  if (!users) {
    return;
  }

  publish({ connection: 'connecting' });

  if (isControlPage()) {
    connectIpc();

    return;
  }

  const current = new WebSocket(
    `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/api/events`,
  );

  socket = current;

  current.onopen = () => {
    publish({ connection: 'connected', clockOffset: null });
    emitPlugin({ kind: 'resync' });
    bestRoundTrip = Infinity;

    const sendPing = () => {
      if (current.readyState === WebSocket.OPEN) {
        current.send(JSON.stringify({ kind: 'clock', clientMs: performance.now() }));
      }
    };

    sendPing();
    ping = window.setInterval(sendPing, 1000);
  };

  current.onmessage = event => {
    try {
      apply(JSON.parse(event.data) as ServerMessage);
    } catch (error) {
      publish({ error: String(error) });
    }
  };

  current.onerror = () => publish({ error: '无法连接本机服务' });

  current.onclose = () => {
    window.clearInterval(ping);
    publish({ connection: 'disconnected' });

    if (users) {
      retry = window.setTimeout(connect, 1000);
    }
  };
}

/**
 * 保持状态通道打开，但不订阅 session 状态更新
 */
export function acquireSession(): () => void {
  return acquire();
}

function acquire() {
  users++;

  if (users === 1) {
    connect();
  }

  return () => {
    users--;

    if (!users) {
      ipcGeneration++;
      window.clearTimeout(retry);
      window.clearInterval(ping);
      releaseIpc?.();
      releaseIpc = null;

      if (socket) {
        socket.onclose = null;
        socket.close();
        socket = null;
      }
    }
  };
}

function subscribe(listener: () => void) {
  listeners.add(listener);

  return () => {
    listeners.delete(listener);
  };
}

export function useSession(): SessionState {
  useEffect(acquire, []);

  return useSyncExternalStore(subscribe, readSession);
}
