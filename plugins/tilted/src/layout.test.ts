import { describe, expect, it } from 'vite-plus/test';

import { centeredWindow } from './layout';

function measuredRows(heights: number[]) {
  let top = 0;

  return heights.map(height => {
    const row = { top, height };
    top += height;

    return row;
  });
}

describe('tilted current-line anchoring', () => {
  const rows = measuredRows(Array.from({ length: 10 }, () => 100));

  it('reserves blank space above the first line instead of filling it with upcoming lines', () => {
    expect(centeredWindow(rows, 0, 5, 1.4)).toEqual({
      start: 0,
      end: 3,
      height: 500,
      translateY: 200,
    });
  });

  it('reserves blank space below the last line without shifting focus down', () => {
    expect(centeredWindow(rows, 9, 5, 1.4)).toEqual({
      start: 7,
      end: 10,
      height: 500,
      translateY: -700,
    });
  });

  it('keeps wrapped lines centred even when neighbouring rows have different heights', () => {
    const wrapped = measuredRows([80, 200, 90, 160, 70, 100, 180]);

    for (const focus of [0, 1, 3, 6]) {
      for (const count of [1, 2, 5, 15]) {
        const window = centeredWindow(wrapped, focus, count, 1.4)!;
        const active = wrapped[focus];
        expect(active.top + active.height / 2 + window.translateY).toBeCloseTo(
          window.height / 2,
          8,
        );
        expect(window.end - window.start).toBeLessThanOrEqual(count);
        expect(window.height).toBeGreaterThanOrEqual(active.height * 1.4);
        expect(wrapped[window.start].top + window.translateY).toBeGreaterThanOrEqual(0);
        const last = wrapped[window.end - 1];
        expect(last.top + last.height + window.translateY).toBeLessThanOrEqual(window.height);
      }
    }
  });

  it('centres a single remaining lyric and anchors upcoming lyrics before playback starts', () => {
    expect(centeredWindow(rows, -1, 5, 1.4)).toEqual(centeredWindow(rows, 0, 5, 1.4));
    const one = measuredRows([100]);
    const window = centeredWindow(one, 0, 1, 1.4)!;
    expect(window.height).toBe(140);
    expect(window.translateY).toBe(20);
    expect(centeredWindow([], -1, 5, 1.4)).toBeNull();
  });
});
