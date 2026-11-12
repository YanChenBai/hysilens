import { qualityConfig } from '@hysilens/vp-config';
import { defineConfig } from 'vite-plus';

export default defineConfig({
  ...qualityConfig,
  test: {
    include: [
      'apps/desktop/src/**/*.test.ts',
      'packages/plugin/src/**/*.test.ts',
      'packages/devtools/src/**/*.test.ts',
      'plugins/*/src/**/*.test.ts',
    ],
  },
  staged: { '*': 'vp check --fix' },
  run: {
    cache: true,
    /* 仅放工作区级检查；app、plugin 和 devtools 的任务在各自 package 内 */
    tasks: {
      'check:types': { command: 'tsc --noEmit', cache: false },
      'check:native': {
        dependsOn: ['@hysilens/desktop-ui#build:web'],
        command: [
          'cargo fmt --all --check',
          'cargo clippy --workspace --all-targets -- -D warnings',
        ],
        cache: false,
      },
      'test:native': {
        dependsOn: ['@hysilens/desktop-ui#build:web'],
        command: 'cargo test --workspace --all-targets',
        cache: false,
      },
    },
  },
});
