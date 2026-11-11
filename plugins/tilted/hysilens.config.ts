import { color, defineConfig, font, number } from '@hysilens/plugin/config';

export default defineConfig({
  id: 'builtin:tilted',
  name: '透视描边',
  description: '整列歌词像立方体的一面向左侧转，当前行放大 1.4 倍，可调字体、显示行数与配色。',
  viewport: { width: 1280, height: 720 },
  config: {
    fontFamily: font({ title: '字体', families: ['ZCOOLXiaoWei'], default: 'ZCOOLXiaoWei' }),
    visibleLines: number({ title: '显示行数', min: 1, max: 15, int: true, default: 5 }),
    color: color({ title: '歌词颜色', default: '#ffffff' }),
    activeColor: color({ title: '当前行颜色', default: '#ffffff' }),
    strokeColor: color({ title: '描边颜色', default: '#5b5356' }),
    depthColor: color({ title: '立体颜色', default: '#494146' }),
    shadowColor: color({ title: '阴影颜色', default: '#211a21' }),
  },
});
