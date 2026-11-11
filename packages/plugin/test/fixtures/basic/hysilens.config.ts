import { color, defineConfig, number } from '@hysilens/plugin/config';

export default defineConfig({
  id: 'basic',
  name: '构建测试',
  viewport: { width: 640, height: 360 },
  features: { font: true, scale: true },
  config: {
    size: number({ title: '字号', min: 10, max: 90, default: 40 }),
    accent: color({ title: '颜色', default: '#ffffff' }),
  },
});
