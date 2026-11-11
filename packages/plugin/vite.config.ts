import { defineConfig } from 'vite-plus';

export default defineConfig({
  pack: {
    entry: {
      cli: 'src/bin.ts',
      index: 'src/index.ts',
      host: 'src/host.ts',
      protocol: 'src/protocol.ts',
      bootstrap: 'src/bootstrap.ts',
      config: 'src/config.ts',
      vite: 'src/vite.ts',
      demo: 'src/demo.ts',
      samples: 'src/samples.ts',
    },
    format: 'esm',
    fixedExtension: false,
    platform: 'node',
    target: 'node22',
    dts: true,
    outDir: 'dist',
    clean: true,
    outputOptions: { chunkFileNames: '[name]-[hash].js' },
  },
});
