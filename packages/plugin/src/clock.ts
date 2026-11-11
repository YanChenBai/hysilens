import { MAX_EXTRAPOLATION_MS } from './protocol';
import type { ClockAnchor } from './protocol';

/** Corrections at least this large are seeks or song changes and apply immediately. */
export const CLOCK_SNAP_MS = 400;

/**
 * Smaller corrections are absorbed linearly over this window. Keeping it longer than
 * `CLOCK_SNAP_MS` means displayed time never runs backwards while playing at rate 1.
 */
export const CLOCK_SLEW_MS = 600;

/** Project an anchor received at `receivedAt` to `now`, bounded so a lost host freezes time. */
export function project(anchor: ClockAnchor, receivedAt: number, now: number): number | null {
  if (anchor.position === null) {
    return null;
  }

  const elapsed = Math.min(MAX_EXTRAPOLATION_MS, Math.max(0, now - receivedAt));

  return anchor.position + elapsed * anchor.rate;
}

export interface Clock {
  readonly set: (anchor: ClockAnchor) => void;
  readonly time: () => number | null;
}

/**
 * Lyric time seen by a plugin. Sampled playback positions jitter by tens of milliseconds, so
 * small corrections are slewed in rather than applied as visible jumps in word animations.
 */
export function createClock(initial: ClockAnchor, now: () => number): Clock {
  let anchor = initial;
  let receivedAt = now();
  // Displayed minus authoritative time when the latest correction arrived; decays to zero.
  let error = 0;
  let slewStart = receivedAt;

  function time(): number | null {
    const at = now();
    const target = project(anchor, receivedAt, at);

    if (target === null || error === 0) {
      return target;
    }

    const remaining = 1 - Math.min(1, (at - slewStart) / CLOCK_SLEW_MS);

    return target + error * remaining;
  }

  return {
    set: next => {
      const displayed = time();
      anchor = next;
      receivedAt = now();
      slewStart = receivedAt;

      const target = project(anchor, receivedAt, receivedAt);
      const delta = displayed === null || target === null ? Infinity : displayed - target;

      // Paused, unknown or seeking: show the authoritative position exactly.
      error = next.rate === 0 || Math.abs(delta) >= CLOCK_SNAP_MS ? 0 : delta;
    },
    time,
  };
}
