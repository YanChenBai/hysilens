import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite-plus';

export default defineConfig({
  plugins: [react({ compiler: true })],
  build: {
    lib: { entry: 'src/index.ts', formats: ['es'], fileName: 'index' },
    rolldownOptions: {
      external: [
        /^react(?:\/|$)/,
        /^@base-ui\//,
        /^@tanstack\//,
        /^motion(?:\/|$)/,
        /^lucide-react$/,
        /^kubai-icons-react(?:\/|$)/,
        /^cn$/,
      ],
    },
  },
});
