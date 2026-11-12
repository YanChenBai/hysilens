import { describe, expect, it } from 'vite-plus/test';

import { defaultSample, samples } from './samples';

const original = (id: string) =>
  samples.find(sample => sample.id === id)?.document.tracks.find(track => track.kind === 'original')
    ?.lines ?? [];

describe('lyric samples', () => {
  it('have unique ids and a default from the list', () => {
    expect(new Set(samples.map(sample => sample.id)).size).toBe(samples.length);
    expect(samples).toContain(defaultSample);
  });

  it.each(samples.map(sample => [sample.id, sample] as const))(
    '%s keeps a consistent timeline',
    (_id, sample) => {
      const lines = original(sample.id);

      expect(lines.length).toBeGreaterThan(3);

      for (const [index, line] of lines.entries()) {
        expect(line.startMs).toBeGreaterThanOrEqual(lines[index - 1]?.startMs ?? 0);
        expect(sample.durationMs).toBeGreaterThan(line.startMs);

        for (const [position, token] of line.tokens.entries()) {
          expect(token.startMs).toBeGreaterThanOrEqual(line.startMs);
          expect(token.endMs).toBeGreaterThan(token.startMs);
          expect(token.startMs).toBeGreaterThanOrEqual(line.tokens[position - 1]?.endMs ?? 0);

          if (line.endMs !== null) {
            expect(token.endMs).toBeLessThanOrEqual(line.endMs);
          }
        }

        if (line.tokens.length) {
          expect(line.tokens.map(token => token.text).join('')).toBe(line.text);
        }
      }

      const starts = new Set(lines.map(line => line.startMs));

      for (const track of sample.document.tracks.filter(track => track.kind !== 'original')) {
        expect(track.lines.every(line => starts.has(line.startMs))).toBe(true);
      }
    },
  );

  it('cover the edge cases renderers must survive', () => {
    const lines = original('edge');
    const gaps = lines.slice(1).map((line, index) => line.startMs - (lines[index]?.endMs ?? 0));

    expect(lines.some(line => line.text === '')).toBe(true);
    expect(lines.some(line => line.text && !line.tokens.length)).toBe(true);
    expect(lines.some(line => line.endMs === null)).toBe(true);
    expect(lines.some(line => line.text.length > 40)).toBe(true);
    expect(lines.some(line => line.tokens.some(token => token.endMs - token.startMs < 100))).toBe(
      true,
    );
    expect(gaps.some(gap => gap < 0)).toBe(true);
    expect(gaps.some(gap => gap >= 10_000)).toBe(true);
  });

  it('time English lyrics by word with rests between phrases', () => {
    const lines = original('en');

    expect(lines.every(line => line.tokens.length > 1 || line.text.length < 6)).toBe(true);
    expect(
      lines.some(line =>
        line.tokens.slice(1).some((token, index) => token.startMs > line.tokens[index]!.endMs),
      ),
    ).toBe(true);
  });
});
