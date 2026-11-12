import { afterEach, describe, expect, it, vi } from 'vite-plus/test';

import { cloneValue } from './clone';

afterEach(() => vi.unstubAllGlobals());

describe('cloneValue', () => {
  it('preserves native structured-clone values when available', () => {
    const source = { date: new Date('2026-01-01T00:00:00Z') };
    const copy = cloneValue(source);

    expect(copy.date).toBeInstanceOf(Date);
    expect(copy.date).toEqual(source.date);
    expect(copy.date).not.toBe(source.date);
  });

  it('copies nested JSON configuration without a native implementation', () => {
    vi.stubGlobal('structuredClone', undefined);
    const source = { lines: [{ text: '歌词', active: true }], offset: null };
    const copy = cloneValue(source);

    expect(copy).toEqual(source);
    copy.lines[0].text = '另一句';

    expect(source.lines[0].text).toBe('歌词');
    expect(globalThis.structuredClone).toBeUndefined();
  });

  it('preserves an absent configuration without a native implementation', () => {
    vi.stubGlobal('structuredClone', undefined);

    expect(cloneValue(undefined)).toBeUndefined();
  });
});
