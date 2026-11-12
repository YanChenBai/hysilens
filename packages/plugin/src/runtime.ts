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

/**
 * 由 `defineConfig` 填充的 phantom 槽位；用 `definePlugin<typeof config>` 读取
 */
export interface ConfigCarrier<Config> {
  readonly '~config'?: Config;
}

/**
 * spec 携带的 config 类型；不是 carrier 的类型一律退化为无类型 record
 */
export type ConfigOf<T> =
  T extends ConfigCarrier<infer Config>
    ? unknown extends Config
      ? Record<string, unknown>
      : NonNullable<Config>
    : Record<string, unknown>;

/**
 * plugin 从 runtime 收到的一切；所有成员都是实时视图或注册点
 */
export interface PluginContext<Config> {
  /**
   * 由 runtime 创建并挂到 `document.body` 的全尺寸容器
   */
  readonly root: HTMLElement;

  /**
   * runtime 运行在 dev 预览而非真实 overlay 中时为 true
   */
  readonly preview: boolean;

  /**
   * unmounted hook 执行完后 abort；传给监听器即可自动清理
   */
  readonly signal: AbortSignal;
  /* 成员是函数属性而不是方法：plugin 可以随意解构 context */

  /**
   * 已由 host 套用默认值的校验后配置
   */
  readonly config: () => Readonly<Config>;

  /**
   * 当前会话的歌词；尚未加载时为 `null`
   */
  readonly lyrics: () => LyricDocument | null;

  /**
   * host 最近发送的播放状态
   */
  readonly playback: () => Playback;

  /**
   * 已应用 offset 的歌词时间，单位 ms，由最新的 clock anchor 投影而来
   */
  readonly time: () => number | null;

  /**
   * plugin 自带文件的绝对 URL，基于 plugin 的 base 解析
   */
  readonly assetUrl: (path: string) => string;

  /**
   * 在 host 中上报非致命错误，不停止 plugin
   */
  readonly report: (error: unknown) => void;

  /**
   * 在 `setup` 返回后运行一次；所有 hook 都 resolve 之后才会通知 host `ready`
   * 必须在 `setup` 中同步调用
   */
  readonly onMounted: (hook: () => Awaitable<void>) => void;

  /**
   * 在 dispose 时、`signal` abort 之前运行；即使某个 hook reject，其余 hook 仍会被等待
   */
  readonly onUnmounted: (hook: () => Awaitable<void>) => void;

  /**
   * 在 config 变化后运行，参数为新的冻结对象与前一个对象
   */
  readonly onConfigChange: (hook: (next: Readonly<Config>, prev: Readonly<Config>) => void) => void;

  /**
   * 在歌词文档被替换后运行
   */
  readonly onLyricsChange: (hook: (document: LyricDocument | null) => void) => void;

  readonly onPlaybackChange: (hook: (playback: Playback) => void) => void;
}

/**
 * `definePlugin` 接受的形状：单个同步的 `setup` 入口
 */
export interface PluginDefinition<Config> {
  /**
   * 同步初始化；异步工作应放进 `onMounted` hook
   */
  readonly setup: (context: PluginContext<Config>) => void;
}

/**
 * `definePlugin` 产出的带标记定义；`kind` 让 bootstrap 拒绝其他被导出为 default 的东西
 */
export interface Plugin<Config = Record<string, unknown>> extends PluginDefinition<Config> {
  readonly kind: 'hysilens:plugin';
}

/**
 * 纯描述符；host 连接后由构建注入的 bootstrap 挂载它
 */
export function definePlugin<Spec = unknown>(
  definition: PluginDefinition<ConfigOf<Spec>>,
): Plugin<ConfigOf<Spec>> {
  if (typeof definition?.setup !== 'function') {
    throw new TypeError('definePlugin 需要 setup 函数');
  }

  return { kind: 'hysilens:plugin', setup: definition.setup };
}

/**
 * `value` 是否为 plugin 定义，而不是任意默认导出
 */
export function isPlugin(value: unknown): value is Plugin {
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as Plugin).kind === 'hysilens:plugin' &&
    typeof (value as Plugin).setup === 'function'
  );
}

/**
 * runtime 依赖的 `MessagePort` 子集；runtime 在 `disposed` 之后关闭它
 */
export interface PortLike {
  postMessage(message: PluginMessage): void;
  addEventListener(type: 'message', listener: (event: { data: unknown }) => void): void;

  /**
   * 必须调用才能收到已排队的消息，与 `MessagePort` 一致
   */
  start(): void;
  close(): void;
}

/**
 * `runPlugin` 使用的、由 host 提供的运行环境
 */
export interface RuntimeEnvironment {
  port: PortLike;

  /**
   * 作为 `context.root` 交给 plugin 的容器，由调用方拥有
   */
  root: HTMLElement;

  /**
   * `context.assetUrl` 使用的绝对 base，通常是 `document.baseURI`
   */
  baseUrl: string;

  /**
   * 用来投影歌词时间的单调时钟，单位 ms
   */
  now: () => number;
}

/**
 * `setup` 期间填充的 hook 列表；之后拒绝再注册，
 * 因此处理第一条消息时每个列表都已固定
 */
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

/**
 * `init` 到达前是 `waiting`，mounted hook 执行期间是 `mounting`，之后是 `ready`
 * `failed` 表示 setup 或某个 mounted hook 抛错；`disposed` 是终态
 */
export type RuntimePhase = 'waiting' | 'mounting' | 'ready' | 'failed' | 'disposing' | 'disposed';

/**
 * 在已连接的 port 上驱动一个 plugin
 * 返回当前阶段用于诊断，以及转发非致命错误的 `report`
 * （用于未捕获的 window 错误）
 */
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
