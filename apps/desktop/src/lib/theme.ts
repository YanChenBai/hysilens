import type { AnySchemaObject } from 'ajv';
import Ajv from 'ajv/dist/2020';

const ajv = new Ajv({ useDefaults: true, allErrors: true, strict: false });
const validators = new Map<string, ReturnType<typeof ajv.compile>>();

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
