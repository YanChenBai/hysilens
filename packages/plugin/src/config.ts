/**
 * 构建期的 plugin 描述
 * 配置在 Node 中求值；Vite 集成将运行时的默认导入替换为纯数据，
 * 避免将 zod 打进 iframe 包
 */
import { z } from 'zod';

import { PLUGIN_ID_PATTERN } from './protocol';
import type { PluginFeatures, PluginViewport } from './protocol';
import type { ConfigCarrier } from './runtime';

export { z };

type Field = z.ZodType;
type Shape = Record<string, Field>;

const COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/;

/**
 * 构建期的 plugin 声明；在变成 manifest 之前由 `defineConfig` 校验
 */
export interface ConfigInput<S extends Shape, F extends PluginFeatures> {
  /**
   * plugin id，会按 `PLUGIN_ID_PATTERN` 校验
   */
  id: string;

  /**
   * plugin 面板中显示的显示名，保存前会去掉首尾空白
   */
  name: string;

  description?: string;

  /**
   * 期望的 iframe 尺寸；宽高都必须是正整数
   */
  viewport?: PluginViewport;

  /**
   * 相对 plugin 根目录的 runtime 入口，默认 `src/index.ts`
   */
  entry?: string;

  /**
   * 要启用的能力；每一项还会注入对应的 config 字段
   */
  features?: F;

  /**
   * plugin 自定义字段；每个字段都必须有默认值
   */
  config: S;
}

type Enabled<F, Key extends keyof PluginFeatures> = F extends { [K in Key]: true | object }
  ? true
  : false;

type Simplify<T> = { [K in keyof T]: T[K] } & {};

/**
 * plugin 可见的 feature 字段；`scale` 由 host 应用并保持隐藏
 */
export type FeatureConfig<F> = (Enabled<F, 'font'> extends true ? { fontFamily: string } : {}) &
  (Enabled<F, 'reveal'> extends true ? { revealMode: 'word' | 'line' } : {}) &
  (Enabled<F, 'romanization'> extends true ? { romanization: boolean } : {}) &
  (Enabled<F, 'translation'> extends true ? { translation: boolean } : {});

/**
 * plugin 收到的带类型 config：解析后的字段值，加上所有已启用 feature 的字段
 */
export type ConfigOutput<S extends Shape, F> = Simplify<
  z.output<z.ZodObject<S>> & FeatureConfig<F>
>;

/**
 * `defineConfig` 产出的只读描述；CLI 读取它，plugin 可导入其数据或引用其类型
 */
export interface ConfigDefinition<Output> extends ConfigCarrier<Output> {
  readonly kind: 'hysilens:config';

  readonly id: string;

  readonly name: string;

  readonly description?: string;

  readonly viewport?: PluginViewport;

  /**
   * 按声明得到的入口，已套用默认值
   */
  readonly entry: string;

  /**
   * 声明 features 的副本，之后重新赋值输入不会改变任何东西
   */
  readonly features: PluginFeatures;

  /**
   * 存储配置的 Draft 2020-12 JSON Schema，包含注入的字段
   */
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

/**
 * 渲染为滑块的数值范围；`step` 决定滑块步长，`int` 拒绝小数值
 */
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

/**
 * 渲染为取色器的不透明颜色，取值为 `#rrggbb`
 */
export function color(options: Labelled & { default: string }) {
  return z
    .string()
    .regex(COLOR_PATTERN)
    .default(options.default)
    .meta(meta(options, { 'x-hysilens-control': 'color' }));
}

/**
 * 渲染为开关的布尔值
 */
export function toggle(options: Labelled & { default: boolean }) {
  return z.boolean().default(options.default).meta(meta(options));
}

/**
 * 单行文本；`maxLength` 默认为 200 个字符
 */
export function text(options: Labelled & { default: string; maxLength?: number }) {
  return z
    .string()
    .max(options.maxLength ?? 200)
    .default(options.default)
    .meta(meta(options));
}

/**
 * 在若干字符串值中选择；`options` 把每个值映射到它的标签
 */
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

/**
 * 字体族名称；`families` 仅作为候选提供，任何名称都仍然有效
 */
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

/**
 * 拒绝 JSON Schema 会静默丢弃的构造，使 host 的校验与 zod 保持一致
 */
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

/**
 * 校验声明并把它转成 manifest 源
 * 遇到非法 id、空 name、非整数 viewport、没有默认值的字段、被 features 覆盖的字段名，
 * 或 JSON Schema 无法表达的校验时抛错
 */
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

/**
 * 判断 `value` 是否为 `defineConfig` 的结果，而不是其他任意默认导出
 */
export function isConfigDefinition(value: unknown): value is ConfigDefinition<unknown> {
  return (
    typeof value === 'object' &&
    value !== null &&
    (value as ConfigDefinition<unknown>).kind === 'hysilens:config'
  );
}
