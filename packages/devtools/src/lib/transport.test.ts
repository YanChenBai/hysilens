import type { LyricLine } from '@hysilens/plugin';
import { describe, expect, it } from 'vite-plus/test';

import {
  clockAnchor,
  createTransport,
  formatTime,
  load,
  nextLineStart,
  positionAt,
  previousLineStart,
  seek,
  setLoop,
  setPlaying,
  setRate,
} from './transport';

const line = (startMs: number): LyricLine => ({ text: 'x', startMs, endMs: null, tokens: [] });

describe('transport', () => {
  it('advances while playing, holds while paused and keeps position across changes', () => {
    let transport = createTransport(10_000, 0);
    expect(positionAt(transport, 1500)).toBe(1500);

    transport = setPlaying(transport, false, 1500);
    expect(positionAt(transport, 9000)).toBe(1500);
    expect(clockAnchor(transport, 9000)).toEqual({ position: 1500, rate: 0 });

    transport = setPlaying(transport, true, 9000);
    transport = setRate(transport, 2, 9500);
    expect(positionAt(transport, 10_500)).toBe(4000);
    expect(clockAnchor(transport, 10_500)).toEqual({ position: 4000, rate: 2 });
  });

  it('loops, or stops at the end and restarts on play', () => {
    let transport = seek(createTransport(10_000, 0), 9000, 0);
    expect(positionAt(transport, 2500)).toBe(1500);

    transport = setLoop(transport, false, 0);
    expect(positionAt(transport, 2500)).toBe(10_000);
    expect(clockAnchor(transport, 2500)).toEqual({ position: 10_000, rate: 0 });

    transport = setPlaying(setPlaying(transport, false, 2500), true, 3000);
    expect(positionAt(transport, 3000)).toBe(0);
  });

  it('clamps seeks and rewinds when loading another sample', () => {
    const transport = createTransport(10_000, 0);
    expect(positionAt(seek(transport, -50, 0), 0)).toBe(0);
    const paused = setPlaying(transport, false, 0);
    expect(positionAt(seek(paused, 99_999, 0), 0)).toBe(10_000);

    const loaded = load(seek(transport, 5000, 0), 30_000, 100);
    expect(loaded.durationMs).toBe(30_000);
    expect(positionAt(loaded, 100)).toBe(0);
    expect(loaded.playing).toBe(true);
  });

  it('steps between lines like a media player', () => {
    const lines = [line(1000), line(5000), line(9000)];
    expect(nextLineStart(lines, 0)).toBe(1000);
    expect(nextLineStart(lines, 5200)).toBe(9000);
    expect(nextLineStart(lines, 9500)).toBeNull();
    expect(previousLineStart(lines, 7000)).toBe(5000);
    expect(previousLineStart(lines, 5300)).toBe(1000);
    expect(previousLineStart(lines, 500)).toBe(0);
  });

  it('formats times with millisecond precision', () => {
    expect(formatTime(0)).toBe('0:00.000');
    expect(formatTime(83_456.9)).toBe('1:23.456');
  });
});
