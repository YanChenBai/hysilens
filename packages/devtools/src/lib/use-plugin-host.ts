import type { LyricDocument } from '@hysilens/plugin';
import { PluginHost } from '@hysilens/plugin/host';
import type { HostStatus } from '@hysilens/plugin/host';
import { WAITING_KIND } from '@hysilens/plugin/protocol';
import type { ConnectMessage } from '@hysilens/plugin/protocol';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { RefObject } from 'react';

import type { ConfigValues } from './config';
import { clockAnchor, isRunning } from './transport';
import type { Transport } from './transport';

export interface LogEntry {
  id: number;
  at: Date;
  level: 'info' | 'warn' | 'error';
  message: string;
}

interface Inputs {
  config: ConfigValues;
  lyrics: LyricDocument | null;
  transport: Transport;
  visible: boolean;
}

/** Drift is checked often so seeks and speed changes reach the plugin within a frame or two. */
const SYNC_INTERVAL_MS = 100;

function playbackOf({ transport, visible }: Inputs) {
  return { playing: isRunning(transport, performance.now()), visible };
}

/**
 * One PluginHost per plugin document. Every page load (including Vite full reloads after a code
 * change) announces itself with `waiting`, so the host is recreated for each new document
 * instead of staying bound to a page that no longer exists.
 */
export function usePluginHost(frame: RefObject<HTMLIFrameElement | null>, inputs: Inputs) {
  const [status, setStatus] = useState<HostStatus>({ phase: 'idle' });
  const [log, setLog] = useState<LogEntry[]>([]);
  const latest = useRef(inputs);
  const host = useRef<PluginHost | null>(null);

  useLayoutEffect(() => {
    latest.current = inputs;
  });

  useEffect(() => {
    let sequence = 0;
    let previous: HostStatus = { phase: 'idle' };

    const append = (level: LogEntry['level'], message: string) =>
      setLog(entries => [
        ...entries.slice(-199),
        { id: (sequence += 1), at: new Date(), level, message },
      ]);

    function onStatus(next: HostStatus) {
      setStatus(next);

      if (next.phase !== previous.phase) {
        append(next.phase === 'error' ? 'error' : 'info', next.error ?? `宿主：${next.phase}`);
      } else if (next.error && next.error !== previous.error) {
        append('warn', next.error);
      }

      previous = next;
    }

    function receive(event: MessageEvent) {
      const source = event.source as Window | null;

      if (!source || source !== frame.current?.contentWindow || event.data?.kind !== WAITING_KIND) {
        return;
      }

      void host.current?.dispose();
      previous = { phase: 'idle' };

      const current = latest.current;

      const next = new PluginHost({
        initial: {
          config: current.config,
          lyrics: current.lyrics,
          playback: playbackOf(current),
          clock: clockAnchor(current.transport, performance.now()),
          preview: true,
        },
        onStatus,
      });

      host.current = next;
      next.connect({
        postMessage: (message: ConnectMessage, origin: string, transfer: Transferable[]) =>
          source.postMessage(message, origin, transfer),
      });
    }

    window.addEventListener('message', receive);

    const timer = setInterval(() => {
      host.current?.syncClock(clockAnchor(latest.current.transport, performance.now()));
      host.current?.setPlayback(playbackOf(latest.current));
    }, SYNC_INTERVAL_MS);

    return () => {
      window.removeEventListener('message', receive);
      clearInterval(timer);
      void host.current?.dispose();
      host.current = null;
    };
  }, [frame]);

  useEffect(() => host.current?.setConfig(inputs.config), [inputs.config]);
  useEffect(() => host.current?.setLyrics(inputs.lyrics), [inputs.lyrics]);

  const { transport, visible } = inputs;

  // Seeks, pauses and speed changes take effect at once rather than on the next tick.
  useEffect(() => {
    const now = performance.now();
    host.current?.syncClock(clockAnchor(transport, now));
    host.current?.setPlayback({ playing: isRunning(transport, now), visible });
  }, [transport, visible]);

  return { status, log, clearLog: () => setLog([]) };
}
