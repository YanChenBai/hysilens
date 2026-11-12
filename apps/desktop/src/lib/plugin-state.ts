import type { ClockAnchor, LyricDocument, Playback } from '@hysilens/plugin';
import { DEMO_DURATION_MS, demo } from '@hysilens/plugin/demo';
import { parseOverlayConfig } from '@hysilens/protocol/config';

import { playbackPosition } from './clock';
import { ignoredLyricState } from './ignored-lyrics';
import type { SessionState } from './session';

/**
 * plugin 每 frame 从 host 收到的输入
 */
export interface PluginFrameState {
  /**
   * 已移除忽略区间的歌词，未加载时为 `null`
   */
  lyrics: LyricDocument | null;

  /**
   * playback 标志，包括 overlay 当前是否应该显示内容
   */
  playback: Playback;

  /**
   * 歌词 clock anchor；`position` 是已应用 overlay 偏移的歌词时间，单位毫秒
   */
  clock: ClockAnchor;
}

/**
 * stream 静默或播放器没有可播放内容时停止显示歌词
 *
 * 状态 snapshot 超过 3000 ms、socket 断开、播放器处于 stopped/closed 状态都视为不可见
 */
function visible(session: SessionState, now: number): boolean {
  const status = session.snapshot?.session?.status ?? 'closed';

  return (
    session.connection === 'connected' &&
    now - session.lastStateAt < 3000 &&
    !['stopped', 'closed'].includes(status)
  );
}

/**
 * plugin 从 host 获得的除 config 之外的全部信息，`clock.position` 是已应用一次
 * overlay 偏移的歌词时间，因此 plugin 自己无需处理 offset
 *
 * @param session 当前 host session 状态，包含连接存活情况
 * @param options `playground` 会让演示循环持续运行；`startedAt` 与 `now` 是
 * 单位为毫秒的 performance 时间
 */
export function pluginFrameState(
  session: SessionState,
  options: { playground: boolean; startedAt: number; now: number },
): PluginFrameState {
  if (options.playground) {
    return {
      lyrics: demo,
      playback: { playing: true, visible: true },
      clock: { position: (options.now - options.startedAt) % DEMO_DURATION_MS, rate: 1 },
    };
  }

  const snapshot = session.snapshot;
  const current = snapshot?.session;
  const overlay = parseOverlayConfig(snapshot?.overlay).options;
  const offset = overlay.offset;
  const serverNow = session.clockOffset === null ? null : options.now + session.clockOffset;
  const position = playbackPosition(snapshot, serverNow);
  const playing = current?.status === 'playing';
  const lyricTime = position === null ? null : position + offset;
  const filtered = ignoredLyricState(snapshot, overlay);

  return {
    lyrics: filtered.document,
    playback: { playing, visible: visible(session, options.now) },
    clock: {
      position: lyricTime,

      /* 没有 rate 时 host 也不外推时间，因此 plugin 同样不能外推 */
      rate: playing && current?.rate != null ? current.rate : 0,
    },
  };
}
