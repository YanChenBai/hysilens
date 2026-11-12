import { readdir } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it, vi } from 'vite-plus/test';

import { DEVTOOLS_BASE, devtoolsMiddleware, ensureDevtools } from './devtools';

interface Reply {
  status: number;

  headers: Record<string, string>;

  body: string;

  forwarded: boolean;
}

/**
 * 用最小的 request/response 替身驱动 connect handler，不打开任何 port
 */
async function request(handler: ReturnType<typeof devtoolsMiddleware>, url: string) {
  return new Promise<Reply>(resolve => {
    const reply: Reply = { status: 200, headers: {}, body: '', forwarded: false };

    const response = {
      set statusCode(value: number) {
        reply.status = value;
      },
      setHeader: (name: string, value: string) => {
        reply.headers[name.toLowerCase()] = value;
      },
      end: (body?: Buffer | string) => {
        reply.body = body ? body.toString() : '';
        resolve(reply);
      },
    };

    const next = vi.fn(() => {
      reply.forwarded = true;
      resolve(reply);
    });

    handler({ url } as never, response as never, next);
  });
}

describe('devtools', () => {
  it('serves the bundled UI with its assets', async () => {
    const dist = ensureDevtools();

    expect(dist).toBe(fileURLToPath(new URL('../dist/devtools/', import.meta.url)));

    const handler = devtoolsMiddleware(dist);

    const index = await request(handler, '/?from=test');
    expect(index.headers['content-type']).toContain('text/html');
    expect(index.body).toContain(`${DEVTOOLS_BASE}assets/`);

    const [script] = (await readdir(join(dist, 'assets'))).filter(name => name.endsWith('.js'));
    const asset = await request(handler, `${DEVTOOLS_BASE}assets/${script}`);

    expect(asset.headers['content-type']).toContain('javascript');
    expect(asset.body.length).toBeGreaterThan(1000);
  }, 120_000);

  it('refuses paths outside the build and forwards everything else', async () => {
    const handler = devtoolsMiddleware(ensureDevtools());

    for (const url of [
      `${DEVTOOLS_BASE}../../package.json`,
      `${DEVTOOLS_BASE}%2e%2e/%2e%2e/package.json`,
    ]) {
      expect((await request(handler, url)).status).toBe(403);
    }

    expect((await request(handler, '/__hysilens/plugin')).forwarded).toBe(true);
    expect((await request(handler, `${DEVTOOLS_BASE}missing.js`)).forwarded).toBe(true);
  }, 120_000);
});
