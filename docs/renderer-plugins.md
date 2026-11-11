# 歌词插件

插件就是一个网页，怎么渲染由插件自己决定。宿主只提供数据，包括配置、歌词、播放状态和歌词时间，并在沙箱 iframe 中加载插件。设计细节与协议见 [插件系统设计](plugin-system.md)。

## 快速开始

```bash
hysilens create my-lyrics --name 我的歌词   # 脚手架
cd my-lyrics
hysilens dev                               # 开发工具：画布、播放控制、测试歌词、配置面板，插件热更新
hysilens build                             # 产出 dist/ 与 dist/hysilens.json
hysilens link                              # 链接到用户插件目录，在 Hysilens 中实时调试
hysilens pack                              # 构建并打包为 my-lyrics-<version>.zip（--no-build 跳过构建）
```

`hysilens dev` 打开的开发工具（`@hysilens/devtools`）使用与 OBS、桌面预览相同的 `PluginHost` 加载插件：

- 画布按 `viewport` 等比显示，可切换透明（棋盘格，模拟 OBS 合成）、深色、浅色背景，并叠加 `scale` 能力的缩放。
- 播放控制：播放／暂停、可拖动进度条、上一句／下一句、0.25×–2× 倍速、循环，以及模拟 `playback.visible = false`。
- 内置测试歌词（`@hysilens/plugin/samples`）：中文逐字带拼音与翻译、英文逐词、日文带罗马音，以及边界情况（超长句、空行、10 秒间奏、只有整行时间、极快逐字、emoji 与组合字符、时间重叠、无结束时间）。歌词列表可点击跳转。
- 配置面板按 `configSchema` 自动生成表单，修改即时推送给插件，也可直接编辑 JSON 或恢复默认值。
- 日志记录宿主状态与插件 `report()` 上报的错误；插件代码修改后 iframe 整页刷新会自动重新连接，`hysilens.config.ts` 的修改也会同步到配置面板。

仓库内的 `plugins/<名称>/` 是随应用发布的内置插件：各自的 `build` 任务构建 `dist/`，`@hysilens/desktop-ui` 的 `plugins` 任务再把它们复制到 `apps/desktop/public/plugins/<名称>/`。其他插件都是外部插件，在各自目录运行 `vp run build`，再用 `hysilens link`（开发调试）安装到用户插件目录，或用 `hysilens pack` 一步构建并打包后在应用中安装。

## hysilens.config.ts

元信息与配置 Schema 写在插件根目录的 `hysilens.config.ts`，这个文件只在构建时执行：

```ts
import {
  color,
  defineConfig,
  font,
  number,
  select,
  text,
  toggle,
  z,
} from '@hysilens/plugin/config';

export default defineConfig({
  id: 'my-lyrics', // 必须与安装目录名一致；builtin: 前缀保留给内置插件
  name: '我的歌词',
  description: '可选',
  viewport: { width: 1280, height: 360 }, // 建议的 OBS 画布尺寸，不强制
  entry: 'src/index.ts', // 默认值
  features: { scale: true, font: { families: ['Example Display'] }, romanization: true },
  config: {
    fontSize: number({ title: '字号', min: 16, max: 120, step: 2, default: 48 }),
    color: color({ title: '歌词颜色', default: '#ffffff' }),
    weight: select({ title: '字重', options: { '400': '常规', '700': '粗体' }, default: '700' }),
    shadow: toggle({ title: '阴影', default: true }),
    label: text({ title: '落款', default: '题', maxLength: 4 }),
    mode: z.enum(['a', 'b']).default('a').meta({ title: '模式' }), // 裸 zod 逃生口
  },
});
```

- 每个字段都必须有默认值，并且能表示为 JSON Schema。`transform`、`preprocess`、`refine`、`trim`、`catch` 在构建时直接报错，因为 JSON Schema 会把它们悄悄丢掉。
- 构建产物中的 `configSchema` 是宿主唯一的校验依据：
  - 桌面端用它生成配置编辑器、填充默认值；
  - 宿主保存配置时用它校验。
  - 插件运行时不含 zod。
- 运行时代码只能用 `import type` 引用 `hysilens.config.ts`。如果用了值导入，构建会失败。

### features

能力默认全部关闭。声明后，宿主会注入对应字段，并在编辑器中显示对应控件：

| feature                               | 注入字段                   | 插件需要做什么                    |
| ------------------------------------- | -------------------------- | --------------------------------- |
| `scale: true`                         | `scale`（0.25～3，默认 1） | 无。宿主以画布中心缩放整个 iframe |
| `font: true \| { families, default }` | `fontFamily`               | 使用 `config().fontFamily`        |
| `reveal: true`                        | `revealMode`（逐词／逐句） | 按该值实现逐词或逐句揭示          |
| `romanization: true`                  | `romanization`（默认开启） | 开启时渲染注音／罗马音轨道        |
| `translation: true`                   | `translation`（默认开启）  | 开启时渲染翻译轨道                |

`scale` 不会出现在插件的 `config()` 类型里。字体选择器会列出系统字体，以及 `families` 中声明的插件字体。

## 运行时

```ts
import { definePlugin } from '@hysilens/plugin';

import type config from '../hysilens.config';

import './style.css';

export default definePlugin<typeof config>({
  setup({
    root,
    config,
    lyrics,
    playback,
    time,
    onMounted,
    onUnmounted,
    onConfigChange,
    onLyricsChange,
    onPlaybackChange,
  }) {
    // 在这里同步注册钩子、创建状态；DOM 挂到 root 上
  },
});
```

