import type { AnySchemaObject } from 'ajv';
import Ajv from 'ajv/dist/2020';

const ajv = new Ajv({ useDefaults: true, allErrors: true, strict: false });
const validators = new Map<string, ReturnType<typeof ajv.compile>>();

/**
 * 按 plugin 的 config schema 校验主题并补全默认值
 *
 * 编译后的校验器按 schema 形状缓存，校验在 structured clone 上执行
 * 因此调用方传入的对象不会被修改
 *
 * @param schema plugin 的 config schema，已收窄到主题相关属性
 * @param input 原始主题值，通常由 JSON 解析得到
 * @returns 归一化后的主题对象
 * @throws `input` 不是对象或不符合 schema 时抛出
 */
export function parseTheme(schema: AnySchemaObject, input: unknown): Record<string, unknown> {
  const key = JSON.stringify(schema);
  let validate = validators.get(key);

  if (!validate) {
    validate = ajv.compile(schema);
    validators.set(key, validate);
  }

  const theme: unknown = structuredClone(input);

  if (!validate(theme) || typeof theme !== 'object' || theme === null || Array.isArray(theme)) {
    throw new Error(ajv.errorsText(validate.errors));
  }

  return theme as Record<string, unknown>;
}
