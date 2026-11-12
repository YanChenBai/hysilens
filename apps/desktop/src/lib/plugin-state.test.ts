import { demo } from '@hysilens/plugin/demo';
import type { Snapshot } from '@hysilens/protocol';
import { describe, expect, it } from 'vite-plus/test';

import { lyricLineId, lyricSongId } from './ignored-lyrics';
import { pluginFrameState } from './plugin-state';
import type { SessionState } from './session';

function session(overrides: Partial<Snapshot['session'] & object> = {}, offset = 0): SessionState {
  const current = {
    sourceId: 'qq',
    title: '',
    artist: '',
    album: '',
    status: 'playing',
    positionMs: 10_000,
    durationMs: 200_000,
    sourcePositionMs: null,
    sourceUpdatedUnixMs: null,
    rate: 1,
    rateBasis: '',
    error: null,
    ...overrides,
  };

  return {
    connection: 'connected',
    lastStateAt: 1000,
    clockOffset: 0,
    error: '',
    snapshot: {
      overlay: { offset } as Snapshot['overlay'],
      source: 'auto',
      version: 1,
      epoch: '',
      seq: 0,
      generation: 0,
      playback: { serverMs: 1000, sessions: [], error: null },
      session: current,
      lyricsStatus: '',
      candidates: [],
      document: demo,
      offsetMs: 0,
      showRomanization: true,
      showTranslation: true,
      cacheIssues: [],
    },
  } as SessionState;
}

describe('pluginFrameState', () => {
  it('keeps the plugin visible during ignored rows and only removes selected content', () => {
    const current = session({ positionMs: 5500 }, 500);
    const snapshot = current.snapshot!;

    snapshot.overlay = {
      ...snapshot.overlay!,
      ignoredLyricLines: {
        [lyricSongId(demo, snapshot.session)]: [lyricLineId(demo.tracks[0]!.lines[1]!)],
      },
    };

    const options = { playground: false, startedAt: 0, now: 1000 };
    const filtered = pluginFrameState(current, options);

    expect(filtered.playback.visible).toBe(true);
    expect(filtered.lyrics!.tracks[0]!.lines).toHaveLength(demo.tracks[0]!.lines.length - 1);
    expect(
      filtered.lyrics!.tracks[0]!.lines.some(
        line => line.startMs === demo.tracks[0]!.lines[1]!.startMs,
      ),
    ).toBe(false);

    snapshot.session!.positionMs = 11499;
    expect(pluginFrameState(current, options).playback.visible).toBe(true);

    snapshot.session!.positionMs = 11500;
    expect(pluginFrameState(current, options).playback.visible).toBe(true);

    snapshot.session!.positionMs = 5499;
    expect(pluginFrameState(current, options).playback.visible).toBe(true);

    snapshot.overlay.ignoredLyricLines = {};
    expect(pluginFrameState(current, options).lyrics).toBe(demo);
  });
  it('applies the overlay offset once and keeps the playback rate', () => {
    const state = pluginFrameState(session({}, 250), {
      playground: false,
      startedAt: 0,
      now: 1500,
    });

    expect(state.clock).toEqual({ position: 10_750, rate: 1 });
    expect(state.playback).toEqual({ playing: true, visible: true });
    expect(state.lyrics).toBe(demo);
  });

  it('freezes time while paused or without a known rate', () => {
    const paused = pluginFrameState(session({ status: 'paused' }), {
      playground: false,
      startedAt: 0,
      now: 1500,
    });

    expect(paused.clock).toEqual({ position: 10_000, rate: 0 });
    expect(paused.playback.playing).toBe(false);

    const unknown = pluginFrameState(session({ rate: null }), {
      playground: false,
      startedAt: 0,
      now: 1500,
    });

    expect(unknown.clock.rate).toBe(0);
  });

  it('hides lyrics for stale streams and closed players', () => {
    const options = { playground: false, startedAt: 0, now: 5000 };
    expect(pluginFrameState(session(), options).playback.visible).toBe(false);

    const closed = pluginFrameState(session({ status: 'closed' }), { ...options, now: 1500 });
    expect(closed.playback.visible).toBe(false);
  });

  it('loops demo lyrics in the playground', () => {
    const state = pluginFrameState(session(), { playground: true, startedAt: 0, now: 40_000 });

    expect(state).toEqual({
      lyrics: demo,
      playback: { playing: true, visible: true },
      clock: { position: 4000, rate: 1 },
    });
  });
});
