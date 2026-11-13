import { defineConfig, number, z } from '@hysilens/plugin/config';

export default defineConfig({
  id: 'leaky',
  name: '值导入配置',
  features: { romanization: true },
  config: {
    size: number({ title: '字号', min: 12, max: 96, default: 24 }),
    mode: z.enum(['line', 'word']).default('line'),
  },
});
