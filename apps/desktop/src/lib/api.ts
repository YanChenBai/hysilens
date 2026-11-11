import type { PluginManifest } from '@hysilens/plugin';
import type { AnySchemaObject, ValidateFunction } from 'ajv';
import Ajv2020 from 'ajv/dist/2020';

export async function getJson<T>(path: string): Promise<T> {
  const response = await fetch(path, { cache: 'no-store' });

  if (!response.ok) {
    throw new Error(await response.text());
  }

  return response.json() as Promise<T>;
}

export async function putJson(path: string, value: unknown): Promise<void> {
  const response = await fetch(path, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(value),
  });

  if (!response.ok) {
    throw new Error(await response.text());
  }
}

export interface PluginCatalog {
  plugins: PluginManifest[];
  errors: string[];
  revision?: number;
}

/** Upload a `hysilens pack` archive; the server validates, installs and announces it. */
export async function installPlugin(file: Blob): Promise<PluginManifest> {
  const response = await fetch('/api/plugins/install', {
    method: 'POST',
    headers: { 'Content-Type': 'application/zip' },
    body: file,
  });

  if (!response.ok) {
    throw new Error((await response.text()) || `安装失败（${response.status}）`);
  }

  return response.json() as Promise<PluginManifest>;
}

/** Force the server to re-read plugin directories (fallback when file events are missed). */
export async function rescanPlugins(): Promise<PluginCatalog> {
  const response = await fetch('/api/plugins/rescan', { method: 'POST' });

  if (!response.ok) {
    throw new Error(await response.text());
  }

  return response.json() as Promise<PluginCatalog>;
}

export interface PluginConfig {
  schema: AnySchemaObject;
  config: Record<string, unknown>;
}

export type ConfigValues = Record<string, unknown>;

// `x-hysilens-*` annotations are UI metadata, so unknown keywords must not fail compilation.
// `allErrors` lets one pass find every invalid field instead of stopping at the first.
const lenient = new Ajv2020({
  strict: false,
  useDefaults: true,
  removeAdditional: true,
  allErrors: true,
});

const exact = new Ajv2020({ strict: false, useDefaults: true, allErrors: true });
const compiled = new Map<string, ValidateFunction>();

function validator(ajv: Ajv2020, mode: string, plugin: PluginManifest): ValidateFunction {
  const key = `${mode}:${plugin.id}:${plugin.revision}`;
  let validate = compiled.get(key);

  if (!validate) {
    validate = ajv.compile(plugin.configSchema as AnySchemaObject);
    compiled.set(key, validate);
  }

  return validate;
}

function asObject(value: unknown): ConfigValues {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? structuredClone(value as ConfigValues)
    : {};
}

/**
 * Configuration handed to a running plugin: unknown keys are dropped and invalid values fall
 * back to their schema defaults, so a stale file never blanks the overlay.
 */
export function loadConfig(plugin: PluginManifest, value: unknown): ConfigValues {
  const validate = validator(lenient, 'load', plugin);
  const config = asObject(value);

  if (validate(config)) {
    return config;
  }

  for (const error of validate.errors ?? []) {
    // Top-level JSON Pointer segment, unescaped per RFC 6901.
    const field = error.instancePath.split('/')[1]?.replaceAll('~1', '/').replaceAll('~0', '~');

    if (field) {
      delete config[field];
    }
  }

  if (!validate(config)) {
    throw new Error(lenient.errorsText(validate.errors, { dataVar: '插件配置' }));
  }

  return config;
}

/** Configuration about to be saved: reported exactly as entered, defaults filled in. */
export function checkConfig(plugin: PluginManifest, value: unknown): ConfigValues {
  const validate = validator(exact, 'save', plugin);
  const config = asObject(value);

  if (!validate(config)) {
    throw new Error(exact.errorsText(validate.errors, { dataVar: '插件配置' }));
  }

  return config;
}
