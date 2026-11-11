import { execFile } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import { describe, expect, it } from 'vite-plus/test';

const run = promisify(execFile);
const cli = fileURLToPath(new URL('../dist/cli.js', import.meta.url));

describe('built CLI', () => {
  it('runs directly in Node without a TypeScript loader', async () => {
    const { stderr } = await run(process.execPath, [cli, '--help']);
    expect(stderr).toContain('hysilens <命令>');
    expect(await readFile(cli, 'utf8')).toMatch(/^#!\/usr\/bin\/env node/);
  });

  it('reads the package version when creating an external plugin', async () => {
    const directory = await mkdtemp(join(tmpdir(), 'hysilens-cli-'));
    const root = join(directory, 'example');

    try {
      await run(process.execPath, [cli, 'create', root]);
      const manifest = JSON.parse(await readFile(join(root, 'package.json'), 'utf8'));
      const sdk = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'));
      expect(manifest.devDependencies['@hysilens/plugin']).toBe(`^${sdk.version}`);
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
});
