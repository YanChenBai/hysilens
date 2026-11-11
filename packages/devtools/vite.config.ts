import { fileURLToPath } from 'node:url';

import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite-plus';

/**
 * Built into `@hysilens/plugin`, which serves the files under `/__hysilens/devtools/`.
 * To work on the devtools themselves, start a plugin with `hysilens dev`, then run `vp dev` here
 * with `VITE_HYSILENS_TARGET=http://127.0.0.1:<plugin port>`.
 */
export default defineConfig({
  base: '/__hysilens/devtools/',
  plugins: [react({ compiler: true }), tailwindcss()],
  build: {
    outDir: fileURLToPath(new URL('../plugin/dist/devtools/', import.meta.url)),
    emptyOutDir: true,
  },
});
