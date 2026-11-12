import { afterEach, describe, expect, expectTypeOf, it, vi } from 'vite-plus/test';

import { createClock, project } from './clock';
import { PluginHost } from './host';
import type { HostStatus } from './host';
import type { HostMessage, InitPayload, LyricDocument } from './protocol';
import { definePlugin, runPlugin } from './runtime';
import type { ConfigCarrier, ConfigOf, Plugin, PluginContext, PortLike } from './runtime';

const lyrics: LyricDocument = {
  id: 'song',
  filename: 'song.lrc',
  metadata: { title: '', artist: '', album: '' },
  tracks: [],
  warnings: [],
};

function initial(overrides: Partial<InitPayload> = {}): InitPayload {
  return {
    config: { color: '#fff' },
    lyrics: null,
    playback: { playing: true, visible: true },
    clock: { position: 1000, rate: 1 },
    preview: false,
    ...overrides,
  };
}

const ports: MessagePort[] = [];

afterEach(() => {
  ports.splice(0).forEach(port => port.close());
  vi.useRealTimers();
});

/**
 * 通过 MessageChannel 连接到真实 runtime 的 host，与 iframe 中的情形一致
 */
function connect<Config>(plugin: Plugin<Config>, state = initial()) {
  const statuses: HostStatus[] = [];
  let now = 0;
  const clock = () => now;
  const host = new PluginHost({ initial: state, now: clock, onStatus: s => statuses.push(s) });
  let runtime: ReturnType<typeof runPlugin> | undefined;

  host.connect({
    postMessage(_message, _origin, [port]) {
      ports.push(port as MessagePort);
      runtime = runPlugin(plugin, {
        port: port as unknown as PortLike,
        root: {} as HTMLElement,
        baseUrl: 'http://127.0.0.1/plugins/demo/',
        now: clock,
      });
    },
  });

  return {
    host,
    statuses,
    runtime: () => runtime,
    advance: (ms: number) => {
      now += ms;
    },
  };
}

/**
 * plugin 侧只记录消息的 host，用于 wire 层面的断言
 */
function recordHost(options: Partial<ConstructorParameters<typeof PluginHost>[0]> = {}) {
  const received: HostMessage[] = [];
  let peer: MessagePort | undefined;
  let now = 0;
  const host = new PluginHost({ initial: initial(), now: () => now, ...options });

  host.connect({
    postMessage(_message, _origin, [port]) {
      peer = port as MessagePort;
      ports.push(peer);
      peer.addEventListener('message', event => received.push(event.data as HostMessage));
      peer.start();
    },
  });

  return {
    host,
    received,
    reply: (message: unknown) => peer?.postMessage(message),
    advance: (ms: number) => {
      now += ms;
    },
  };
}

