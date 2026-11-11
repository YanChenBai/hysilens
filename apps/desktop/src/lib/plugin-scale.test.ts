import { expect, it } from 'vite-plus/test';

import { pluginScale } from './plugin-scale';

it('defaults to natural size for missing, invalid or unsupported scale settings', () => {
  for (const value of [undefined, null, '', '1.5', NaN, Infinity, -Infinity]) {
    expect(pluginScale(value, true)).toBe(1);
  }

  expect(pluginScale(2, false)).toBe(1);
});

it('preserves fractional zoom and bounds the rendered plugin canvas', () => {
  expect(pluginScale(1.25, true)).toBe(1.25);
  expect(pluginScale(0.5, true)).toBe(0.5);
  expect(pluginScale(-2, true)).toBe(0.25);
  expect(pluginScale(9, true)).toBe(3);
});
