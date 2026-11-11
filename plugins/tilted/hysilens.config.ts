import { color, defineConfig, font, number, toggle } from '@hysilens/plugin/config';

import { preset } from './src/preset';

export default defineConfig({
  id: 'builtin:tilted',
  name: '透视描边',
  description:
    '整列歌词像立方体的一面向左侧转，当前行居中并放大 1.4 倍，可调字体、字号、显示行数与配色。',
  viewport: { width: 1280, height: 720 },
  config: {
    fontFamily: font({ title: '字体', families: ['ZCOOLXiaoWei'], default: 'ZCOOLXiaoWei' }),
    fontSize: number({ title: '字号', min: 18, max: 96, default: preset.fontSize }),
    fontWeight: number({ title: '字重', min: 100, max: 900, step: 100, default: 400 }),
    visibleLines: number({ title: '显示行数', min: 1, max: 15, int: true, default: 5 }),
    lineGap: number({ title: '行间距', min: 0, max: 80, default: preset.lineGap }),
    inactiveOpacity: number({
      title: '非当前句透明度',
      min: 0,
      max: 1,
      step: 0.01,
      default: preset.inactiveOpacity,
    }),
    color: color({ title: '歌词颜色', default: '#ffffff' }),
    activeColor: color({ title: '当前行颜色', default: '#ffffff' }),
    strokeColor: color({ title: '描边颜色', default: '#5b5356' }),
    strokeWidth: number({
      title: '描边宽度',
      min: 0,
      max: 6,
      step: 0.1,
      default: preset.strokeWidth,
    }),
    depthColor: color({ title: '立体颜色', default: '#494146' }),
    depth: number({ title: '立体厚度', min: 0, max: 12, step: 0.5, default: preset.depth }),
    shadow: toggle({ title: '文字阴影', default: preset.shadow }),
    shadowColor: color({ title: '阴影颜色', default: '#211a21' }),
    shadowOpacity: number({
      title: '阴影透明度',
      min: 0,
      max: 1,
      step: 0.01,
      default: preset.shadowOpacity,
    }),
    shadowBlur: number({ title: '阴影模糊', min: 0, max: 40, default: preset.shadowBlur }),
    shadowX: number({ title: '阴影水平偏移', min: -30, max: 30, default: preset.shadowX }),
    shadowY: number({ title: '阴影垂直偏移', min: -30, max: 30, default: preset.shadowY }),
  },
});
