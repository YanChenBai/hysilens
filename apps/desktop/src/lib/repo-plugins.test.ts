import type { PluginManifest } from '@hysilens/plugin';
import type { ConfigDefinition } from '@hysilens/plugin/config';
import { describe, expect, it } from 'vite-plus/test';

import scroll from '../../../../plugins/scroll/hysilens.config';
import tilted from '../../../../plugins/tilted/hysilens.config';
import { checkConfig, loadConfig } from './api';

/**
 * host 只能看到 manifest，因此通过同一路径测试内置 schema
 */
function manifest(definition: ConfigDefinition<unknown>): PluginManifest {
  return {
    apiVersion: 1,
    id: definition.id,
    name: definition.name,
    version: '0.0.0',
    entry: 'index.js',
    styles: [],
    viewport: definition.viewport,
    features: definition.features,
    configSchema: definition.configSchema,
    revision: definition.id,
  };
}

const all = [scroll, tilted].map(manifest);

function rejects(plugin: PluginManifest, value: Record<string, unknown>) {
  expect(() => checkConfig(plugin, value), JSON.stringify(value)).toThrow();
}

describe('repository plugin configs', () => {
  it('use the reserved builtin namespace', () => {
    expect(all.map(plugin => plugin.id)).toEqual(['builtin:scroll', 'builtin:tilted']);
  });

  it('declare unique ids and viewports, with host scaling only for scroll', () => {
    expect(new Set(all.map(plugin => plugin.id)).size).toBe(all.length);

    for (const plugin of all) {
      expect(plugin.viewport?.width).toBeGreaterThan(0);
      expect(plugin.viewport?.height).toBeGreaterThan(0);
    }

    const plugin = manifest(scroll);

    expect(plugin.features.scale).toBe(true);
    expect(loadConfig(plugin, {}).scale).toBe(1);
    expect(checkConfig(plugin, { scale: 1.15 }).scale).toBe(1.15);
    rejects(plugin, { scale: 0 });
    rejects(plugin, { scale: 4 });
  });

  it('validate line counts and colors', () => {
    for (const plugin of all) {
      rejects(plugin, { color: 'red' });
    }

    const plugin = manifest(scroll);

    rejects(plugin, { visibleLines: 2.5 });
    rejects(plugin, { visibleLines: 0 });
    expect(checkConfig(plugin, { visibleLines: 4 }).visibleLines).toBe(4);
  });

  it('exposes font, colors and line count for tilted while dropping old geometry and scale', () => {
    const plugin = manifest(tilted);

    expect(plugin.features.font).toBeUndefined();

    const saved = loadConfig(plugin, {
      rotateX: -4,
      rotateY: -48,
      activeScale: 1.2,
      scale: 2,
      fontSize: 80,
      color: '#abcdef',
    });

    for (const key of ['rotateX', 'rotateY', 'activeScale', 'scale']) {
      expect(saved).not.toHaveProperty(key);
    }

    expect(saved.color).toBe('#abcdef');
    expect(saved.visibleLines).toBe(5);
    expect(saved.fontFamily).toBe('ZCOOLXiaoWei');
    expect(saved.fontSize).toBe(80);
    expect(loadConfig(plugin, {}).fontSize).toBe(42);
    rejects(plugin, { fontSize: 0 });
    rejects(plugin, { fontSize: 97 });
    expect(checkConfig(plugin, { fontFamily: 'Arial' }).fontFamily).toBe('Arial');
    expect(checkConfig(plugin, { visibleLines: 3 }).visibleLines).toBe(3);
    rejects(plugin, { visibleLines: 0 });
    rejects(plugin, { visibleLines: 2.5 });
  });

  it('accepts configurable tilted text appearance and validates effect bounds', () => {
    const plugin = manifest(tilted);

    const appearance = {
      fontSize: 56,
      fontWeight: 600,
      lineGap: 20,
      inactiveOpacity: 0.4,
      strokeWidth: 2.5,
      depth: 3,
      shadow: false,
      shadowOpacity: 0.6,
      shadowBlur: 8,
      shadowX: -2,
      shadowY: 5,
    };

    expect(checkConfig(plugin, appearance)).toMatchObject(appearance);

    for (const value of [
      { strokeWidth: -1 },
      { depth: 13 },
      { shadowOpacity: 2 },
      { shadowBlur: -1 },
      { shadowX: 31 },
      { shadow: 'false' },
    ]) {
      rejects(plugin, value);
    }
  });

  it('offer reveal mode only where the renderer implements it', () => {
    for (const plugin of all) {
      expect(plugin.features.reveal).toBeUndefined();
      expect(loadConfig(plugin, {})).not.toHaveProperty('revealMode');
    }
  });
});
