import type { ClockAnchor, LyricLine } from '@hysilens/plugin';

/** A simulated player: position is `anchor` plus elapsed time since `anchorAt` at `rate`. */
export interface Transport {
  playing: boolean;
  rate: number;
  loop: boolean;
  durationMs: number;
  anchor: number;
  anchorAt: number;
}

export function createTransport(durationMs: number, now: number): Transport {
  return { playing: true, rate: 1, loop: true, durationMs, anchor: 0, anchorAt: now };
}

const clamp = (value: number, max: number) => Math.min(max, Math.max(0, value));

export function positionAt(transport: Transport, now: number): number {
  if (!transport.playing) {
    return transport.anchor;
  }

  const raw = transport.anchor + (now - transport.anchorAt) * transport.rate;

  if (raw < transport.durationMs) {
    return raw;
  }

  return transport.loop ? raw % transport.durationMs : transport.durationMs;
}

/** Playing without loop past the end counts as stopped for the plugin clock. */
export function isRunning(transport: Transport, now: number): boolean {
  return transport.playing && (transport.loop || positionAt(transport, now) < transport.durationMs);
}

function rebase(transport: Transport, now: number): Transport {
  return { ...transport, anchor: positionAt(transport, now), anchorAt: now };
}

export function seek(transport: Transport, position: number, now: number): Transport {
  return { ...transport, anchor: clamp(position, transport.durationMs), anchorAt: now };
}

export function setPlaying(transport: Transport, playing: boolean, now: number): Transport {
  const next = rebase(transport, now);

  // Pressing play at the very end of a non-looping run starts over, like a media player.
  if (playing && next.anchor >= next.durationMs) {
    return { ...next, playing, anchor: 0 };
  }

  return { ...next, playing };
}

export function setRate(transport: Transport, rate: number, now: number): Transport {
  return { ...rebase(transport, now), rate };
}

export function setLoop(transport: Transport, loop: boolean, now: number): Transport {
  return { ...rebase(transport, now), loop };
}

/** Load a new sample: rewind and keep the current play state, speed and loop setting. */
export function load(transport: Transport, durationMs: number, now: number): Transport {
  return { ...transport, durationMs, anchor: 0, anchorAt: now };
}

export function clockAnchor(transport: Transport, now: number): ClockAnchor {
  return {
    position: positionAt(transport, now),
    rate: isRunning(transport, now) ? transport.rate : 0,
  };
}

export function lineIndexAt(lines: LyricLine[], time: number): number {
  return lines.findLastIndex(line => line.startMs <= time);
}

/** Like a player's back button: restart the current line unless just past its start. */
export function previousLineStart(lines: LyricLine[], time: number): number {
  const index = lineIndexAt(lines, time);
  const current = lines[index];

  // Before the first line there is nothing earlier to go to but the very start.
  if (!current) {
    return 0;
  }

  if (time - current.startMs > 1000) {
    return current.startMs;
  }

  return lines[index - 1]?.startMs ?? 0;
}

export function nextLineStart(lines: LyricLine[], time: number): number | null {
  return lines[lineIndexAt(lines, time) + 1]?.startMs ?? null;
}

export function formatTime(ms: number): string {
  const total = Math.max(0, Math.floor(ms));
  const minutes = Math.floor(total / 60_000);
  const seconds = Math.floor((total % 60_000) / 1000);
  const millis = total % 1000;

  return `${minutes}:${String(seconds).padStart(2, '0')}.${String(millis).padStart(3, '0')}`;
}
