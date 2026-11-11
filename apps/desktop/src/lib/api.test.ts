import type { PluginManifest } from '@hysilens/plugin';
import { describe, expect, it } from 'vite-plus/test';

import { checkConfig, loadConfig } from './api';

const plugin: PluginManifest = {
  apiVersion: 1,
  id: 'example',
  name: 'Example',
  version: '1.0.0',
  entry: 'index.js',
  styles: [],
  features: { scale: true },
  revision: 'r1',
  configSchema: {
    $schema: 'https://json-schema.org/draft/2020-12/schema',
    type: 'object',
    additionalProperties: false,
    properties: {
      size: { type: 'number', minimum: 10, maximum: 90, default: 40, 'x-hysilens-step': 2 },
      accent: { type: 'string', pattern: '^#[0-9a-fA-F]{6}$', default: '#ffffff' },
      scale: { type: 'number', minimum: 0.25, maximum: 3, default: 1 },
    },
  },
};

describe('plugin config', () => {
  it('fills defaults without mutating the stored value', () => {
    const stored = { size: 50 };

    expect(loadConfig(plugin, stored)).toEqual({ size: 50, accent: '#ffffff', scale: 1 });
    expect(stored).toEqual({ size: 50 });
    expect(loadConfig(plugin, null)).toEqual({ size: 40, accent: '#ffffff', scale: 1 });
  });

  it('drops unknown keys and resets invalid values when loading', () => {
    expect(loadConfig(plugin, { size: 500, accent: 'red', direction: 'up', scale: 1.15 })).toEqual({
      size: 40,
      accent: '#ffffff',
      scale: 1.15,
    });
  });

  it('reports every problem when saving', () => {
    expect(() => checkConfig(plugin, { size: 500, direction: 'up' })).toThrow(
      /插件配置.*(maximum|must be <=|additional)/,
    );
    expect(checkConfig(plugin, { size: 12 })).toEqual({ size: 12, accent: '#ffffff', scale: 1 });
  });
});