describe('lifecycle', () => {
  it('awaits mounted hooks before reporting ready', async () => {
    const order: string[] = [];

    const { host } = connect(
      definePlugin({
        setup({ config, onMounted }) {
          order.push(`setup:${String(config().color)}`);
          onMounted(async () => {
            await new Promise(resolve => setTimeout(resolve, 5));
            order.push('mounted');
          });
        },
      }),
    );

    await vi.waitFor(() => expect(host.status.phase).toBe('ready'));

    expect(order).toEqual(['setup:#fff', 'mounted']);
  });

  it('reports a fatal error when setup or mounted fails', async () => {
    const { host } = connect(
      definePlugin({
        setup({ onMounted }) {
          onMounted(() => {
            throw new Error('字体加载失败');
          });
        },
      }),
    );

    await vi.waitFor(() => expect(host.status).toEqual({ phase: 'error', error: '字体加载失败' }));
  });

  it('rejects hooks registered after setup and async setup', async () => {
    let context: PluginContext<Record<string, unknown>> | undefined;

    const { host } = connect(
      definePlugin({
        setup(ctx) {
          context = ctx;
        },
      }),
    );

    await vi.waitFor(() => expect(host.status.phase).toBe('ready'));
    expect(() => context?.onMounted(() => {})).toThrow('setup');

    const failing = connect(definePlugin({ setup: (async () => {}) as () => void }));
    await vi.waitFor(() => expect(failing.host.status.phase).toBe('error'));
  });

  it('awaits unmounted hooks, aborts the signal and resolves dispose', async () => {
    let signal: AbortSignal | undefined;
    const order: string[] = [];

    const { host } = connect(
      definePlugin({
        setup(ctx) {
          signal = ctx.signal;
          ctx.onUnmounted(async () => {
            await new Promise(resolve => setTimeout(resolve, 5));
            order.push(`unmounted:${String(ctx.signal.aborted)}`);
          });
        },
      }),
    );

    await vi.waitFor(() => expect(host.status.phase).toBe('ready'));
    await host.dispose();

    expect(order).toEqual(['unmounted:false']);
    expect(signal?.aborted).toBe(true);
    expect(host.status.phase).toBe('disposed');
  });

  it('stops waiting for an unresponsive plugin', async () => {
    const { host } = recordHost({ disposeTimeoutMs: 10 });

    await host.dispose();

    expect(host.status.phase).toBe('disposed');
  });

  it('fails when the plugin never becomes ready', async () => {
    vi.useFakeTimers();

    const { host } = recordHost({ readyTimeoutMs: 100 });

    vi.advanceTimersByTime(100);

    expect(host.status).toEqual({ phase: 'error', error: '插件未在规定时间内就绪' });
  });
});

describe('state updates', () => {
  it('delivers config, lyrics and playback changes with typed hooks', async () => {
    const events: unknown[] = [];

    const { host } = connect(
      definePlugin({
        setup({ onConfigChange, onLyricsChange, onPlaybackChange, lyrics }) {
          onConfigChange((next, prev) => events.push(['config', prev.color, next.color]));
          onLyricsChange(document => events.push(['lyrics', document?.id, lyrics()?.id]));
          onPlaybackChange(playback => events.push(['playback', playback.visible]));
        },
      }),
    );

    await vi.waitFor(() => expect(host.status.phase).toBe('ready'));

    host.setConfig({ color: '#000' });
    host.setLyrics(lyrics);
    host.setLyrics(lyrics);
    host.setPlayback({ playing: true, visible: true });
    host.setPlayback({ playing: true, visible: false });

    await vi.waitFor(() => expect(events).toHaveLength(3));

    expect(events).toEqual([
      ['config', '#fff', '#000'],
      ['lyrics', 'song', 'song'],
      ['playback', false],
    ]);
  });

  it('reports hook errors without failing the plugin', async () => {
    const { host } = connect(
      definePlugin({
        setup({ onConfigChange }) {
          onConfigChange(() => {
            throw new Error('坏配置');
          });
        },
      }),
    );

    await vi.waitFor(() => expect(host.status.phase).toBe('ready'));

    host.setConfig({ color: '#000' });

    await vi.waitFor(() => expect(host.status).toEqual({ phase: 'ready', error: '坏配置' }));
  });
});

