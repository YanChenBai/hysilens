/**
 * 把内置歌词 plugin 发布到 `public/plugins`，每个 plugin 自行构建（其 `build` 任务，
 * 本包的 `plugins` 任务依赖它）；这里只做校验与复制
 */
import { cp, mkdir, rename, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { validatePluginDirectory } from '../../../packages/plugin/src/manifest';
import { BUILTIN_PREFIX } from '../../../packages/plugin/src/protocol';
import type { PluginManifest } from '../../../packages/plugin/src/protocol';

/**
 * `plugins/` 下的目录；每个目录声明 id `builtin:<folder>`
 */
const BUNDLED = ['scroll', 'tilted'];

const root = resolve(import.meta.dirname, '../../..');
const publicRoot = resolve(import.meta.dirname, '../public/plugins');
const manifests: PluginManifest[] = [];

/**
 * 通过临时文件写入 JSON，让读取方不会看到写了一半的索引
 */
async function publishJson(file: string, value: unknown) {
  const temporary = `${file}.${process.pid}.tmp`;

  await writeFile(temporary, JSON.stringify(value, null, 2));
  await rename(temporary, file);
}

/* 从头重建内置集合，这样被移除或取消内置的 plugin 不会残留 */
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
