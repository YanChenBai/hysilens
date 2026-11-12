import type { Snapshot } from '@hysilens/protocol';
import { describe, expect, it } from 'vite-plus/test';

import { activeLines, playbackPosition, tokenProgress } from './clock';

function snapshot(status = 'playing', rate: number | null = 1): Snapshot {
  return {
    version: 1,
    epoch: 'test',
    seq: 1,
    generation: 1,
    playback: { serverMs: 5000, sessions: [], error: null },
    session: {
      sourceId: 'QQMusic.exe',
      title: 'A',
      artist: 'B',
      album: 'C',
      status,
      positionMs: 1000,
      durationMs: 10000,
      sourcePositionMs: 1000,
      sourceUpdatedUnixMs: null,
      rate,
      rateBasis: 'reported',
      error: null,
    },
    source: 'auto',
    lyricsStatus: 'manual',
    candidates: [],
    document: null,
    offsetMs: 0,
    showRomanization: true,
    showTranslation: true,
    cacheIssues: [],
  };
}

describe('playback clock', () => {
  it('uses mapped time, freezes pauses and bounds disconnection drift', () => {
    expect(playbackPosition(snapshot(), 5500)).toBe(1500);
    expect(playbackPosition(snapshot('paused'), 5500)).toBe(1000);
    expect(playbackPosition(snapshot('playing', null), 5500)).toBe(1000);
    expect(playbackPosition(snapshot(), 20000)).toBe(3000);
    expect(playbackPosition(snapshot(), 4000)).toBe(1000);
  });

  it('keeps overlapping vocals and honors line ends', () => {
    const lines = [
      { startMs: 1000, endMs: 2000, text: 'A', tokens: [] },
      { startMs: 1500, endMs: 2500, text: 'B', tokens: [] },
    ];

    expect(activeLines(lines, 1500).map(line => line.text)).toEqual(['A', 'B']);
    expect(activeLines(lines, 2000).map(line => line.text)).toEqual(['B']);
    expect(activeLines(lines, 2500)).toEqual([]);
    expect(tokenProgress(1000, 2000, 1500)).toBe(50);
    expect(tokenProgress(1000, 1000, 1000)).toBe(100);
  });

  it('keeps lyric time advancing without total duration and freezes on pause', () => {
    const state = snapshot();
    state.session!.sourceId = 'kugou';
    state.session!.positionMs = 123548.812;
    state.session!.durationMs = null;

    expect(playbackPosition(state, 5500)).toBe(124048.812);
    expect(playbackPosition(state, 20000)).toBe(125548.812);

    state.session!.status = 'paused';
    expect(playbackPosition(state, 5500)).toBe(123548.812);

    state.session!.positionMs = 90000;
    expect(playbackPosition(state, 5500)).toBe(90000);
  });
});
