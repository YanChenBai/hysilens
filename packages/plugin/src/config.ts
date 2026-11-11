/**
 * Build-time plugin description. Evaluated only by `hysilens build`; plugin runtime code may
 * reference it with `import type` alone, so zod never reaches the iframe bundle.
 */
import { z } from 'zod';

import { PLUGIN_ID_PATTERN } from './protocol';
import type { PluginFeatures, PluginViewport } from './protocol';
import type { ConfigCarrier } from './runtime';

export { z };

type Field = z.ZodType;
type Shape = Record<string, Field>;

const COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/;

export interface ConfigInput<S extends Shape, F extends PluginFeatures> {
  id: string;
  name: string;
  description?: string;
  viewport?: PluginViewport;
  /** Runtime entry relative to the plugin root. */
  entry?: string;
  features?: F;
  config: S;
}

type Enabled<F, Key extends keyof PluginFeatures> = F extends { [K in Key]: true | object }
  ? true
  : false;

type Simplify<T> = { [K in keyof T]: T[K] } & {};

/** Feature fields visible to the plugin; `scale` is applied by the host and stays hidden. */
export type FeatureConfig<F> = (Enabled<F, 'font'> extends true ? { fontFamily: string } : {}) &
  (Enabled<F, 'reveal'> extends true ? { revealMode: 'word' | 'line' } : {}) &
  (Enabled<F, 'romanization'> extends true ? { romanization: boolean } : {}) &
  (Enabled<F, 'translation'> extends true ? { translation: boolean } : {});

export type ConfigOutput<S extends Shape, F> = Simplify<
  z.output<z.ZodObject<S>> & FeatureConfig<F>
>;

export interface ConfigDefinition<Output> extends ConfigCarrier<Output> {
  readonly kind: 'hysilens:config';
  readonly id: string;
  readonly name: string;
  readonly description?: string;
  readonly viewport?: PluginViewport;
  readonly entry: string;
  readonly features: PluginFeatures;
  /** Draft 2020-12 JSON Schema of the stored configuration, injected fields included. */
  readonly configSchema: Record<string, unknown>;
}

interface Labelled {
  title: string;
  description?: string;
}

function meta(options: Labelled, extra: Record<string, unknown> = {}) {
  const entries = Object.entries({ description: options.description, ...extra });

  return {
    title: options.title,
    ...Object.fromEntries(entries.filter(([, value]) => value !== undefined)),
  };
}

export function number(
  options: Labelled & { min: number; max: number; default: number; step?: number; int?: boolean },
) {
  const base = options.int ? z.number().int() : z.number();

  return base
    .min(options.min)
    .max(options.max)
    .default(options.default)
    .meta(meta(options, { 'x-hysilens-control': 'slider', 'x-hysilens-step': options.step }));
}

export function color(options: Labelled & { default: string }) {
  return z
    .string()
    .regex(COLOR_PATTERN)
    .default(options.default)
    .meta(meta(options, { 'x-hysilens-control': 'color' }));
}

export function toggle(options: Labelled & { default: boolean }) {
  return z.boolean().default(options.default).meta(meta(options));
}

export function text(options: Labelled & { default: string; maxLength?: number }) {
  return z
    .string()
    .max(options.maxLength ?? 200)
    .default(options.default)
    .meta(meta(options));
}

/** Choice between string values; `options` maps each value to its label. */
export function select<const Options extends Record<string, string>>(
  options: Labelled & { options: Options; default: keyof Options & string },
) {
  const values = Object.keys(options.options) as [keyof Options & string, ...string[]];

  if (!values.length) {
    throw new Error(`${options.title}：select 至少需要一个选项`);
  }

  return (z.enum(values) as unknown as z.ZodEnum<{ [K in keyof Options & string]: K }>)
    .default(options.default as never)
    .meta(meta(options, { 'x-hysilens-enum-labels': options.options }));
}

export function font(options: Labelled & { default: string; families?: string[] }) {
  return z
    .string()
    .min(1)
    .max(200)
    .default(options.default)
    .meta(
      meta(options, {
        'x-hysilens-control': 'font',
        'x-hysilens-font-families': options.families ?? [],
      }),
    );
}

