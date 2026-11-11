import type { PluginManifest } from '@hysilens/plugin';
import type { ConfigDefinition } from '@hysilens/plugin/config';
import { describe, expect, it } from 'vite-plus/test';

import scroll from '../../../../plugins/scroll/hysilens.config';
import tilted from '../../../../plugins/tilted/hysilens.config';
import { checkConfig, loadConfig } from './api';

/** The host sees only the manifest, so test the bundled schemas through that same path. */
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

  it('declare unique ids, viewports and host-side scaling', () => {
    expect(new Set(all.map(plugin => plugin.id)).size).toBe(all.length);

    for (const plugin of all) {
      expect(plugin.features.scale).toBe(true);
      expect(plugin.viewport?.width).toBeGreaterThan(0);
      expect(plugin.viewport?.height).toBeGreaterThan(0);
      expect(loadConfig(plugin, {}).scale).toBe(1);
      expect(checkConfig(plugin, { scale: 1.15 }).scale).toBe(1.15);
      rejects(plugin, { scale: 0 });
      rejects(plugin, { scale: 4 });
    }
  });

  it('validate line counts and colors', () => {
    for (const plugin of all) {
      rejects(plugin, { visibleLines: 2.5 });
      rejects(plugin, { visibleLines: 0 });
      rejects(plugin, { color: 'red' });
      expect(checkConfig(plugin, { visibleLines: 4 }).visibleLines).toBe(4);
    }
  });

  it('offer reveal mode only where the renderer implements it', () => {
    for (const plugin of all) {
      expect(plugin.features.reveal).toBeUndefined();
      expect(loadConfig(plugin, {})).not.toHaveProperty('revealMode');
    }
  });
});
