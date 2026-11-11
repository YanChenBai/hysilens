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
      // Each bundled plugin builds itself; this copies the results into public/plugins.
      plugins: {
        dependsOn: ['@hysilens/renderer-scroll#build', '@hysilens/renderer-tilted#build'],
        command: 'vpx scripts/bundle-plugins.ts',
        cache: false,
      },
      // Rust embeds this output, so it must exist before any cargo build or test.
      'build:web': {
        dependsOn: ['plugins'],
        command: ['vp exec tsc --noEmit', 'vp build'],
        cache: false,
      },
      'dev:web': { dependsOn: ['plugins'], command: 'vp dev', cache: false },
      dev: { command: 'vp exec tauri dev -- --dev', cache: false },
      build: { command: 'vp exec tauri build --no-bundle', cache: false },
    },
  },
});
