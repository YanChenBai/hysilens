import { expect, it } from 'vite-plus/test';

import { overlayDefaults, parseOverlayConfig } from './overlay-config';

it('validates WebSocket configuration and falls back only for invalid fields', () => {
  expect(parseOverlayConfig(undefined).options).toEqual(overlayDefaults);

  const parsed = parseOverlayConfig({
    fontSize: NaN,
    color: 'red',
    activeColor: '#ABCDEF',
    romanization: false,
    lineGap: 4,
  });

  expect(parsed.options.fontSize).toBe(42);
  expect(parsed.options.color).toBe(overlayDefaults.color);
  expect(parsed.options.activeColor).toBe('#abcdef');
  expect(parsed.options.romanization).toBe(false);
  expect(parsed.options.lineGap).toBe(4);
  expect(parsed.issues).toEqual(['color', 'fontSize']);
  expect(
    parseOverlayConfig({ offset: 20000, translation: 'unknown', fontFamily: 'x;color:red' }).issues,
  ).toEqual(['fontFamily', 'offset', 'translation']);
});

it('accepts two or three scrolling lines and defaults older configurations to three', () => {
  expect(parseOverlayConfig({ visibleLines: 2 }).options.visibleLines).toBe(2);
  expect(parseOverlayConfig({ visibleLines: 3 }).options.visibleLines).toBe(3);
  expect(parseOverlayConfig({}).options.visibleLines).toBe(3);

  for (const visibleLines of [0, 1, 4, 2.5, '2', null]) {
    const parsed = parseOverlayConfig({ visibleLines });

    expect(parsed.options.visibleLines).toBe(3);
    expect(parsed.issues).toContain('visibleLines');
  }
});
