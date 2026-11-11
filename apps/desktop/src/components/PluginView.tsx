import type { PluginManifest } from '@hysilens/plugin';
import { attachFrame, PluginHost } from '@hysilens/plugin/host';
import type { HostStatus } from '@hysilens/plugin/host';
import { parseOverlayConfig } from '@hysilens/protocol/config';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';

import { getJson, loadConfig } from '../lib/api';
import type { ConfigValues, PluginCatalog, PluginConfig } from '../lib/api';
import { useOverlayViewport } from '../lib/local-overlay';
import { pluginScale } from '../lib/plugin-scale';
import { pluginFrameState } from '../lib/plugin-state';
import { readSession, useSession } from '../lib/session';

/** Matches the host's clock sync cadence; anchors are only sent when the plugin would drift. */
const SYNC_INTERVAL_MS = 250;

function previewColorScheme(preview: string | null) {
  const schemes: Record<string, string> = {
    system: 'light dark',
    dark: 'dark',
    '1': 'dark',
    light: 'light',
  };

  return schemes[preview ?? 'light'] ?? 'light';
}

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
 * Own one PluginHost per plugin revision and keep it fed with config, lyrics and time.
 * The iframe receives its `src` in the same commit that `ready` turns true, and a layout
 * effect runs before that navigation can dispatch `load` or the plugin's `waiting` message.
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

  // A layout effect declared before the host effect: both run in order within the same commit,
  // so the host is created from the config that just made `ready` true, not a stale ref.
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
    // A new revision means a new iframe element and document, and therefore a new host.
  }, [frame, ready, revision]);

  return { status, ready };
}

export function PluginView({ playground = false }: { playground?: boolean }) {
  const preview = new URLSearchParams(location.search).get('preview');
  const previewMode = usePreviewMode(preview, playground);
  const session = useSession();
  const options = parseOverlayConfig(session.snapshot?.overlay).options;

  const catalog = useQuery({
    queryKey: ['plugins'],
    queryFn: () => getJson<PluginCatalog>('/api/plugins'),
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

      const value = await getJson<PluginConfig>(`/api/plugins/${plugin.id}/config`);

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
