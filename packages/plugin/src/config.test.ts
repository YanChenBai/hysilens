import { describe, expect, expectTypeOf, it } from 'vite-plus/test';

import { color, defineConfig, font, number, select, text, toggle, z } from './config';
import { definePlugin } from './runtime';
import type { ConfigOf } from './runtime';

const base = { id: 'demo', name: '示例' };

describe('defineConfig', () => {
  it('emits a JSON Schema with helper metadata and no required fields', () => {
    const config = defineConfig({
      ...base,
      viewport: { width: 1280, height: 720 },
      config: {
        size: number({ title: '字号', min: 10, max: 100, step: 2, default: 40 }),
        accent: color({ title: '颜色', default: '#ffffff' }),
        shadow: toggle({ title: '阴影', default: true }),
        weight: select({
          title: '字重',
          options: { '400': '常规', '700': '粗体' },
          default: '700',
        }),
        label: text({ title: '落款', default: '题' }),
      },
    });

    expect(config.entry).toBe('src/index.ts');
    expect(config.features).toEqual({});

    expect(config.configSchema).toMatchObject({
      type: 'object',
      additionalProperties: false,
      properties: {
        size: {
          type: 'number',
          minimum: 10,
          maximum: 100,
          default: 40,
          title: '字号',
          'x-hysilens-control': 'slider',
          'x-hysilens-step': 2,
        },
        accent: { type: 'string', default: '#ffffff', 'x-hysilens-control': 'color' },
        shadow: { type: 'boolean', default: true, title: '阴影' },
        weight: {
          enum: ['400', '700'],
          default: '700',
          'x-hysilens-enum-labels': { '400': '常规', '700': '粗体' },
        },
        label: { type: 'string', maxLength: 200, default: '题' },
      },
    });

    expect(config.configSchema).not.toHaveProperty('required');
    expect(config.configSchema.properties).not.toHaveProperty('size.multipleOf');
    expect(config.configSchema.properties).not.toHaveProperty('shadow.description');
  });

  it('injects only declared feature fields', () => {
    const config = defineConfig({
      ...base,
      features: {
        scale: true,
        font: { families: ['古韵楷体'], default: '古韵楷体' },
        reveal: true,
        romanization: true,
      },
      config: {},
    });

    const properties = config.configSchema.properties as Record<string, Record<string, unknown>>;

    expect(Object.keys(properties).sort()).toEqual([
      'fontFamily',
      'revealMode',
      'romanization',
      'scale',
    ]);
    expect(properties.scale).toMatchObject({ minimum: 0.25, maximum: 3, default: 1 });
    expect(properties.fontFamily).toMatchObject({
      default: '古韵楷体',
      'x-hysilens-control': 'font',
      'x-hysilens-font-families': ['古韵楷体'],
    });
    expect(properties.revealMode).toMatchObject({ enum: ['word', 'line'], default: 'word' });
  });

  it('accepts representable raw zod as an escape hatch', () => {
    const config = defineConfig({
      ...base,
      config: { mode: z.enum(['a', 'b']).default('a').meta({ title: '模式' }) },
    });

    expect(config.configSchema.properties).toMatchObject({ mode: { enum: ['a', 'b'] } });
  });

  it.each([
    [
      'transform',
      z
        .string()
        .default('a')
        .transform(value => value.length),
      'transform',
    ],
    ['preprocess', z.preprocess(value => value, z.string().default('a')), 'preprocess'],
    [
      'refine',
      z
        .string()
        .default('a')
        .refine(value => value !== 'b'),
      'refine',
    ],
    ['trim', z.string().trim().default('a'), 'trim'],
    ['catch', z.number().catch(1), 'catch'],
    ['nested', z.array(z.string().refine(Boolean)).default([]), 'refine'],
  ])('rejects %s', (_name, field, message) => {
    expect(() => defineConfig({ ...base, config: { field } })).toThrow(message);
  });

  it('rejects missing defaults, injected name clashes and invalid metadata', () => {
    expect(() => defineConfig({ ...base, config: { size: z.number() } })).toThrow('默认值');

    expect(() =>
      defineConfig({
        ...base,
        features: { font: true },
        config: { fontFamily: font({ title: '字体', default: 'x' }) },
      }),
    ).toThrow('features');

    expect(() => defineConfig({ ...base, id: '../x', config: {} })).toThrow('id');
    expect(() => defineConfig({ ...base, name: ' ', config: {} })).toThrow('name');
    expect(() => defineConfig({ ...base, viewport: { width: 0, height: 10 }, config: {} })).toThrow(
      'viewport',
    );
  });
});

describe('types', () => {
  it('derives plugin config from fields and features, hiding scale', () => {
    const config = defineConfig({
      ...base,
      features: { scale: true, font: true, reveal: true, translation: true },
      config: {
        size: number({ title: '字号', min: 1, max: 9, default: 2 }),
        weight: select({ title: '字重', options: { light: '细', bold: '粗' }, default: 'bold' }),
        shadow: toggle({ title: '阴影', default: false }),
      },
    });

    expectTypeOf<ConfigOf<typeof config>>().toEqualTypeOf<{
      size: number;

      weight: 'light' | 'bold';

      shadow: boolean;

      fontFamily: string;

      revealMode: 'word' | 'line';

      translation: boolean;
    }>();

    definePlugin<typeof config>({
      setup({ config: current }) {
        expectTypeOf(current().weight).toEqualTypeOf<'light' | 'bold'>();
        // @ts-expect-error scale 由 host 应用，对 plugin 隐藏
        void current().scale;
      },
    });
  });
});

describe('plugin ids', () => {
  it('reserve the builtin namespace and reject other prefixes', () => {
    expect(defineConfig({ id: 'builtin:scroll', name: '内置', config: {} }).id).toBe(
      'builtin:scroll',
    );

    for (const id of ['other:x', 'builtin:', 'builtin:a/b', 'Builtin:x']) {
      expect(() => defineConfig({ id, name: '示例', config: {} })).toThrow('id');
    }
  });
});
