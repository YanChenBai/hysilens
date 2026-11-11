/** Vite integration: wraps the plugin entry, emits `hysilens.json`, and serves a dev host. */
import { writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

import { normalizePath, runnerImport } from 'vite-plus';
import type { Plugin as VitePlugin, ViteDevServer } from 'vite-plus';

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

/** The plugin page and its description; `/` itself is the prebuilt devtools UI. */
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

/** Added by the `hysilens` CLI; do not also list it in a plugin's own vite.config. */
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

      // The entry is a JS module rather than an HTML app, so libraries such as React would
      // otherwise keep their development branches in the published bundle.
      const nodeEnv = env.command === 'build' ? env.mode : 'development';

      return {
        base: './',
        define: { 'process.env.NODE_ENV': JSON.stringify(nodeEnv) },
        server: { cors: true },
        build: {
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

      // Type-only imports are erased before bundling, so reaching here means a value import.
      if (normalizePath(id.split('?')[0] ?? '') === configPath) {
        this.error(`运行时代码只能用 import type 引用 ${CONFIG_FILE}，否则 zod 会被打进插件包`);
      }

      return undefined;
    },
    async configureServer(server) {
      // Registered before Vite's own middlewares, so `/` is the devtools, not an index.html lookup.
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
