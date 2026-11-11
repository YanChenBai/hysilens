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
    // Ids of renderers that no longer exist under those names.
    .transform(value => (['marquee', 'builtin'].includes(value) ? defaults.renderer : value)),
});

export type OverlayOptions = z.output<typeof overlaySchema>;

export const overlayDefaults = overlaySchema.parse(defaults);

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
