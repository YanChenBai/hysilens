import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite-plus';

export default defineConfig({
  plugins: [react({ compiler: true }), tailwindcss()],
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': { target: 'http://127.0.0.1:38473', ws: true },
      '/plugins': { target: 'http://127.0.0.1:38473' },
    },
  },
  run: {
    tasks: {
      /* 每个内置 plugin 自己构建；这里只负责把结果复制到 public/plugins */
      plugins: {
        dependsOn: ['@hysilens/renderer-scroll#build', '@hysilens/renderer-tilted#build'],
        command: 'vpx scripts/bundle-plugins.ts',
        cache: false,
      },
      /* Rust 会嵌入这份产物，所以任何 cargo build 或 test 之前都必须先生成 */
      'build:web': {
        dependsOn: ['plugins'],
        command: ['vp exec tsc --noEmit', 'vp build'],
        cache: false,
      },
      'dev:web': { dependsOn: ['plugins'], command: 'vp dev', cache: false },
      dev: { command: 'vp exec tauri dev -- -- --dev', cache: false },
      build: { command: 'vp exec tauri build --no-bundle', cache: false },
    },
  },
});
