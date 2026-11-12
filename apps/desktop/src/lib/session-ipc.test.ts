import type { Snapshot } from '@hysilens/protocol';
import { parseOverlayConfig } from '@hysilens/protocol/config';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vite-plus/test';

import type { ControlEvent } from './control';

const { subscribe, send, release, callbacks } = vi.hoisted(() => ({
  subscribe: vi.fn(),
  send: vi.fn(),
  release: vi.fn(),
  callbacks: [] as ((event: ControlEvent) => void)[],
}));

vi.mock('./control', () => ({
  isControlPage: () => true,
  sendControl: send,
  subscribeControl: subscribe,
}));

beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
  vi.useFakeTimers();
  callbacks.length = 0;
  vi.stubGlobal('WebSocket', vi.fn());
  vi.stubGlobal('window', {
    setInterval,
    clearInterval,
    setTimeout,
    clearTimeout,
  });
  send.mockResolvedValue(null);
  subscribe.mockImplementation((receive: (event: ControlEvent) => void) => {
    callbacks.push(receive);

    return release;
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('main-window session IPC', () => {
  it('aligns playback time from pushed timestamps without clock requests', async () => {
    const session = await import('./session');
    const stop = session.acquireSession();

    const snapshot: Snapshot = {
      overlay: parseOverlayConfig(undefined).options,
      source: 'auto',
      version: 1,
      epoch: 'push-clock',
      seq: 1,
      generation: 0,
      playback: { serverMs: 500, sessions: [], error: null },
      session: null,
      lyricsStatus: 'waiting',
      candidates: [],
      document: null,
      offsetMs: 0,
      showRomanization: true,
      showTranslation: true,
      cacheIssues: [],
    };

    callbacks[0]({ kind: 'connection', connected: true });
    callbacks[0]({ kind: 'state', state: snapshot, sentAtMs: performance.now() + 500 });
    expect(session.readSession().clockOffset).toBe(500);
    callbacks[0]({
      kind: 'state',
      state: { ...snapshot, seq: 2 },
      sentAtMs: performance.now() + 495,
    });
    expect(session.readSession().clockOffset).toBe(500);
    await vi.advanceTimersByTimeAsync(5000);

    expect(send).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
    stop();
  });

  it('shares one native subscription and releases it after the last user', async () => {
    const session = await import('./session');
    const first = session.acquireSession();
    const second = session.acquireSession();
    callbacks[0]({ kind: 'connection', connected: true });
    session.send({ kind: 'offset', value: 250 });

    expect(subscribe).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith({ kind: 'offset', value: 250 });
    expect(WebSocket).not.toHaveBeenCalled();
    first();
    expect(release).not.toHaveBeenCalled();
    second();
    expect(release).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('handles service restart notifications without opening a socket or duplicating timers', async () => {
    const session = await import('./session');
    const receivePlugin = vi.fn();
    const unsubscribe = session.onPluginEvent(receivePlugin);
    const stop = session.acquireSession();
    callbacks[0]({ kind: 'connection', connected: true });
    callbacks[0]({ kind: 'connection', connected: false });

    expect(session.readSession().connection).toBe('disconnected');
    expect(() => session.send({ kind: 'offset', value: 20 })).toThrow('尚未连接');
    expect(vi.getTimerCount()).toBe(0);

    callbacks[0]({ kind: 'connection', connected: true });
    callbacks[0]({ kind: 'resync' });

    expect(session.readSession().connection).toBe('connected');
    expect(receivePlugin).toHaveBeenCalledWith({ kind: 'resync' });
    expect(subscribe).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(5000);
    expect(send).not.toHaveBeenCalled();
    expect(WebSocket).not.toHaveBeenCalled();
    stop();
    unsubscribe();
  });
});
