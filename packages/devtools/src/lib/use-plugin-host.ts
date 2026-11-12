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
  /**
   * 作为 React key 的单调递增 id
   */
  id: number;

  /**
   * 记录该条目时的墙上时间
   */
  at: Date;

  level: 'info' | 'warn' | 'error';

  message: string;
}

/**
 * devtools 推送给 plugin host 的全部内容；每次 render 都是新对象
 */
interface Inputs {
  config: ConfigValues;

  lyrics: LyricDocument | null;

  transport: Transport;

  /**
   * host 是否应把 overlay 视为对播放器可见
   */
  visible: boolean;
}

/**
 * 频繁检查 drift，让 seek 和速度变化能在一两帧内到达 plugin
 */
const SYNC_INTERVAL_MS = 100;

function playbackOf({ transport, visible }: Inputs) {
  return { playing: isRunning(transport, performance.now()), visible };
}

/**
 * 每个 plugin 文档对应一个 PluginHost，每次页面加载（包括改代码后 Vite 的整页重载）
 * 都会用 `waiting` 宣告自己，因此 host 会为每个新文档重新创建，
 * 而不是继续绑定到已经不存在的页面
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

  /* seek、暂停和速度变化立即生效，而不是等到下一个 tick */
  useEffect(() => {
    const now = performance.now();
    host.current?.syncClock(clockAnchor(transport, now));
    host.current?.setPlayback({ playing: isRunning(transport, now), visible });
  }, [transport, visible]);

  return { status, log, clearLog: () => setLog([]) };
}