| 成员                   | 说明                                                                   |
| ---------------------- | ---------------------------------------------------------------------- |
| `root`                 | SDK 创建的全尺寸容器（`[data-hysilens-root]`）                         |
| `config()`             | 已校验、已填默认值的配置，类型由 `typeof config` 推导                  |
| `lyrics()`             | 当前歌词文档或 `null`                                                  |
| `playback()`           | `{ playing, visible }`；`visible` 综合了连接状态和播放器状态           |
| `time()`               | 歌词时间（ms，已应用 offset），`null` 表示未知；随时调用都是最新推算值 |
| `preview`              | 是否在预览／演示模式中                                                 |
| `assetUrl(path)`       | 解析插件内资源的绝对 URL                                               |
| `signal`               | dispose 时触发 abort，可传给 `addEventListener`                        |
| `report(error)`        | 上报非致命错误，宿主会显示，但插件继续运行                             |
| `onMounted(fn)`        | 可以是 async；完成后宿主才视为就绪，超过 10 秒或抛错则显示错误         |
| `onUnmounted(fn)`      | 可以是 async（例如退场动画），宿主最多等待 1 秒                        |
| `onConfigChange(fn)`   | `(next, prev)`，保存配置后触发                                         |
| `onLyricsChange(fn)`   | 切歌或换歌词文档时触发                                                 |
| `onPlaybackChange(fn)` | 播放／暂停、显示／隐藏变化时触发                                       |

- 宿主只在播放状态变化、跳转或时钟漂移超过 30 ms 时发送时钟锚点，SDK 在插件内推算时间，不会每帧收到消息。
- React、Vue、Canvas、WebGL 或原生 DOM 均可使用：在 `onMounted` 中挂载，在 `onUnmounted` 中卸载。构建时 `process.env.NODE_ENV` 固定为 `production`，框架会使用生产版本。需要框架插件（如 `@vitejs/plugin-react`）时写在插件自己的 `vite.config.ts` 里，不需要也不要再添加 `hysilens()`。

## 资源与沙箱

- CSS 和图片、字体等资源直接在入口中 `import`，或放进 `public/` 原样发布；构建会自动收集样式。`new URL('./asset.png', import.meta.url)` 与 `fetch(ctx.assetUrl('data.json'))` 都可以使用。
- 插件页运行在 `sandbox="allow-scripts"` 的 iframe 中，是 opaque origin：
  - 不能访问 `parent`；
  - 不能使用 `localStorage` 和 cookie；
  - 不能调用宿主的写接口。
  - 宿主为 `/plugins/*` 返回 `Access-Control-Allow-Origin: *`，所以模块、样式和字体都能正常加载。
- 不再支持按顺序注入的经典脚本，也不再支持远程 `styles`。第三方库请通过 ES 模块导入。

## 安装与配置文件

- 外部插件放在 `%LOCALAPPDATA%/Hysilens/plugins/<id>/`，目录内容就是 `dist/`（必须包含 `hysilens.json`）。界面中的「插件目录」按钮会创建并打开这个目录。
- 插件面板的「安装插件」按钮接受 `hysilens pack` 生成的 zip：服务端在隐藏的临时目录中解压，按与加载相同的规则校验后，原子替换 `plugins/<id>/`，失败时保留旧版本。压缩包不能超过 64 MB、解压后不能超过 256 MB，不能包含越界路径或符号链接；目标若是 `hysilens link` 建立的开发链接则拒绝覆盖。
- 服务端监听插件目录（包括 `hysilens link` 链接到的目录）和配置目录，有变化时通过事件 WebSocket 推送 `plugins` / `pluginConfig` 通知，各窗口随即重新读取，不再轮询；插件面板的「重新扫描」按钮作为兜底。与内置插件同名的外部插件优先。缺少 `hysilens.json` 或校验失败的目录不会加载，失败原因显示在插件面板中。
- `HYSILENS_PLUGINS_DIR` 覆盖外部插件目录，`HYSILENS_DATA_DIR` 统一覆盖用户数据根目录。
- 插件配置保存在 `%LOCALAPPDATA%/Hysilens/plugin-configs/<id>.json`（内置插件为 `plugin-configs/builtin/<名称>.json`），`HYSILENS_PLUGIN_CONFIGS_DIR` 可覆盖这个目录。加载时会丢弃未知字段，非法值回退到默认值；保存时严格校验。

## 内置插件

随应用发布的插件只有下面两个（`scripts/build-plugins.ts` 的 `BUNDLED` 名单），其余插件都是外部插件，通过 `hysilens link` 或 zip 安装。

| 插件             | 名称     | 建议宽 × 高 |
| ---------------- | -------- | ----------- |
| `builtin:scroll` | 滚动歌词 | 960 × 540   |
| `builtin:tilted` | 透视描边 | 1280 × 720  |

- `builtin:scroll`（`plugins/scroll`）：竖向滚动，支持逐字或整行高亮，可配置歌词颜色、高亮颜色、字号、显示行数、行距和字体。
- `builtin:tilted`（`plugins/tilted`）：透视描边效果，提供 X／Y／Z 轴旋转、透视距离、文字厚度、字重、描边、阴影和字体配置，按 DOM 实际高度排版。
