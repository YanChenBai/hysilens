import { expect, it } from 'vite-plus/test';

import { canSlide, focusLine } from './lyric-window';
it('keeps context through gaps and preserves empty timed rows', () => {
  const lines = [
    { startMs: 1000, endMs: 2000, text: 'A', tokens: [] },
    { startMs: 3000, endMs: 4000, text: '', tokens: [] },
    { startMs: 5000, endMs: 6000, text: 'B', tokens: [] },
  ];

  expect(focusLine(lines, null)).toBe(-1);
  expect(focusLine(lines, 0)).toBe(-1);
  expect(focusLine(lines, 2500)).toBe(0);
  expect(focusLine(lines, 3000)).toBe(1);
  expect(focusLine(lines, 6000)).toBe(2);
});
it('slides normal adjacent progression and snaps seeks or unavailable positions', () => {
  expect(canSlide(1, 2, 1990, 2010)).toBe(true);
  expect(canSlide(1, 2, 1000, 10000)).toBe(false);
  expect(canSlide(2, 1, 2010, 1990)).toBe(false);
  expect(canSlide(1, 3, 1990, 2010)).toBe(false);
  expect(canSlide(-1, 0, null, 1000)).toBe(false);
});
