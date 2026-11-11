/**
 * Publish the bundled lyric plugins into `public/plugins`. Each plugin builds itself (its own
 * `build` task, which this package's `plugins` task depends on); this only validates and copies.
 */
import { cp, mkdir, rename, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { validatePluginDirectory } from '../../../packages/plugin/src/manifest';
import { BUILTIN_PREFIX } from '../../../packages/plugin/src/protocol';
import type { PluginManifest } from '../../../packages/plugin/src/protocol';

/** Folders under `plugins/`; each declares the id `builtin:<folder>`. */
const BUNDLED = ['scroll', 'tilted'];

const root = resolve(import.meta.dirname, '../../..');
const publicRoot = resolve(import.meta.dirname, '../public/plugins');
const manifests: PluginManifest[] = [];

async function publishJson(file: string, value: unknown) {
  const temporary = `${file}.${process.pid}.tmp`;
  await writeFile(temporary, JSON.stringify(value, null, 2));
  await rename(temporary, file);
}

// Rebuild the bundled set from scratch so removed or unbundled plugins never linger.
await rm(publicRoot, { recursive: true, force: true });
await mkdir(publicRoot, { recursive: true });

for (const folder of BUNDLED) {
  const dist = resolve(root, 'plugins', folder, 'dist');
  const { manifest, errors } = await validatePluginDirectory(dist, `${BUILTIN_PREFIX}${folder}`);

  if (!manifest || errors.length) {
    throw new Error(
      `Bundled plugin ${folder} is not built correctly:\n  - ${errors.join('\n  - ')}`,
    );
  }

  await cp(dist, resolve(publicRoot, folder), { recursive: true });
  manifests.push(manifest);
}

await publishJson(resolve(publicRoot, 'index.json'), manifests);
