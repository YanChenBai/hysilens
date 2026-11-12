import { fileURLToPath } from 'node:url';

import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite-plus';

/**
 * 构建产物内嵌到 `@hysilens/plugin`，由它从 `/__hysilens/devtools/` 提供
 * 调试 devtools 时先用 `hysilens dev` 启动 plugin，再在这里设置
 * `VITE_HYSILENS_TARGET=http://127.0.0.1:<plugin port>` 并运行 `vp dev`
 */
export default defineConfig({
  base: '/__hysilens/devtools/',
  plugins: [react({ compiler: true }), tailwindcss()],
  build: {
    outDir: fileURLToPath(new URL('../plugin/dist/devtools/', import.meta.url)),
    emptyOutDir: true,
  },
});
