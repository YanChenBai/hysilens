/** Serve the devtools UI bundled with `@hysilens/plugin`. */
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { Connect } from 'vite-plus';

export const DEVTOOLS_BASE = '/__hysilens/devtools/';

const CONTENT_TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.woff2': 'font/woff2',
  '.json': 'application/json',
};

/** Resolve the prebuilt UI inside this package; no workspace sources are needed at runtime. */
export function ensureDevtools(): string {
  const dist = fileURLToPath(new URL('../dist/devtools/', import.meta.url));

  if (!existsSync(join(dist, 'index.html'))) {
    throw new Error(
      '缺少 @hysilens/plugin 内置 devtools，请在仓库中运行 vp run @hysilens/plugin#build 后重试',
    );
  }

  return dist;
}

/** Serve `/` and `/__hysilens/devtools/*` from the built devtools. */
export function devtoolsMiddleware(dist: string): Connect.NextHandleFunction {
  return (request, response, next) => {
    const path = decodeURIComponent(request.url?.split('?')[0] ?? '');
    let file: string;

    if (path === '/' || path === '/index.html') {
      file = join(dist, 'index.html');
    } else if (path.startsWith(DEVTOOLS_BASE)) {
      file = normalize(join(dist, path.slice(DEVTOOLS_BASE.length)));
    } else {
      next();

      return;
    }

    const inside = relative(dist, file);

    if (inside.startsWith('..') || inside.split(sep).includes('..')) {
      response.statusCode = 403;
      response.end();

      return;
    }

    readFile(file).then(
      body => {
        response.setHeader(
          'Content-Type',
          CONTENT_TYPES[extname(file)] ?? 'application/octet-stream',
        );
        response.setHeader('Cache-Control', 'no-store');
        response.end(body);
      },
      () => next(),
    );
  };
}
