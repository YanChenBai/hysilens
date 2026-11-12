import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

import { describe, expect, it } from 'vite-plus/test';

const html = readFileSync(new URL('../../index.html', import.meta.url), 'utf8');
const script = html.match(/<script>([\s\S]*?)<\/script>/)![1];

function startup(pathname: string, saved: string | null = null, systemDark = false) {
  const classes = new Set<string>();
  const style: Record<string, string> = {};

  runInNewContext(script, {
    location: {
      pathname,
      search: pathname.includes('?') ? pathname.slice(pathname.indexOf('?')) : '',
    },
    localStorage: {
      getItem() {
        if (saved === 'unavailable') {
          throw new Error('Storage unavailable');
        }

        return saved;
      },
    },
    matchMedia: () => ({ matches: systemDark }),
    URLSearchParams,
    document: {
      documentElement: {
        style,
        classList: {
          add: (name: string) => classes.add(name),
          toggle(name: string, enabled: boolean) {
            if (enabled) {
              classes.add(name);
            } else {
              classes.delete(name);
            }
          },
        },
      },
    },
  });

  return { classes, style };
}

describe('control first paint', () => {
  it.each(['/control', '/settings'])('defaults %s to dark before React loads', pathname => {
    expect([...startup(pathname).classes]).toEqual(['control', 'dark']);
  });

  it('honours saved light and dark themes before React loads', () => {
    expect(startup('/control', 'light', true).classes.has('dark')).toBe(false);
    expect(startup('/control', 'dark').classes.has('dark')).toBe(true);
  });

  it.each([true, false])('resolves system theme when dark preference is %s', dark => {
    expect(startup('/control', 'system', dark).classes.has('dark')).toBe(dark);
  });

  it.each(['unavailable', 'invalid'])('keeps the default theme when storage is %s', saved => {
    expect(startup('/control', saved).classes.has('dark')).toBe(true);
  });

  it('leaves the transparent overlay untouched', () => {
    const result = startup('/');

    expect([...result.classes]).toEqual([]);
    expect(result.style).toEqual({});
  });

  it.each([
    ['dark', 'dark', '#000'],
    ['light', 'light', '#fafafa'],
    ['system', 'light dark', 'light-dark(#fafafa, #000)'],
  ])('preserves the %s preview background', (preview, scheme, background) => {
    expect(startup(`/?preview=${preview}`).style).toEqual({
      colorScheme: scheme,
      background,
    });
  });
});
