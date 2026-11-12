import { PLUGIN_ID_PATTERN } from '@hysilens/plugin/protocol';
import { z } from 'zod';

import defaults from '../../../config/overlay-defaults.json' with { type: 'json' };

const color = z
  .string()
  .regex(/^#[\da-f]{6}(?:[\da-f]{2})?$/i)
  .transform(value => value.toLowerCase());

const number = (minimum: number, maximum: number, fallback: number) =>
  z.preprocess(
    value => (typeof value === 'string' && !value.trim() ? undefined : value),
    z.coerce.number().finite().min(minimum).max(maximum).default(fallback),
  );

const boolean = (fallback: boolean) =>
  z.preprocess(value => {
    if (value === 'true' || value === '1') {
      return true;
    }

    if (value === 'false' || value === '0') {
      return false;
    }

    return value;
  }, z.boolean().default(fallback));

const ignoredLyricLines = z.record(z.string(), z.array(z.string()));

/**
 * 持久化 overlay 设置的校验后形状；字段默认值来自
 * `config/overlay-defaults.json`，除非该字段自己指定了默认值
 */
export const overlaySchema = z.object({
  color: color.default(defaults.color),
  activeColor: color.default(defaults.activeColor),
  background: z.union([z.literal('transparent'), color]).default(defaults.background),
  fontFamily: z
    .string()
    .trim()
    .regex(/^[\p{L}\p{N} _-]+$/u)
    .max(80)
    .default(defaults.fontFamily),
  fontSize: number(14, 96, defaults.fontSize),
  lineGap: number(0, 48, defaults.lineGap),
  visibleLines: z.union([z.literal(2), z.literal(3)]).default(3),
  offset: number(-10000, 10000, defaults.offset),
  romanization: boolean(defaults.romanization),
  translation: boolean(defaults.translation),
  ignoredLyricLines: ignoredLyricLines.default({}),
  renderer: z
    .string()
    .regex(PLUGIN_ID_PATTERN)
    .default(defaults.renderer)
    /* 这些名字下已不存在的 renderer 的 id */
    .transform(value => (['marquee', 'builtin'].includes(value) ? defaults.renderer : value)),
});

/**
 * 解析后的 overlay 设置
 * 布尔值与数值会从 settings 文件可能使用的字符串形式转换而来，
 * 缺失的字段一律回退到默认值
 *
 * 尺寸单位为 CSS 像素：`fontSize` 保持在 `14..96`，`lineGap` 保持在 `0..48`
 * `offset` 是歌词时间线的偏移，单位 ms，限制在 `-10000..10000`
 * `renderer` 是 plugin id，历史 id（`marquee`、`builtin`）会被改写成默认 renderer
 * `ignoredLyricLines` 把歌曲 id 映射到 overlay 中隐藏行的 id，
 * 每个 id 编码为 `[startMs, normalizedText]`
 */
export type OverlayOptions = z.output<typeof overlaySchema>;

/**
 * 来自 `config/overlay-defaults.json` 的默认值，已经过 schema 校验
 */
export const overlayDefaults = overlaySchema.parse(defaults);

/**
 * 逐字段解析持久化设置，使单个非法值不会丢弃其余字段
 * `issues` 列出回退到默认值的字段
 */
export function parseOverlayConfig(input: unknown) {
  const incoming =
    typeof input === 'object' && input !== null ? (input as Record<string, unknown>) : {};

  const values: Record<string, unknown> = {};
  const issues: string[] = [];

  for (const [name, schema] of Object.entries(overlaySchema.shape)) {
    const parsed = schema.safeParse(incoming[name]);

    if (!parsed.success) {
      issues.push(name);
    }

    values[name] = parsed.success ? parsed.data : schema.parse(undefined);
  }

  return { options: overlaySchema.parse(values), issues };
}
