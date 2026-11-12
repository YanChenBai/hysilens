/**
 * `hysilens` 命令行：构建、开发、校验、链接与打包 renderer plugin
 */
import { existsSync } from 'node:fs';
import {
  lstat,
  mkdir,
  readdir,
  readFile,
  readlink,
  rm,
  symlink,
  writeFile,
} from 'node:fs/promises';
import { basename, dirname, join, relative, resolve, sep } from 'node:path';
import { parseArgs } from 'node:util';

import { zipSync } from 'fflate';
import { build, createServer } from 'vite-plus';

import {
  assertExternal,
  CONFIG_FILE,
  MANIFEST_FILE,
  readPackageVersion,
  userPluginsDirectory,
  validatePluginDirectory,
} from './manifest';
import type { PluginManifest } from './protocol';
import { hysilens } from './vite';

const HELP = `用法：hysilens <命令> [目录]

  build [dir]      构建插件到 dir/dist，并生成 ${MANIFEST_FILE}
  dev [dir]        启动带 mock 宿主的开发服务器（--port 指定端口）
  validate [dir]   按宿主规则校验构建产物（dir 或 dir/dist）
  link [dir]       将 dist 链接到用户插件目录，在真实应用中调试
  pack [dir]       构建并打包为 <id>-<version>.zip（--no-build 直接打包现有 dist）
  create <dir>     创建新插件（--name 指定显示名称）`;

function pluginRoot(dir: string | undefined): string {
  const root = resolve(dir ?? '.');

  if (!existsSync(join(root, CONFIG_FILE))) {
    throw new Error(`${root} 中没有 ${CONFIG_FILE}`);
  }

  return root;
}

/**
 * 构建单个 plugin 目录并返回校验通过的 manifest
 */
export async function buildPlugin(
  dir: string,
  options: { logLevel?: 'info' | 'warn' | 'error' | 'silent' } = {},
): Promise<PluginManifest> {
  const root = pluginRoot(dir);

  await build({ root, logLevel: options.logLevel, plugins: [hysilens()] });

  return assertValid(join(root, 'dist'));
}

async function assertValid(directory: string, expectedId?: string): Promise<PluginManifest> {
  const { manifest, errors } = await validatePluginDirectory(directory, expectedId);

  if (!manifest || errors.length) {
    throw new Error(`插件产物无效（${directory}）：\n  - ${errors.join('\n  - ')}`);
  }

  return manifest;
}

async function dev(dir: string | undefined, port: string | undefined) {
  const server = await createServer({
    root: pluginRoot(dir),
    plugins: [hysilens()],
    server: { port: port ? Number(port) : undefined },
  });

  await server.listen();
  server.printUrls();
}

async function validate(dir: string | undefined) {
  const root = resolve(dir ?? '.');
  const directory = existsSync(join(root, MANIFEST_FILE)) ? root : join(root, 'dist');

  const manifest = await assertValid(directory);

  console.warn(`✓ ${manifest.id}@${manifest.version} 有效`);
}

async function link(dir: string | undefined) {
  const root = pluginRoot(dir);
  const dist = join(root, 'dist');
  const manifest = await assertValid(dist);

  assertExternal(manifest, '链接到用户插件目录');

  const target = join(userPluginsDirectory(), manifest.id);
  const existing = await lstat(target).catch(() => undefined);

  if (existing && !existing.isSymbolicLink()) {
    throw new Error(`${target} 已存在且不是链接，请手动处理后重试`);
  }

  if (existing) {
    await rm(target);
  }

  await mkdir(dirname(target), { recursive: true });
  /* 在 Windows 上创建 junction 不需要提权；其他平台会忽略该 type */
  await symlink(dist, target, 'junction');
  console.warn(`✓ ${target} → ${await readlink(target)}`);
}

async function collect(directory: string, base = directory): Promise<Record<string, Uint8Array>> {
  const files: Record<string, Uint8Array> = {};

  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);

    if (entry.isDirectory()) {
      Object.assign(files, await collect(path, base));
    } else if (entry.isFile()) {
      files[relative(base, path).split(sep).join('/')] = await readFile(path);
    }
  }

  return files;
}

/**
 * 构建（除非 `build: false`）并把 `dist/` 打成 `<id>-<version>.zip`
 * 压缩包根目录就是 plugin 目录本身，这也是应用安装器所期望的结构
 */
export async function packPlugin(
  dir: string,
  options: { build?: boolean; logLevel?: 'info' | 'warn' | 'error' | 'silent' } = {},
): Promise<{ manifest: PluginManifest; output: string }> {
  const root = pluginRoot(dir);

  const manifest =
    options.build === false
      ? await assertValid(join(root, 'dist'))
      : await buildPlugin(root, { logLevel: options.logLevel });

  assertExternal(manifest, '打包安装');

  const output = join(root, `${manifest.id}-${manifest.version}.zip`);
  const files = await collect(join(root, 'dist'));

  await writeFile(output, zipSync(files, { level: 9 }));

  return { manifest, output };
}

