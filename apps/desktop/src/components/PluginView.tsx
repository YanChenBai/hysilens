import type { PluginManifest } from '@hysilens/plugin';
import { attachFrame, PluginHost } from '@hysilens/plugin/host';
import type { HostStatus } from '@hysilens/plugin/host';
import { parseOverlayConfig } from '@hysilens/protocol/config';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';

import { loadConfig } from '../lib/api';
import type { ConfigValues } from '../lib/api';
import { useOverlayViewport } from '../lib/local-overlay';
import { pluginApi } from '../lib/plugin-api';
import { pluginScale } from '../lib/plugin-scale';
import { pluginFrameState } from '../lib/plugin-state';
import { readSession, useSession } from '../lib/session';

/**
 * 与 host 的 clock 同步节奏一致；仅当 plugin 会产生漂移时才发送 anchor
 */
const SYNC_INTERVAL_MS = 250;

/**
 * 与 preview query 值对应的 CSS `color-scheme`，让 iframe 绘制的背景与控制 UI 一致
 */
function previewColorScheme(preview: string | null) {
  const schemes: Record<string, string> = {
    system: 'light dark',
    dark: 'dark',
    '1': 'dark',
    light: 'light',
  };

  return schemes[preview ?? 'light'] ?? 'light';
}

/**
 * 跟踪 preview iframe 是否应使用演示数据，由父级控制面板通过 `postMessage` 驱动
 * 非 preview 模式下以初始 `playground` 值为准
 */
function usePreviewMode(preview: string | null, playground: boolean) {
  const [previewMode, setPreviewMode] = useState(playground);

  useEffect(() => {
    if (!preview) {
      return;
    }

    function receive(event: MessageEvent) {
      if (
        event.source === parent &&
        event.origin === location.origin &&
        event.data?.kind === 'hysilens:preview-mode' &&
        typeof event.data.playground === 'boolean'
      ) {
        setPreviewMode(event.data.playground);
      }
    }

    window.addEventListener('message', receive);
    parent.postMessage({ kind: 'hysilens:preview-ready' }, location.origin);

    return () => window.removeEventListener('message', receive);
  }, [preview]);

  return previewMode;
}

/**
 * 每个 plugin revision 拥有一个 PluginHost，并持续向它提供 config、歌词与时间
 * iframe 在 `ready` 变为 true 的同一次 commit 中拿到 `src`，而 layout effect
 * 会在该跳转派发 `load` 或 plugin 的 `waiting` 消息之前执行
 *
 * @param frame host 要挂载到的 iframe 元素
 * @param plugin 当前活跃的 manifest；revision 变化会构建新的 host 与 document
 * @param config 归一化后的 config 值，未加载完成时为 `undefined`
 * @param previewMode 是否用演示歌词代替实时 session
 */
function usePluginHost(
  frame: React.RefObject<HTMLIFrameElement | null>,
  plugin: PluginManifest | undefined,
  config: ConfigValues | undefined,
  previewMode: boolean,
) {
  const [status, setStatus] = useState<HostStatus>({ phase: 'idle' });
  const host = useRef<PluginHost | null>(null);
  const latest = useRef({ config, previewMode });
  const ready = Boolean(plugin && config);
  const revision = plugin ? `${plugin.id}:${plugin.revision}` : '';

  /*
   * 声明在 host effect 之前的 layout effect：两者在同一次 commit 中按序执行
   * 因此 host 用的是刚让 `ready` 变为 true 的 config，而不是过期的 ref
   */
  useLayoutEffect(() => {
    latest.current = { config, previewMode };

    if (config) {
      host.current?.setConfig(config);
    }
  }, [config, previewMode]);

  useLayoutEffect(() => {
    const element = frame.current;
    const initialConfig = latest.current.config;

    if (!element || !ready || !initialConfig) {
      return;
    }

    const startedAt = performance.now();

    const state = () =>
      pluginFrameState(readSession(), {
        playground: latest.current.previewMode,
        startedAt,
        now: performance.now(),
      });

    const current = new PluginHost({
      initial: { config: initialConfig, preview: latest.current.previewMode, ...state() },
      onStatus: setStatus,
    });

    const detach = attachFrame(current, element);
    host.current = current;

    const timer = setInterval(() => {
      const next = state();
      current.setLyrics(next.lyrics);
      current.setPlayback(next.playback);
      current.syncClock(next.clock);
    }, SYNC_INTERVAL_MS);

    return () => {
      clearInterval(timer);
      detach();
      host.current = null;
      void current.dispose();
    };
    /* 新的 revision 意味着新的 iframe 元素与 document，因此也需要新的 host */
  }, [frame, ready, revision]);

  return { status, ready };
}

/**
 * 在沙箱 iframe 中渲染当前歌词 plugin 并保持同步
 *
 * 在 preview 模式下，`preview` query 参数决定配色方案，并在嵌入控制面板时
 * 把 frame 切换为演示数据
 *
 * @param playground frame 默认是否从演示数据开始；供 `/playground` 使用
 */
export function PluginView({ playground = false }: { playground?: boolean }) {
  const preview = new URLSearchParams(location.search).get('preview');
  const previewMode = usePreviewMode(preview, playground);
  const session = useSession();
  const options = parseOverlayConfig(session.snapshot?.overlay).options;

  const catalog = useQuery({
    queryKey: ['plugins'],
    queryFn: pluginApi.listPlugins,
  });

  const plugin = catalog.data?.plugins.find(item => item.id === options.renderer);

  useOverlayViewport(plugin, Boolean(preview));

  const config = useQuery({
    queryKey: ['plugin-config', plugin?.id, plugin?.revision],
    enabled: Boolean(plugin),
    queryFn: async () => {
      if (!plugin) {
        throw new Error('未找到歌词插件');
      }

      const value = await pluginApi.getPluginConfig(plugin.id);

      return loadConfig(plugin, value.config);
    },
  });

  const frame = useRef<HTMLIFrameElement>(null);
  const query = preview ? `&preview=${encodeURIComponent(preview)}` : '';

  const src = plugin
    ? `/api/plugins/${plugin.id}/view?revision=${encodeURIComponent(plugin.revision)}${query}`
    : '';

  const { status, ready } = usePluginHost(frame, plugin, config.data, previewMode);

  if (!plugin) {
    return <div data-plugin-error="未找到歌词插件" />;
  }

  const error = config.error ? String(config.error) : status.error;

  return (
    <iframe
      data-plugin-error={error}
      data-plugin-status={status.phase}
      key={`${plugin.id}:${plugin.revision}`}
      ref={frame}
      title={plugin.name}
      src={ready ? src : undefined}
      sandbox="allow-scripts"
      className="absolute inset-0 block h-full w-full border-0 bg-transparent"
      style={{
        colorScheme: previewColorScheme(preview),
        transform: `scale(${pluginScale(config.data?.scale, plugin.features.scale === true)})`,
        transformOrigin: 'center',
      }}
    />
  );
}
