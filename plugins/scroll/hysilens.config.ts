import { color, defineConfig, font, number, toggle } from '@hysilens/plugin/config';

export default defineConfig({
  id: 'builtin:scroll',
  name: '滚动歌词',
  description: '逐字或整行高亮的竖向滚动歌词。',
  viewport: { width: 960, height: 540 },
  entry: 'src/main.tsx',
  features: {
    scale: true,
    romanization: true,
    translation: true,
  },
  config: {
    fontFamily: font({ title: '字体', families: ['Noto Sans SC'], default: 'Noto Sans SC' }),
    fontSize: number({ title: '字号', min: 14, max: 120, default: 44 }),
    visibleLines: number({ title: '显示行数', min: 1, max: 9, int: true, default: 3 }),
    lineGap: number({ title: '行间距', min: 0, max: 64, default: 16 }),

    wordHighlight: toggle({ title: '逐字高亮', default: true }),
    color: color({ title: '歌词颜色', default: '#edf3ff' }),
    activeColor: color({ title: '高亮颜色', default: '#bcb8ff' }),
  },
});