function featureFields(features: PluginFeatures): Shape {
  const fields: Shape = {};

  if (features.scale) {
    fields.scale = number({ title: '整体缩放', min: 0.25, max: 3, step: 0.05, default: 1 });
  }

  if (features.font) {
    const options = typeof features.font === 'object' ? features.font : {};
    fields.fontFamily = font({
      title: '字体',
      default: options.default ?? 'Segoe UI',
      families: options.families,
    });
  }

  if (features.reveal) {
    fields.revealMode = select({
      title: '歌词呈现',
      options: { word: '逐词', line: '逐句' },
      default: 'word',
    });
  }

  if (features.romanization) {
    fields.romanization = toggle({ title: '显示注音与罗马音', default: true });
  }

  if (features.translation) {
    fields.translation = toggle({ title: '显示翻译', default: true });
  }

  return fields;
}

const REJECTED_TYPES: Record<string, string> = {
  pipe: 'transform / preprocess / pipe',
  transform: 'transform',
  catch: 'catch',
  custom: 'custom',
  lazy: 'lazy',
  promise: 'promise',
  function: 'function',
};

const REJECTED_CHECKS: Record<string, string> = {
  custom: 'refine / superRefine',
  overwrite: 'trim / toLowerCase 等改写',
};

/** Reject constructs that JSON Schema would silently drop, so host validation matches zod. */
function assertRepresentable(schema: Field, path: string): void {
  const def = (schema as unknown as { _zod: { def: Record<string, unknown> } })._zod.def;
  const type = def.type as string;

  if (type in REJECTED_TYPES) {
    throw new Error(`配置字段 ${path} 使用了无法表示为 JSON Schema 的 ${REJECTED_TYPES[type]}`);
  }

  for (const check of (def.checks as Array<{ _zod: { def: { check: string } } }>) ?? []) {
    const kind = check._zod.def.check;

    if (kind in REJECTED_CHECKS) {
      throw new Error(`配置字段 ${path} 使用了无法表示为 JSON Schema 的 ${REJECTED_CHECKS[kind]}`);
    }
  }

  for (const child of children(def)) {
    assertRepresentable(child.schema, `${path}${child.suffix}`);
  }
}

function children(def: Record<string, unknown>): Array<{ schema: Field; suffix: string }> {
  const result: Array<{ schema: Field; suffix: string }> = [];
  const single = (def.innerType ?? def.element ?? def.valueType) as Field | undefined;

  if (single) {
    result.push({ schema: single, suffix: def.element ? '[]' : '' });
  }

  for (const [key, value] of Object.entries((def.shape as Shape | undefined) ?? {})) {
    result.push({ schema: value, suffix: `.${key}` });
  }

  for (const [index, value] of ((def.options ?? def.items ?? []) as Field[]).entries()) {
    result.push({ schema: value, suffix: `[${index}]` });
  }

  return result;
}

function assertInput(input: ConfigInput<Shape, PluginFeatures>): void {
  if (!PLUGIN_ID_PATTERN.test(input.id ?? '')) {
    throw new Error(
      `插件 id 无效：${String(input.id)}（字母、数字、_、-，最长 80；内置插件可加 builtin: 前缀）`,
    );
  }

  if (!input.name?.trim()) {
    throw new Error('插件 name 不能为空');
  }

  const viewport = input.viewport;

  const positive = (value: number) => Number.isInteger(value) && value > 0;

  if (viewport && !(positive(viewport.width) && positive(viewport.height))) {
    throw new Error('viewport 宽高必须是正整数');
  }
}

export function defineConfig<const S extends Shape, const F extends PluginFeatures = {}>(
  input: ConfigInput<S, F>,
): ConfigDefinition<ConfigOutput<S, F>> {
  assertInput(input);

  const features: PluginFeatures = { ...input.features };
  const injected = featureFields(features);

  for (const [name, field] of Object.entries(input.config)) {
    if (name in injected) {
      throw new Error(`配置字段 ${name} 由 features 注入，不能重复声明`);
    }

    assertRepresentable(field, name);

    if (!field.safeParse(undefined).success) {
      throw new Error(`配置字段 ${name} 必须提供默认值`);
    }
  }

  const schema = z.strictObject({ ...input.config, ...injected });

  return {
    kind: 'hysilens:config',
    id: input.id,
    name: input.name.trim(),
    description: input.description,
    viewport: input.viewport,
    entry: input.entry ?? 'src/index.ts',
    features,
    configSchema: z.toJSONSchema(schema, { target: 'draft-2020-12', io: 'input' }) as Record<
      string,
      unknown
    >,
  };
}

export function isConfigDefinition(value: unknown): value is ConfigDefinition<unknown> {
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as ConfigDefinition<unknown>).kind === 'hysilens:config'
  );
}
