import type { LyricDocument } from '@hysilens/protocol';
import { describe, expect, it } from 'vite-plus/test';

import { ignoreLyrics, lyricLineId, lyricSongId } from './ignored-lyrics';

const document: LyricDocument = {
  id: 'cache-v1',
  filename: 'cache.lrc',
  warnings: [],
  metadata: { title: ' Song  Name ', artist: 'ARTIST', album: 'Ａlbum' },
  tracks: [
    {
      kind: 'original',
      lines: [
        { startMs: 0, endMs: 900, text: '演唱句', tokens: [] },
        {
          startMs: 1000,
          endMs: 1800,
          text: '吉他：某某',
          tokens: [{ startMs: 1000, endMs: 1800, text: '吉他：某某' }],
        },
        { startMs: 2000, endMs: null, text: '下一句', tokens: [] },
      ],
    },
    { kind: 'translation', lines: [{ startMs: 0, endMs: null, text: 'Translation', tokens: [] }] },
  ],
};

describe('ignored lyrics', () => {
  it('uses normalized metadata across cache versions and preserves field boundaries', () => {
    const updated = {
      ...document,
      id: 'cache-v2',
      filename: 'renamed',
      metadata: { title: 'song name', artist: 'artist', album: 'Album' },
    };

    expect(lyricSongId(document, null)).toBe(lyricSongId(updated, null));
    expect(
      lyricSongId({ ...document, metadata: { title: 'a-b', artist: 'c', album: '' } }, null),
    ).not.toBe(
      lyricSongId({ ...document, metadata: { title: 'a', artist: 'b-c', album: '' } }, null),
    );
    expect(lyricSongId({ ...document, metadata: { title: '', artist: '', album: '' } }, null)).toBe(
      'document:cache-v1',
    );
  });

  it('removes selected content and overlapping secondary spans without blank placeholders', () => {
    const result = ignoreLyrics(document, [lyricLineId(document.tracks[0]!.lines[1]!)]);
    expect(result.intervals).toEqual([{ start: 1000, end: 2000 }]);
    expect(result.document.tracks[0]!.lines.map(line => line.text)).toEqual(['演唱句', '下一句']);
    expect(result.document.tracks[1]!.lines).toEqual([
      { startMs: 0, endMs: 1000, text: 'Translation', tokens: [] },
      { startMs: 2000, endMs: null, text: 'Translation', tokens: [] },
    ]);
    expect(document.tracks[0]!.lines[1]!.text).toBe('吉他：某某');
    expect(ignoreLyrics(document, [lyricLineId(document.tracks[0]!.lines[1]!)])).toBe(result);
  });

  it('leaves unselected lyrics alone and does not apply stale selections to changed rows', () => {
    expect(ignoreLyrics(document, []).document).toBe(document);
    const stale = lyricLineId({ ...document.tracks[0]!.lines[1]!, text: '另一句' });
    expect(ignoreLyrics(document, [stale]).document).toBe(document);
    const last = ignoreLyrics(document, [lyricLineId(document.tracks[0]!.lines[2]!)]);
    expect(last.intervals).toEqual([{ start: 2000, end: Infinity }]);
    expect(last.document.tracks[0]!.lines.map(line => line.text)).toEqual(['演唱句', '吉他：某某']);
  });

  it('removes consecutive leading credits so upcoming lyrics can be laid out immediately', () => {
    const result = ignoreLyrics(document, document.tracks[0]!.lines.slice(0, 2).map(lyricLineId));
    expect(result.document.tracks[0]!.lines).toEqual([document.tracks[0]!.lines[2]]);
    expect(result.document.tracks[1]!.lines).toEqual([
      { startMs: 2000, endMs: null, text: 'Translation', tokens: [] },
    ]);
  });

  it('preserves the previous line end when its following row is removed', () => {
    const untimed = {
      ...document,
      tracks: [
        {
          kind: 'original' as const,
          lines: document.tracks[0]!.lines.map(line => ({ ...line, endMs: null })),
        },
      ],
    };

    const result = ignoreLyrics(untimed, [lyricLineId(untimed.tracks[0]!.lines[1]!)]);
    expect(result.document.tracks[0]!.lines[0]!.endMs).toBe(1000);
  });
});
