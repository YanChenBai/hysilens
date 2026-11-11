import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite-plus';

// `hysilens build` / `hysilens dev` add the plugin pipeline; only framework setup lives here.
export default defineConfig({
  plugins: [react({ compiler: true })],
});
