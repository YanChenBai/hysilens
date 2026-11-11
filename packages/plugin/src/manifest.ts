/** Node-side manifest helpers shared by the Vite plugin and the CLI. */
import { createHash } from 'node:crypto';
import { readdir, readFile, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { isAbsolute, join, relative, resolve, sep } from 'node:path';

import type { ConfigDefinition } from './config';
import { API_VERSION, BUILTIN_PREFIX, isBuiltinId, PLUGIN_ID_PATTERN } from './protocol';
import type { PluginManifest } from './protocol';

export const MANIFEST_FILE = 'hysilens.json';

export const CONFIG_FILE = 'hysilens.config.ts';

export function createManifest(
  definition: ConfigDefinition<unknown>,
  build: { version: string; entry: string; styles: string[]; revision?: string },
): PluginManifest {
  return {
    apiVersion: API_VERSION,
    id: definition.id,
    name: definition.name,
    version: build.version,
    ...(definition.description ? { description: definition.description } : {}),
    entry: build.entry,
    styles: build.styles,
    ...(definition.viewport ? { viewport: definition.viewport } : {}),
    features: definition.features,
    configSchema: definition.configSchema,
    revision: build.revision ?? '',
  };
}

async function listFiles(directory: string, base = directory): Promise<string[]> {
  const files: string[] = [];

  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);

    if (entry.isDirectory()) {
      files.push(...(await listFiles(path, base)));
    } else if (entry.isFile()) {
      files.push(relative(base, path).split(sep).join('/'));
    }
  }

  return files.sort();
}

/** Content fingerprint of every published file plus the manifest itself. */
export async function computeRevision(
  directory: string,
  manifest: PluginManifest,
): Promise<string> {
  const hash = createHash('sha256');
  hash.update(JSON.stringify({ ...manifest, revision: '' }));

  for (const file of await listFiles(directory)) {
    if (file === MANIFEST_FILE) {
      continue;
    }

    hash.update(`\0${file}\0`);
    hash.update(await readFile(join(directory, file)));
  }

  return hash.digest('hex');
}

/** Relative, forward-slash path that stays inside its root. */
export function isSafePath(path: unknown): path is string {
  return (
    typeof path === 'string' &&
    path.length > 0 &&
    !path.includes('\\') &&
    !path.includes(':') &&
    !isAbsolute(path) &&
    path.split('/').every(part => part && part !== '.' && part !== '..')
  );
}

async function isFile(path: string): Promise<boolean> {
  return stat(path).then(
    info => info.isFile(),
    () => false,
  );
}

function shapeErrors(manifest: Partial<PluginManifest>): string[] {
  const errors: string[] = [];

  if (manifest.apiVersion !== API_VERSION) {
    errors.push(`apiVersion 必须为 ${API_VERSION}`);
  }

  if (!PLUGIN_ID_PATTERN.test(String(manifest.id))) {
    errors.push('id 无效');
  }

  if (typeof manifest.name !== 'string' || !manifest.name.trim()) {
    errors.push('name 缺失');
  }

  if (typeof manifest.version !== 'string' || !manifest.version) {
    errors.push('version 缺失');
  }

  if (typeof manifest.configSchema !== 'object' || manifest.configSchema === null) {
    errors.push('configSchema 必须是对象');
  }

  if (!Array.isArray(manifest.styles)) {
    errors.push('styles 必须是数组');
  }

  return errors;
}

/** Apply the host's acceptance rules to an installable plugin directory. */
export async function validatePluginDirectory(
  directory: string,
  expectedId?: string,
): Promise<{ manifest?: PluginManifest; errors: string[] }> {
  let manifest: PluginManifest;

  try {
    manifest = JSON.parse(await readFile(join(directory, MANIFEST_FILE), 'utf8')) as PluginManifest;
  } catch (error) {
    return { errors: [`无法读取 ${MANIFEST_FILE}：${(error as Error).message}`] };
  }

  const errors = shapeErrors(manifest);

  if (expectedId !== undefined && manifest.id !== expectedId) {
    errors.push(`id ${String(manifest.id)} 与目录名 ${expectedId} 不一致`);
  }

  for (const [label, path] of [
    ['entry', manifest.entry],
    ...(manifest.styles ?? []).map(style => ['styles', style] as const),
  ] as const) {
    if (!isSafePath(path)) {
      errors.push(`${label} 路径无效：${String(path)}`);
    } else if (!(await isFile(join(directory, path)))) {
      errors.push(`${label} 文件不存在：${path}`);
    }
  }

  if (typeof manifest.entry === 'string' && !manifest.entry.endsWith('.js')) {
    errors.push('entry 必须是 .js 文件');
  }

  return { manifest, errors };
}

/** Bundled plugins ship inside the app; only external ones can be linked, packed or installed. */
export function assertExternal(manifest: PluginManifest, action: string): void {
  if (isBuiltinId(manifest.id)) {
    throw new Error(
      `${manifest.id} 是内置插件（${BUILTIN_PREFIX} 前缀），随应用发布，不能${action}`,
    );
  }
}

/** Mirror of `hysilens_core::config::data_root` and `plugins::root`. */
export function userPluginsDirectory(env: NodeJS.ProcessEnv = process.env): string {
  if (env.HYSILENS_PLUGINS_DIR) {
    return resolve(env.HYSILENS_PLUGINS_DIR);
  }

  const data = env.HYSILENS_DATA_DIR
    ? resolve(env.HYSILENS_DATA_DIR)
    : join(env.LOCALAPPDATA ?? tmpdir(), 'Hysilens');

  return join(data, 'plugins');
}

export async function readPackageVersion(root: string): Promise<string> {
  try {
    const source = JSON.parse(await readFile(join(root, 'package.json'), 'utf8')) as {
      version?: unknown;
    };

    return typeof source.version === 'string' && source.version ? source.version : '0.0.0';
  } catch {
    return '0.0.0';
  }
}
