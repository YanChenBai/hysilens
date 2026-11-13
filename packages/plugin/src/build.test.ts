import { readFile, rm } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { runInNewContext } from 'node:vm';

import { strFromU8, unzipSync } from 'fflate';
import { afterAll, describe, expect, it, vi } from 'vite-plus/test';

import config from '../test/fixtures/leaky/hysilens.config';
import { buildPlugin, packPlugin } from './cli';
import { PluginHost } from './host';
import {
  assertExternal,
  isSafePath,
  userPluginsDirectory,
  validatePluginDirectory,
} from './manifest';
import { isBuiltinId, localName } from './protocol';

const fixtures = resolve(import.meta.dirname, '../test/fixtures');

afterAll(async () => {
  await rm(join(fixtures, 'basic/dist'), { recursive: true, force: true });
  await rm(join(fixtures, 'leaky/dist'), { recursive: true, force: true });
});

describe('hysilens build', () => {
  it('emits a self-contained, valid plugin directory', async () => {
    const manifest = await buildPlugin(join(fixtures, 'basic'), { logLevel: 'silent' });
    const dist = join(fixtures, 'basic/dist');

    expect(manifest).toMatchObject({
      apiVersion: 1,
      id: 'basic',
      name: '构建测试',
      version: '1.2.3',
      entry: 'index.js',
      viewport: { width: 640, height: 360 },
      features: { font: true, scale: true },
    });
    expect(manifest.revision).toMatch(/^[0-9a-f]{64}$/);
    expect(manifest.styles).toHaveLength(1);
    expect(Object.keys(manifest.configSchema.properties as object).sort()).toEqual([
      'accent',
      'fontFamily',
      'scale',
      'size',
    ]);

    const entry = await readFile(join(dist, 'index.js'), 'utf8');

    expect(entry).toContain('hysilens:connect');
    expect(entry).not.toMatch(/toJSONSchema|ZodObject/);
    /* 即使入口不是 HTML 应用，依赖库也必须看到生产环境的 NODE_ENV */
    expect(entry).not.toContain('process.env');
    expect(entry).toContain('production');
    expect((await validatePluginDirectory(dist, 'basic')).errors).toEqual([]);
  }, 60_000);

  it('replaces config value imports with matching data without bundling zod', async () => {
    await buildPlugin(join(fixtures, 'leaky'), { logLevel: 'silent' });

    const entry = await readFile(join(fixtures, 'leaky/dist/index.js'), 'utf8');

    const root = { dataset: {}, style: {}, textContent: '' };
    const listeners = new Map<string, (event: MessageEvent) => void>();
    const parent = { postMessage() {} };

    const context = {
      AbortController,
      performance,
      document: {
        baseURI: 'https://example.com/plugins/leaky/',
        createElement: () => root,
        body: { append() {} },
      },
      window: {
        addEventListener: (type: string, listener: (event: MessageEvent) => void) => {
          listeners.set(type, listener);
        },
        removeEventListener: (type: string) => {
          listeners.delete(type);
        },
        parent,
      },
    };

    expect(entry).not.toMatch(/toJSONSchema|ZodObject|from ["']zod/);
    runInNewContext(entry, context);

    const host = new PluginHost({
      initial: {
        config: { size: 24, mode: 'line', romanization: false },
        lyrics: null,
        playback: { playing: false, visible: true },
        clock: { position: null, rate: 1 },
        preview: false,
      },
    });

    try {
      host.connect({
        postMessage(data, _origin, ports) {
          listeners.get('message')?.({ data, source: parent, ports } as unknown as MessageEvent);
        },
      });

      await vi.waitFor(() => {
        expect(JSON.parse(root.textContent)).toEqual(JSON.parse(JSON.stringify(config)));
      });
    } finally {
      await host.dispose();
    }
  }, 60_000);
});

describe('hysilens pack', () => {
  it('builds first and zips dist with the manifest at the archive root', async () => {
    const directory = join(fixtures, 'basic');
    await rm(join(directory, 'dist'), { recursive: true, force: true });

    const { manifest, output } = await packPlugin(directory, { logLevel: 'silent' });

    try {
      expect(output).toBe(join(directory, 'basic-1.2.3.zip'));

      const files = unzipSync(await readFile(output));

      expect(Object.keys(files).sort()).toEqual(
        ['hysilens.json', 'index.js', ...manifest.styles].sort(),
      );
      expect(JSON.parse(strFromU8(files['hysilens.json']))).toEqual(manifest);
    } finally {
      await rm(output, { force: true });
    }
  }, 60_000);
});

describe('manifest rules', () => {
  it('accepts only contained relative paths', () => {
    expect(isSafePath('assets/a.css')).toBe(true);

    for (const path of ['', '../a.js', '/a.js', 'C:/a.js', 'a\\b.js', 'a//b.js', './a.js']) {
      expect(isSafePath(path)).toBe(false);
    }
  });

  it('reports mismatched ids and missing files', async () => {
    const dist = join(fixtures, 'basic/dist');
    await buildPlugin(join(fixtures, 'basic'), { logLevel: 'silent' });

    const { errors } = await validatePluginDirectory(dist, 'other');

    expect(errors).toEqual(['id basic 与目录名 other 不一致']);
    expect((await validatePluginDirectory(fixtures)).errors[0]).toContain('无法读取');
  }, 60_000);

  it('mirrors the native plugin directory resolution', () => {
    expect(userPluginsDirectory({ HYSILENS_PLUGINS_DIR: '/x/plugins' })).toBe(
      resolve('/x/plugins'),
    );
    expect(userPluginsDirectory({ HYSILENS_DATA_DIR: '/data' })).toBe(
      join(resolve('/data'), 'plugins'),
    );
    expect(userPluginsDirectory({ LOCALAPPDATA: 'C:/Local' })).toBe(
      join('C:/Local', 'Hysilens', 'plugins'),
    );
  });
});

describe('bundled plugins', () => {
  it('cannot be linked, packed or installed as external plugins', () => {
    const manifest = { id: 'builtin:scroll' } as Parameters<typeof assertExternal>[0];
    expect(() => assertExternal(manifest, '打包安装')).toThrow('builtin:');
    expect(() => assertExternal({ ...manifest, id: 'scroll' }, '打包安装')).not.toThrow();
    expect(localName('builtin:scroll')).toBe('scroll');
    expect(isBuiltinId('scroll')).toBe(false);
  });
});
