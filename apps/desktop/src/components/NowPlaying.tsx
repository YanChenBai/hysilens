import type { Snapshot } from '@hysilens/protocol';
import { Music } from 'kubai-icons-react';

export function NowPlaying({
  snapshot,
  connected,
}: {
  snapshot: Snapshot | null;
  connected: boolean;
}) {
  return (
    <section className="flex min-w-20 flex-1 items-center gap-2.5" aria-label="当前播放">
      <div className="relative shrink-0" title={connected ? '已连接播放器服务' : '正在连接服务'}>
        <Music size={24} color="#a78bfa" />
        <span
          role="status"
          aria-label={connected ? '已连接播放器服务' : '正在连接服务'}
          className={`ring-background absolute -right-0.5 -bottom-0.5 size-1.5 rounded-full ring-2 ${connected ? 'bg-success' : 'bg-muted-foreground'}`}
        />
      </div>
      <div className="min-w-0 flex-1">
        <p className="truncate text-xs font-medium" title={snapshot?.session?.title}>
          {snapshot?.session?.title || '等待播放器'}
        </p>
        <p className="text-muted-foreground mt-0.5 truncate text-[11px]">
          {snapshot?.session?.artist || '打开播放器同步歌词'}
        </p>
      </div>
    </section>
  );
}
