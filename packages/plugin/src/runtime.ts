import { createClock } from './clock';
import { errorMessage } from './protocol';
import type {
  ClockAnchor,
  HostMessage,
  InitPayload,
  LyricDocument,
  Playback,
  PluginMessage,
} from './protocol';

type Awaitable<T> = T | Promise<T>;

/** Phantom slot filled by `defineConfig`; read with `definePlugin<typeof config>`. */
export interface ConfigCarrier<Config> {
  readonly '~config'?: Config;
}

export type ConfigOf<T> =
  T extends ConfigCarrier<infer Config>
    ? unknown extends Config
      ? Record<string, unknown>
      : NonNullable<Config>
    : Record<string, unknown>;

export interface PluginContext<Config> {
  /** Full-size container created by the runtime and attached to `document.body`. */
  readonly root: HTMLElement;
  readonly preview: boolean;
  readonly signal: AbortSignal;
  // Members are function properties, not methods: plugins destructure the context freely.
  /** Validated configuration with defaults applied by the host. */
  readonly config: () => Readonly<Config>;
  readonly lyrics: () => LyricDocument | null;
  readonly playback: () => Playback;
  /** Lyric time in ms with offset applied, projected from the latest clock anchor. */
  readonly time: () => number | null;
  readonly assetUrl: (path: string) => string;
  /** Surface a non-fatal error in the host without stopping the plugin. */
  readonly report: (error: unknown) => void;

  readonly onMounted: (hook: () => Awaitable<void>) => void;
  readonly onUnmounted: (hook: () => Awaitable<void>) => void;
  readonly onConfigChange: (hook: (next: Readonly<Config>, prev: Readonly<Config>) => void) => void;
  readonly onLyricsChange: (hook: (document: LyricDocument | null) => void) => void;
  readonly onPlaybackChange: (hook: (playback: Playback) => void) => void;
}

export interface PluginDefinition<Config> {
  readonly setup: (context: PluginContext<Config>) => void;
}

export interface Plugin<Config = Record<string, unknown>> extends PluginDefinition<Config> {
  readonly kind: 'hysilens:plugin';
}

/** Pure descriptor; the build-injected bootstrap mounts it once the host connects. */
export function definePlugin<Spec = unknown>(
  definition: PluginDefinition<ConfigOf<Spec>>,
): Plugin<ConfigOf<Spec>> {
  if (typeof definition?.setup !== 'function') {
    throw new TypeError('definePlugin 需要 setup 函数');
  }

  return { kind: 'hysilens:plugin', setup: definition.setup };
}

export function isPlugin(value: unknown): value is Plugin {
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as Plugin).kind === 'hysilens:plugin' &&
    typeof (value as Plugin).setup === 'function'
  );
}

export interface PortLike {
  postMessage(message: PluginMessage): void;
  addEventListener(type: 'message', listener: (event: { data: unknown }) => void): void;
  start(): void;
  close(): void;
}

export interface RuntimeEnvironment {
  port: PortLike;
  root: HTMLElement;
  baseUrl: string;
  now: () => number;
}

interface Hooks<Config> {
  mounted: Array<() => Awaitable<void>>;
  unmounted: Array<() => Awaitable<void>>;
  config: Array<(next: Readonly<Config>, prev: Readonly<Config>) => void>;
  lyrics: Array<(document: LyricDocument | null) => void>;
  playback: Array<(playback: Playback) => void>;
}

interface Instance<Config> {
  state: InitPayload;
  hooks: Hooks<Config>;
  setClock(anchor: ClockAnchor): void;
  abort: AbortController;
}

export type RuntimePhase = 'waiting' | 'mounting' | 'ready' | 'failed' | 'disposing' | 'disposed';

