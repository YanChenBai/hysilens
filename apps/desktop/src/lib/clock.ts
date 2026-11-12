import type { LyricLine, Snapshot } from '@hysilens/protocol';

/**
 * 由最近一次 snapshot 推算出的歌词位置，单位毫秒，并限制在曲目长度内
 *
 * 没有正在播放的 session 或位置未知时返回 null
 */
export function playbackPosition(state: Snapshot | null, serverNow: number | null): number | null {
  const session = state?.session;

  if (!state || !session || session.positionMs === null) {
    return null;
  }

  let position = session.positionMs;

  if (session.status === 'playing' && session.rate !== null && serverNow !== null) {
    /*
     * 超过两秒没有新的 anchor 就停止外推，这样连接断开时歌词会停住而不是漂移
     */
    const elapsed = Math.min(2000, Math.max(0, serverNow - state.playback.serverMs));
    position += elapsed * session.rate;
  }

  return Math.max(0, Math.min(session.durationMs ?? Infinity, position));
}

/**
 * 覆盖 `position` 的行；`endMs === null` 的行会一直活跃到下一行
 */
export function activeLines(lines: LyricLine[], position: number): LyricLine[] {
  return lines.filter(
    line => line.startMs <= position && (line.endMs === null || position < line.endMs),
  );
}

/**
 * 定时 token 的进度百分比，限制在 `0`–`100`
 */
export function tokenProgress(start: number, end: number, position: number): number {
  if (end <= start) {
    return position >= end ? 100 : 0;
  }

  return Math.min(100, Math.max(0, ((position - start) / (end - start)) * 100));
}