async function pack(dir: string | undefined, build: boolean) {
  const { output } = await packPlugin(dir ?? '.', { build });

  console.warn(`✓ ${relative(process.cwd(), output)}，可在 Hysilens 插件面板中「安装插件」`);
}

function insideWorkspace(root: string): boolean {
  for (let current = dirname(root); current !== dirname(current); current = dirname(current)) {
    if (existsSync(join(current, 'packages', 'plugin', 'src', 'cli.ts'))) {
      return true;
    }
  }

  return false;
}

async function create(dir: string | undefined, name: string | undefined) {
  if (!dir) {
    throw new Error('请指定插件目录：hysilens create <dir>');
  }

  const root = resolve(dir);
  const id = basename(root);

  if (existsSync(root) && (await readdir(root)).length) {
    throw new Error(`${root} 已存在且不为空`);
  }

  const version = insideWorkspace(root)
    ? 'workspace:*'
    : `^${await readPackageVersion(resolve(import.meta.dirname, '..'))}`;

  const files: Record<string, string> = {
    'package.json': `${JSON.stringify(
      {
        name: id,
        version: '0.1.0',
        private: true,
        type: 'module',
        scripts: { dev: 'hysilens dev', build: 'hysilens build', pack: 'hysilens pack' },
        devDependencies: { '@hysilens/plugin': version },
      },
      null,
      2,
    )}\n`,
    [CONFIG_FILE]: `import { color, defineConfig, number } from '@hysilens/plugin/config';

export default defineConfig({
  id: ${JSON.stringify(id)},
  name: ${JSON.stringify(name ?? id)},
  viewport: { width: 1280, height: 360 },
  features: { font: true, romanization: true, translation: true },
  config: {
    fontSize: number({ title: '字号', min: 16, max: 120, default: 48 }),
    color: color({ title: '歌词颜色', default: '#ffffff' }),
  },
});
`,
    'src/index.ts': `import { definePlugin } from '@hysilens/plugin';

import type config from '../${CONFIG_FILE.replace(/\.ts$/, '')}';
import './style.css';

export default definePlugin<typeof config>({
  setup({ root, config, lyrics, time, onMounted, onUnmounted }) {
    const line = document.createElement('p');
    line.className = 'line';
    let frame = 0;

    const render = () => {
      const now = time();
      const lines = lyrics()?.tracks.find(track => track.kind === 'original')?.lines ?? [];
      const current = now === null ? undefined : lines.findLast(item => item.startMs <= now);
      const { color, fontSize, fontFamily } = config();
      Object.assign(line.style, { color, fontSize: \`\${fontSize}px\`, fontFamily });
      line.textContent = current?.text ?? '';
      frame = requestAnimationFrame(render);
    };

    onMounted(() => {
      root.append(line);
      render();
    });
    onUnmounted(() => cancelAnimationFrame(frame));
  },
});
`,
    'src/style.css': `.line {
  position: absolute;
  inset: auto 0 20%;
  margin: 0;
  text-align: center;
  font-weight: 600;
}
`,
    'tsconfig.json': `${JSON.stringify(
      {
        compilerOptions: {
          target: 'ES2023',
          module: 'ESNext',
          moduleResolution: 'Bundler',
          lib: ['ES2023', 'DOM'],
          strict: true,
          noEmit: true,
          skipLibCheck: true,
          verbatimModuleSyntax: true,
          types: ['vite/client'],
        },
        include: ['src', CONFIG_FILE],
      },
      null,
      2,
    )}\n`,
  };

  for (const [file, content] of Object.entries(files)) {
    await mkdir(dirname(join(root, file)), { recursive: true });
    await writeFile(join(root, file), content);
  }

  console.warn(`✓ 已创建 ${relative(process.cwd(), root) || '.'}，运行 hysilens dev 开始开发`);
}

/**
 * 从 `argv`（不含 node 与脚本路径）执行一条命令，返回进程退出码
 * 用法错误与 `--help` 输出到 stderr
 */
export async function main(argv: string[]): Promise<number> {
  const { positionals, values } = parseArgs({
    args: argv,
    allowPositionals: true,
    options: {
      port: { type: 'string' },
      name: { type: 'string' },
      'no-build': { type: 'boolean' },
      help: { type: 'boolean', short: 'h' },
    },
  });

  const [command, dir] = positionals;

  try {
    switch (command) {
      case 'build': {
        const manifest = await buildPlugin(dir ?? '.');
        console.warn(`✓ ${manifest.id}@${manifest.version} → dist/${MANIFEST_FILE}`);
        break;
      }

      case 'dev': {
        await dev(dir, values.port);
        break;
      }

      case 'validate': {
        await validate(dir);
        break;
      }

      case 'link': {
        await link(dir);
        break;
      }

      case 'pack': {
        await pack(dir, !values['no-build']);
        break;
      }

      case 'create': {
        await create(dir, values.name);
        break;
      }

      default: {
        console.warn(HELP);

        return values.help || !command ? 0 : 1;
      }
    }

    return 0;
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);

    return 1;
  }
}