/** Drive one plugin over a connected port. Returns the current phase for diagnostics. */
export function runPlugin<Config>(plugin: Plugin<Config>, env: RuntimeEnvironment) {
  let phase: RuntimePhase = 'waiting';
  let instance: Instance<Config> | undefined;

  const send = (message: PluginMessage) => {
    if (phase !== 'disposed') {
      env.port.postMessage(message);
    }
  };

  const report = (error: unknown, fatal = false) => {
    send({ kind: 'error', message: errorMessage(error), fatal });
  };

  function emit<Args extends unknown[]>(hooks: Array<(...args: Args) => void>, ...args: Args) {
    for (const hook of hooks) {
      try {
        hook(...args);
      } catch (error) {
        report(error);
      }
    }
  }

  async function mount(payload: InitPayload) {
    phase = 'mounting';
    instance = createInstance(plugin, payload, env, report);

    for (const hook of instance.hooks.mounted) {
      await hook();
    }

    if (phase === 'mounting') {
      phase = 'ready';
      send({ kind: 'ready' });
    }
  }

  async function dispose() {
    if (phase === 'disposing' || phase === 'disposed') {
      return;
    }

    phase = 'disposing';

    const results = await Promise.allSettled(
      (instance?.hooks.unmounted ?? []).map(async hook => hook()),
    );

    for (const result of results) {
      if (result.status === 'rejected') {
        report(result.reason);
      }
    }

    instance?.abort.abort();
    send({ kind: 'disposed' });
    phase = 'disposed';
    env.port.close();
  }

  function update(message: Exclude<HostMessage, { kind: 'init' | 'dispose' }>) {
    if (!instance || phase === 'disposing' || phase === 'disposed') {
      return;
    }

    const { state, hooks } = instance;

    switch (message.kind) {
      case 'config': {
        const prev = state.config as Readonly<Config>;
        state.config = Object.freeze({ ...message.config });
        emit(hooks.config, state.config as Readonly<Config>, prev);
        break;
      }

      case 'lyrics': {
        state.lyrics = message.document;
        emit(hooks.lyrics, message.document);
        break;
      }

      case 'playback': {
        state.playback = Object.freeze({ playing: message.playing, visible: message.visible });
        emit(hooks.playback, state.playback);
        break;
      }

      case 'clock': {
        instance.setClock({ position: message.position, rate: message.rate });
        break;
      }
    }
  }

  env.port.addEventListener('message', event => {
    const message = event.data as HostMessage;

    if (message?.kind === 'init') {
      if (phase === 'waiting') {
        mount(message).catch(error => {
          phase = 'failed';
          report(error, true);
        });
      }
    } else if (message?.kind === 'dispose') {
      void dispose();
    } else if (message?.kind) {
      update(message);
    }
  });
  env.port.start();

  return { phase: () => phase, report: (error: unknown) => report(error) };
}

function createInstance<Config>(
  plugin: Plugin<Config>,
  payload: InitPayload,
  env: RuntimeEnvironment,
  report: (error: unknown) => void,
): Instance<Config> {
  const state: InitPayload = {
    config: Object.freeze({ ...payload.config }),
    lyrics: payload.lyrics,
    playback: Object.freeze({ ...payload.playback }),
    clock: payload.clock,
    preview: payload.preview,
  };

  const clock = createClock(payload.clock, env.now);
  const abort = new AbortController();
  const hooks: Hooks<Config> = { mounted: [], unmounted: [], config: [], lyrics: [], playback: [] };
  let registering = true;

  function register<Key extends keyof Hooks<Config>>(key: Key) {
    return (hook: Hooks<Config>[Key][number]) => {
      if (!registering) {
        throw new Error('生命周期钩子只能在 setup 中同步注册');
      }

      (hooks[key] as Array<typeof hook>).push(hook);
    };
  }

  const context: PluginContext<Config> = {
    root: env.root,
    preview: state.preview,
    signal: abort.signal,
    config: () => state.config as Readonly<Config>,
    lyrics: () => state.lyrics,
    playback: () => state.playback,
    time: clock.time,
    assetUrl: path => new URL(path, env.baseUrl).href,
    report,
    onMounted: register('mounted'),
    onUnmounted: register('unmounted'),
    onConfigChange: register('config'),
    onLyricsChange: register('lyrics'),
    onPlaybackChange: register('playback'),
  };

  try {
    const result: unknown = plugin.setup(context);

    if (result instanceof Promise) {
      throw new TypeError('setup 必须是同步函数，异步初始化请放在 onMounted 中');
    }
  } finally {
    registering = false;
  }

  return { state, hooks, setClock: clock.set, abort };
}
