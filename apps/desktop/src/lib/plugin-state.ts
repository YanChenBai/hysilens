import type { ClockAnchor, LyricDocument, Playback } from '@hysilens/plugin';
import { DEMO_DURATION_MS, demo } from '@hysilens/plugin/demo';
import { parseOverlayConfig } from '@hysilens/protocol/config';

import { playbackPosition } from './clock';
import { ignoredLyricState } from './ignored-lyrics';
import type { SessionState } from './session';

export interface PluginFrameState {
  lyrics: LyricDocument | null;
  playback: Playback;
  clock: ClockAnchor;
}

/** Stop showing lyrics once the stream goes quiet or the player has nothing to play. */
function visible(session: SessionState, now: number): boolean {
  const status = session.snapshot?.session?.status ?? 'closed';

  return (
    session.connection === 'connected' &&
    now - session.lastStateAt < 3000 &&
    !['stopped', 'closed'].includes(status)
  );
}

/**
 * Everything a plugin learns from the host besides its config. `clock.position` is lyric
 * time with the overlay offset applied once, so plugins never handle offsets themselves.
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
      // Without a rate the host does not extrapolate either, so neither may the plugin.
      rate: playing && current?.rate != null ? current.rate : 0,
    },
  };
}