describe('clock', () => {
  it('projects anchors and caps extrapolation', () => {
    expect(project({ position: 1000, rate: 1 }, 0, 500)).toBe(1500);
    expect(project({ position: 1000, rate: 0 }, 0, 500)).toBe(1000);
    expect(project({ position: 1000, rate: 1 }, 0, 10_000)).toBe(3000);
    expect(project({ position: null, rate: 1 }, 0, 500)).toBeNull();
  });

  it.each([40, -40, 350, -350])(
    'slews a %i ms correction without jumps or running backwards',
    correction => {
      let now = 0;

      const clock = createClock({ position: 1000, rate: 1 }, () => now);

      now = 500;
      clock.set({ position: 1500 + correction, rate: 1 });

      expect(clock.time()).toBe(1500);

      let previous = 1500;

      for (now = 516; now < 1100; now += 16) {
        const current = clock.time() ?? 0;
        expect(current).toBeGreaterThanOrEqual(previous);
        expect(Math.abs(current - (now + 1000))).toBeLessThanOrEqual(Math.abs(correction));
        previous = current;
      }

      /* slew 窗口结束后，clock 与权威时间线完全一致 */
      now = 1100;
      expect(clock.time()).toBeCloseTo(2100 + correction);
    },
  );

  it('applies seeks, pauses and newly known positions immediately', () => {
    let now = 0;

    const clock = createClock({ position: null, rate: 0 }, () => now);

    clock.set({ position: 1000, rate: 1 });
    expect(clock.time()).toBe(1000);

    now = 100;
    clock.set({ position: 30_000, rate: 1 });
    expect(clock.time()).toBe(30_000);

    now = 200;
    clock.set({ position: 30_120, rate: 0 });
    expect(clock.time()).toBe(30_120);

    now = 900;
    expect(clock.time()).toBe(30_120);
  });

  it('sends anchors only when the plugin projection drifts', async () => {
    const { host, received, advance } = recordHost();
    const clocks = () => received.filter(message => message.kind === 'clock');

    advance(250);
    host.syncClock({ position: 1250, rate: 1 });
    host.syncClock({ position: 1270, rate: 1 });
    host.syncClock({ position: 1300, rate: 1 });
    host.syncClock({ position: 1300, rate: 0 });
    host.syncClock({ position: null, rate: 0 });

    await vi.waitFor(() => expect(received.at(-1)?.kind).toBe('clock'));

    expect(clocks()).toEqual([
      { kind: 'clock', position: 1300, rate: 1 },
      { kind: 'clock', position: 1300, rate: 0 },
      { kind: 'clock', position: null, rate: 0 },
    ]);
  });

  it('keeps time flowing through steady playback longer than the extrapolation guard', async () => {
    let time: (() => number | null) | undefined;

    const { host, advance } = connect(
      definePlugin({
        setup(ctx) {
          time = ctx.time;
        },
      }),
    );

    await vi.waitFor(() => expect(host.status.phase).toBe('ready'));

    /*
     * 准确的 host 不会漂移；但缺少 heartbeat 时 plugin 会冻结在 +2000 ms，
     * 直到下一次 sync 才发现
     * 因此要在两次 sync 之间采样，此时冻结在屏幕上可见
     */
    for (let elapsed = 250; elapsed <= 5000; elapsed += 250) {
      advance(250);
      expect(time?.()).toBe(1000 + elapsed);
      host.syncClock({ position: 1000 + elapsed, rate: 1 });
      await new Promise(resolve => setTimeout(resolve, 5));
    }
  });

  it('sends no heartbeat while paused', async () => {
    const { host, received, advance } = recordHost();

    host.syncClock({ position: 1000, rate: 0 });

    for (let step = 0; step < 12; step += 1) {
      advance(250);
      host.syncClock({ position: 1000, rate: 0 });
    }

    await new Promise(resolve => setTimeout(resolve, 10));

    expect(received.filter(message => message.kind === 'clock')).toHaveLength(1);
  });

  it('exposes projected time to the plugin', async () => {
    let time: (() => number | null) | undefined;

    const { host, advance } = connect(
      definePlugin({
        setup(ctx) {
          time = ctx.time;
        },
      }),
    );

    await vi.waitFor(() => expect(host.status.phase).toBe('ready'));

    advance(400);
    expect(time?.()).toBe(1400);

    host.syncClock({ position: 5000, rate: 0 });
    await vi.waitFor(() => expect(time?.()).toBe(5000));
  });
});

describe('types', () => {
  it('infers configuration from a config carrier', () => {
    type Spec = ConfigCarrier<{ size: number; fontFamily: string }>;
    expectTypeOf<ConfigOf<Spec>>().toEqualTypeOf<{ size: number; fontFamily: string }>();
    expectTypeOf<ConfigOf<unknown>>().toEqualTypeOf<Record<string, unknown>>();

    definePlugin<Spec>({
      setup({ config, onConfigChange }) {
        expectTypeOf(config().size).toBeNumber();
        onConfigChange(next => expectTypeOf(next.fontFamily).toBeString());
      },
    });
  });
});
