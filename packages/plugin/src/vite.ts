/**
 * Vite 集成：包装 plugin 入口、生成 `hysilens.json`，并提供开发用 host
 */
import { writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

import { normalizePath, runnerImport } from 'vite';
import type { Plugin as VitePlugin, ViteDevServer } from 'vite';

import { isConfigDefinition } from './config';
import type { ConfigDefinition } from './config';
import { devtoolsMiddleware, ensureDevtools } from './devtools';
import {
  CONFIG_FILE,
  MANIFEST_FILE,
  computeRevision,
  createManifest,
  readPackageVersion,
} from './manifest';

const ENTRY_ID = 'virtual:hysilens/entry';

const RESOLVED_ENTRY_ID = `\0${ENTRY_ID}`;

const ENTRY_FILE = 'index.js';

const DEV_PLUGIN_PATH = '/__hysilens/plugin';

const DEV_CONFIG_PATH = '/__hysilens/config.json';

/**
 * 在 `root` 中求值 `hysilens.config.ts` 并返回其默认导出
 * 当该文件导出的不是 `defineConfig` 结果时抛错
 */
export async function loadPluginConfig(root: string): Promise<ConfigDefinition<unknown>> {
  const file = resolve(root, CONFIG_FILE);
  const { module } = await runnerImport<{ default: unknown }>(file);

  if (!isConfigDefinition(module.default)) {
    throw new Error(`${CONFIG_FILE} 必须 export default defineConfig(...)`);
  }

  return module.default;
}

const DEV_PLUGIN_HTML = `<!doctype html>
<html>
  <head><meta charset="utf-8" /><style>html,body{margin:0;width:100%;height:100%;background:transparent}</style></head>
  <body><script type="module" src="/@id/${ENTRY_ID}"></script></body>
</html>`;

/**
 * plugin 页面及其描述；`/` 本身是预构建的 devtools UI
 */
function serveDevPages(server: ViteDevServer, definition: () => ConfigDefinition<unknown>) {
  server.middlewares.use(async (request, response, next) => {
    const path = request.url?.split('?')[0];

    if (path === DEV_PLUGIN_PATH) {
      response.setHeader('Content-Type', 'text/html; charset=utf-8');
      response.end(await server.transformIndexHtml(DEV_PLUGIN_PATH, DEV_PLUGIN_HTML));
    } else if (path === DEV_CONFIG_PATH) {
      response.setHeader('Content-Type', 'application/json');
      response.end(JSON.stringify(definition()));
    } else {
      next();
    }
  });
}

/**
 * 由 `hysilens` CLI 添加；不要在 plugin 自己的 vite.config 中再列一次
 */
export function hysilens(): VitePlugin {
  let root = process.cwd();
  let configPath = '';
  let outDir = 'dist';
  let definition: ConfigDefinition<unknown> | undefined;
  let styles: string[] = [];

  const current = () => {
    if (!definition) {
      throw new Error(`${CONFIG_FILE} 尚未加载`);
    }

    return definition;
  };

  return {
    name: 'hysilens',
    async config(user, env) {
      root = resolve(user.root ?? process.cwd());
      configPath = normalizePath(resolve(root, CONFIG_FILE));
      definition = await loadPluginConfig(root);

      /*
       * 入口是 JS 模块而不是 HTML 应用，否则 React 之类的库会
       * 把开发分支保留在发布产物里
       */
      const nodeEnv = env.command === 'build' ? env.mode : 'development';

      return {
        base: './',
        define: { 'process.env.NODE_ENV': JSON.stringify(nodeEnv) },
        server: { cors: true },
        build: {
          target: 'chrome100',
          cssTarget: 'chrome100',
          outDir: 'dist',
          emptyOutDir: true,
          assetsDir: 'assets',
          modulePreload: false,
          rollupOptions: {
            input: { index: ENTRY_ID },
            output: {
              entryFileNames: ENTRY_FILE,
              chunkFileNames: 'chunks/[name]-[hash].js',
              assetFileNames: 'assets/[name]-[hash][extname]',
            },
          },
        },
      };
    },
    configResolved(config) {
      outDir = resolve(config.root, config.build.outDir);
    },
    resolveId(id) {
      return id === ENTRY_ID ? RESOLVED_ENTRY_ID : undefined;
    },
    load(id) {
      if (id === RESOLVED_ENTRY_ID) {
        const entry = normalizePath(resolve(root, current().entry));

        return [
          `import plugin from ${JSON.stringify(entry)};`,
          `import { start } from '@hysilens/plugin/runtime';`,
          'start(plugin);',
        ].join('\n');
      }

      /* 在 Node 中求值配置后只输出纯数据，避免将 zod 和构建期依赖带入浏览器 */
      if (normalizePath(id.split('?')[0] ?? '') === configPath) {
        return `export default ${JSON.stringify(current())};`;
      }

      return undefined;
    },
    async configureServer(server) {
      /*
       * 注册在 Vite 自身 middleware 之前，因此 `/` 指向 devtools，
       * 而不是去找 index.html
       */
      server.middlewares.use(devtoolsMiddleware(ensureDevtools()));

      server.watcher.add(configPath);
      server.watcher.on('change', async file => {
        if (normalizePath(file) === configPath) {
          definition = await loadPluginConfig(root);
          server.ws.send({ type: 'full-reload' });
        }
      });

      serveDevPages(server, current);
    },
    generateBundle(_options, bundle) {
      const entry = Object.values(bundle).find(output => output.type === 'chunk' && output.isEntry);

      styles =
        entry?.type === 'chunk' ? [...(entry.viteMetadata?.importedCss ?? new Set<string>())] : [];
    },
    async writeBundle() {
      const manifest = createManifest(current(), {
        version: await readPackageVersion(root),
        entry: ENTRY_FILE,
        styles,
      });

      manifest.revision = await computeRevision(outDir, manifest);
      await writeFile(join(outDir, MANIFEST_FILE), `${JSON.stringify(manifest, null, 2)}\n`);
    },
  };
}
