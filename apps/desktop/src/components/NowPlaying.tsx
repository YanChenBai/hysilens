import type { Snapshot } from '@hysilens/protocol';
import { Music } from 'kubai-icons-react';
import { useEffect, useState } from 'react';

import { formatClock, playbackPosition } from '../lib/clock';

/**
 * 当前进度与总时长，播放中每 250 ms 外推一次；未知值显示 `-`
 *
 * @param clockOffset 服务端 clock 减本地 clock；未测出前为 null，此时只显示上报位置
 */
function PlaybackTime({
  snapshot,
  clockOffset,
}: {
  snapshot: Snapshot;

  clockOffset: number | null;
}) {
  const [now, setNow] = useState(() => performance.now());
  const playing = snapshot.session?.status === 'playing';

  useEffect(() => {
    if (!playing) {
      return;
    }

    const timer = window.setInterval(() => setNow(performance.now()), 250);

    return () => window.clearInterval(timer);
  }, [playing]);

  const position = playbackPosition(snapshot, clockOffset === null ? null : now + clockOffset);
  const text = `${formatClock(position)} / ${formatClock(snapshot.session?.durationMs)}`;

  return (
    <span className="shrink-0 tabular-nums" aria-label={`播放进度 ${text}`}>
      {text}
    </span>
  );
}

/**
 * 紧凑的正在播放信息，带连接指示器与播放进度
 *
 * @param connected event socket 是否打开；决定圆点颜色与 tooltip
 * @param clockOffset 服务端 clock 减本地 clock，用于外推播放进度
 */
export function NowPlaying({
  snapshot,
  connected,
  clockOffset,
}: {
  snapshot: Snapshot | null;

  connected: boolean;

  clockOffset: number | null;
}) {
  const session = snapshot?.session;
  const connectionLabel = connected ? '已连接播放器服务' : '正在连接服务';

  return (
    <section className="flex min-w-20 flex-1 items-center gap-2.5" aria-label="当前播放">
      <div className="relative shrink-0" title={connectionLabel}>
        <Music size={24} color="#a78bfa" />
        <span
          role="status"
          aria-label={connectionLabel}
          className={`ring-background absolute -right-0.5 -bottom-0.5 size-1.5 rounded-full ring-2 ${connected ? 'bg-success' : 'bg-muted-foreground'}`}
        />
      </div>
      <div className="min-w-0 flex-1">
        <p className="truncate text-xs font-medium" title={session?.title}>
          {session?.title || '等待播放器'}
        </p>
        <div className="text-muted-foreground mt-0.5 flex min-w-0 items-baseline gap-1.5 text-[11px]">
          <p className="min-w-0 truncate">{session?.artist || '打开播放器同步歌词'}</p>
          {session && (
            <>
              <span aria-hidden className="shrink-0 opacity-60">
                ·
              </span>
              <PlaybackTime snapshot={snapshot} clockOffset={clockOffset} />
            </>
          )}
        </div>
      </div>
    </section>
  );
}
