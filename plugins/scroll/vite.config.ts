import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite-plus';

/* `hysilens build` / `hysilens dev` 会注入 plugin 构建流程；这里只放框架配置 */
export default defineConfig({
  plugins: [react({ compiler: true })],
});
