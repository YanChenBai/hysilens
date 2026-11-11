import type { LyricLine, Snapshot } from '@hysilens/protocol';

export function playbackPosition(state: Snapshot | null, serverNow: number | null): number | null {
  const session = state?.session;

  if (!state || !session || session.positionMs === null) {
    return null;
  }

  let position = session.positionMs;

  if (session.status === 'playing' && session.rate !== null && serverNow !== null) {
    // 超过两秒没收到新锚点就停止外推，防止断线后歌词继续前进。
    const elapsed = Math.min(2000, Math.max(0, serverNow - state.playback.serverMs));
    position += elapsed * session.rate;
  }

  return Math.max(0, Math.min(session.durationMs ?? Infinity, position));
}

export function activeLines(lines: LyricLine[], position: number): LyricLine[] {
  return lines.filter(
    line => line.startMs <= position && (line.endMs === null || position < line.endMs),
  );
}

export function tokenProgress(start: number, end: number, position: number): number {
  if (end <= start) {
    return position >= end ? 100 : 0;
  }

  return Math.min(100, Math.max(0, ((position - start) / (end - start)) * 100));
}
