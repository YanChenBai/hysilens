# 插件系统 v1（重构设计）

> 状态：已完成，仓库内只保留随应用发布的 `builtin:scroll` 与 `builtin:tilted`。
> 不兼容旧插件接口；`apiVersion` 保持 `1`。插件作者指南见 [renderer-plugins.md](renderer-plugins.md)。

## 目标

- 插件就是一个网页，怎么渲染由插件自己决定，宿主只提供数据。
- 一份构建期配置文件描述元信息和配置 Schema，产物只有一份 `hysilens.json`。
- 运行时插件包不含 zod；宿主只认 JSON Schema，不执行插件代码。
- 宿主与插件经 MessageChannel 通信，iframe 沙箱隔离；OBS、桌面预览、本地悬浮窗、`hysilens dev` 共用同一个宿主实现。
- 提供 `hysilens` CLI，第三方插件无需本仓库即可开发、构建、打包。

## 包结构

`packages/plugin` → `@hysilens/plugin`（取代 `@hysilens/plugin-sdk`）

| 入口                        | 内容                                                         | 运行环境                      |
| --------------------------- | ------------------------------------------------------------ | ----------------------------- |
| `@hysilens/plugin`          | `definePlugin`、`ctx` 类型、协议客户端                       | 插件 iframe                   |
| `@hysilens/plugin/config`   | `defineConfig`、配置 helper、`z`                             | 仅构建期                      |
| `@hysilens/plugin/host`     | `PluginHost`：iframe、握手、时钟、销毁                       | 桌面端 / OBS / `hysilens dev` |
| `@hysilens/plugin/protocol` | 宿主 ↔ 插件消息类型、manifest 类型                           | 共享                          |
| `@hysilens/plugin/vite`     | Vite 插件：包装入口、收集样式、产出 manifest                 | 构建                          |
| bin `hysilens`              | `build` / `dev` / `validate` / `link` / `pack` / `create`    | Node                          |
| `@hysilens/plugin/samples`  | 测试歌词集（中文、英文、日文、边界情况）                     | 开发工具 / 预览               |
| `@hysilens/devtools`        | `hysilens dev` 的界面（独立包，静态构建后由 dev 服务器提供） | 浏览器                        |

`zod`、`vite` 为 `@hysilens/plugin` 的依赖 / peer 依赖，只在 `config`、`vite`、CLI 中使用。

## 插件目录

```
my-plugin/
  hysilens.config.ts   # 元信息 + 配置 Schema（仅构建期执行）
  src/index.ts         # export default definePlugin<typeof config>({ setup })
  public/              # 原样复制的静态资源（Vite 约定）
  package.json         # { "scripts": { "build": "hysilens build" } }
```

### hysilens.config.ts

```ts
import { color, defineConfig, number, toggle, z } from '@hysilens/plugin/config';

export default defineConfig({
  id: 'tilted',
  name: '透视描边',
  description: '透视、描边与立体阴影',
  viewport: { width: 1280, height: 720 },
  entry: 'src/index.ts', // 默认值
  features: { scale: true, font: true, romanization: true },
  config: {
    perspective: number({ title: '透视距离', min: 200, max: 3000, step: 10, default: 900 }),
    activeColor: color({ title: '高亮颜色', default: '#ffffff' }),
    shadow: toggle({ title: '文字阴影', default: true }),
    mode: z.enum(['a', 'b']).default('a').meta({ title: '模式' }), // 逃生口
  },
});
```

- `id` 为 `<名称>`，名称匹配 `^[a-zA-Z0-9_-]{1,80}$`，安装目录名必须与之一致。`builtin:<名称>` 保留给随应用发布的插件：它们的目录名是 `<名称>`（Windows 路径不能含 `:`），外部插件、`hysilens link`、`hysilens pack` 和 zip 安装都不能使用这个前缀。`version` 取自 `package.json`。

- `config` 中每个字段都必须带默认值，并且能表示为 JSON Schema。`transform`、`preprocess`、`pipe`、`refine` 以及函数默认值在构建时报错。
- helper：`number`、`color`、`toggle`、`select`、`text`、`font`。元数据统一写入 `x-hysilens-*`：
  - `x-hysilens-control`: `color` | `font` | `slider`
  - `x-hysilens-enum-labels`: `{ value: 显示名 }`
  - `x-hysilens-step`: 仅用于 UI 步进，不生成 `multipleOf`（浮点 `multipleOf` 在 ajv / jsonschema 中会误判）
  - `x-hysilens-font-families`: 插件自带字体
- 运行时代码引用 `hysilens.config.ts` 时只能用 `import type`。如果用了值导入，Vite 插件会直接报错。

### features

所有能力默认关闭，声明后宿主注入对应字段并负责编辑器控件：

| feature                                 | 注入字段（默认）                     | 进入 `config()` 类型 | 实现方          |
| --------------------------------------- | ------------------------------------ | -------------------- | --------------- |
| `scale: true`                           | `scale`（1，0.25–3）                 | 否                   | 宿主缩放 iframe |
| `font: true \| { families?, default? }` | `fontFamily`（`default` 或系统默认） | `string`             | 插件            |
| `reveal: true`                          | `revealMode`（`'word'`）             | `'word' \| 'line'`   | 插件            |
| `romanization: true`                    | `romanization`（`true`）             | `boolean`            | 插件            |
| `translation: true`                     | `translation`（`true`）              | `boolean`            | 插件            |

自定义字段与注入字段同名时，构建报错。

### src/index.ts

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
    const stage = document.createElement('main');

    onMounted(async () => {
      root.append(stage);
      await document.fonts.ready;
    });
    onConfigChange((next, prev) => applyTheme(stage, next));
    onLyricsChange(document => rebuild(stage, document));
    onPlaybackChange(({ visible }) => {
      stage.hidden = !visible;
    });
    onUnmounted(async () => {
      await stage.animate({ opacity: [1, 0] }, 300).finished;
    });
  },
});
```

## 运行时 API

```ts
interface PluginContext<Config> {
  root: HTMLElement; // SDK 创建的全尺寸容器，挂在 body 下
  config(): Readonly<Config>; // 已校验、已填默认值
  lyrics(): LyricDocument | null;
  playback(): Playback; // { playing: boolean; visible: boolean }
  time(): number | null; // 歌词时间（已加 offset），按时钟锚点推算；null 表示未知
  preview: boolean; // 是否处于编辑器预览 / 演示模式
  assetUrl(path: string): string;
  signal: AbortSignal; // dispose 时 abort
  report(error: unknown): void; // 非致命错误上报

  onMounted(fn: () => void | Promise<void>): void;
  onUnmounted(fn: () => void | Promise<void>): void;
  onConfigChange(fn: (next: Readonly<Config>, prev: Readonly<Config>) => void): void;
  onLyricsChange(fn: (document: LyricDocument | null) => void): void;
  onPlaybackChange(fn: (playback: Playback) => void): void;
}
```

- `config`、`lyrics`、`playback`、`time` 都是 getter 函数，解构后仍然能读到最新值。
- 钩子只能在 `setup` 中同步注册，`setup` 结束后再注册会抛错。
- 没有响应式系统。如有需要，插件可以在钩子里自行接入 signals 或框架。
- `definePlugin` 是纯函数，只返回描述对象。挂载由 Vite 插件注入的启动代码完成，所以入口模块顶层可以有副作用。

### 生命周期

```
connect ─► init ─► setup() ─► onMounted（await）─► ready ─► 运行中 ─► dispose ─► onUnmounted（await，≤1s）─► disposed
                                    └─ 抛错 / 超时 10s ─► error（宿主显示错误）
```

## 通信协议

1. 宿主创建 `MessageChannel`，iframe `load` 后发送
   `iframe.contentWindow.postMessage({ kind: 'hysilens:connect', apiVersion: 1 }, '*', [port2])`。
   沙箱 iframe 的 origin 为 `null`，所以 targetOrigin 只能用 `*`；端口本身就是通信凭证，之后的消息只走端口。
2. 宿主 → 插件（经端口）：

   | kind       | 载荷                                           | 时机                   |
   | ---------- | ---------------------------------------------- | ---------------------- |
   | `init`     | `{ config, lyrics, playback, clock, preview }` | 连接后一次             |
   | `config`   | `{ config }`                                   | 配置保存 / 切换        |
   | `lyrics`   | `{ document }`                                 | 文档引用变化           |
   | `playback` | `{ playing, visible }`                         | 值变化                 |
   | `clock`    | `{ position, rate }`                           | 锚点变化或漂移超过阈值 |
   | `dispose`  | `{}`                                           | 卸载 / 切换插件        |

3. 插件 → 宿主：`ready`、`error { message, fatal }`、`disposed`。

### 时钟

- `clock.position` 是已应用 offset 的歌词时间，`null` 表示未知；`rate` 为播放速率，暂停时为 `0`。
- 插件收到锚点时用自己的 `performance.now()` 打时间戳，`time()` 返回
  `position + (now - receivedAt) * rate`，单次外推最多 2000 ms（与现有 `playbackPosition` 一致）。
- 宿主以低频计时器（250 ms）计算期望时间，在以下情况才发送新锚点：状态改变、跳转、演示循环回绕，或与插件推算值偏差超过 30 ms。消息不再随每帧发送。

## 产物与安装

`hysilens build` 输出 `dist/`，它本身就是可安装的插件目录：

```
dist/
  hysilens.json
  index.js
  style.css        # 如有
  ...              # public/ 与 Vite 产出的资源
```

```json
{
  "apiVersion": 1,
  "id": "tilted",
  "name": "透视描边",
  "version": "0.1.0",
  "description": "…",
  "entry": "index.js",
  "styles": ["style.css"],
  "viewport": { "width": 1280, "height": 720 },
  "features": { "scale": true, "font": true, "romanization": true },
  "configSchema": { "$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object" },
  "revision": "<内容 sha256>"
}
```

- 内置插件：`apps/desktop/public/plugins/<id>/`，只包含 `scripts/build-plugins.ts` 中 `BUNDLED` 名单里的插件（`builtin` 与 `tilted`）。外部插件：`<data>/plugins/<id>/`，同名时外部优先。
- `hysilens pack` 先构建，再输出 `<id>-<version>.zip`，压缩包根目录就是 `dist/` 的内容；应用通过 `POST /api/plugins/install` 安装它。
- Rust 只读取 `hysilens.json`，校验 `apiVersion`、`id` 与目录名一致（内置目录为 `builtin:<目录名>`）、`entry` 和 `styles` 路径不越界，以及 `configSchema` 是合法的 JSON Schema。

## 宿主

- `GET /api/plugins/{id}/view` 生成插件页，内容只有：`<base>`、styles、`<script type="module" src=entry>`。SDK 运行时已经打包在 entry 里。
- iframe 使用 `sandbox="allow-scripts"`。插件页是 opaque origin，因此：
  - `/plugins/*` 与 view 需要返回 `Access-Control-Allow-Origin: *`（ES module 和字体都需要）；
  - 插件中不能使用 `localStorage`、cookie、`parent` 访问。
- `PluginHost`（`@hysilens/plugin/host`）负责：创建 iframe、握手、ready 超时、推送 config / lyrics / playback / clock、dispose 超时、状态事件。桌面端 `PluginView`、预览面板、本地悬浮窗、`hysilens dev` 都使用它。
- 配置：`GET/PUT /api/plugins/{id}/config`。前端用 ajv（`useDefaults`）填默认值并校验，Rust 保存时再用 jsonschema 校验。文件保存在 `plugin-configs/<id>.json`，内置插件为 `plugin-configs/builtin/<名称>.json`。
- `scale` 由宿主对 iframe 做 CSS 缩放，插件感知不到。

## CLI

```bash
hysilens create <dir>   # 脚手架
hysilens dev            # Vite dev + 开发工具（内置于 @hysilens/plugin）+ HMR
hysilens build          # dist/ + dist/hysilens.json
hysilens validate [dir] # 按宿主规则校验产物
hysilens link           # 将 dist/ 链接到用户插件目录 <data>/plugins/<id>
hysilens pack           # 构建并打包为 <id>-<version>.zip（--no-build 只打包）
```

### 开发工具构建

`packages/devtools` 仅作为私有工作区源码包，不单独发布。运行
`vp run @hysilens/plugin#build` 会构建开发工具，将 HTML、JS 和 CSS 输出到
`packages/plugin/dist/devtools/`。CLI 与 SDK 通过 Vite+ 的 `vp pack` 构建为 ESM JS
及类型声明，统一输出到 `packages/plugin/dist/`，包内仅包含 `dist`；
`prepack` 也会执行该构建。

`hysilens dev` 直接注册包内静态资源：`/` 提供开发工具页面，
`/__hysilens/devtools/` 提供资源，`/__hysilens/plugin` 提供插件 iframe 页面。
运行时不再查找 `@hysilens/devtools`，也不会临时构建它。
仓库首次运行开发服务或测试前，以及修改开发工具源码后，需要重新执行上述构建命令。

## 实施阶段

| 阶段 | 内容                                                                                                                                                                                                                            | 负责   |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| P1   | `packages/plugin` 骨架；`/protocol`；运行时（`definePlugin`、ctx、时钟、生命周期）；`/host`（`PluginHost`）；测试                                                                                                               | Claude |
| P2   | `/config`：`defineConfig`、helper、features 注入、JSON Schema 生成与类型推导；测试                                                                                                                                              | Claude |
| P3   | `/vite` 插件、CLI（`build` / `validate` / `link` / `pack` / `dev` / `create`）；改写 `scripts/build-plugins.ts`                                                                                                                 | Claude |
| P4a  | 迁移 `builtin`（作为参考实现，内部仍可用 React）                                                                                                                                                                                | Claude |
| P4b  | 迁移其余插件；删除 `lyric-scenes`                                                                                                                                                                                               | Claude |
| P5   | Rust：只读 `hysilens.json`、view 页、CORS、`/config` 接口；桌面端：`PluginView` / 预览 / 悬浮窗改用 `PluginHost`、编辑器改用 ajv；删除 `plugin-sdk`、`bridge.js`、`plugin-frame.ts`、`validateTheme`、`applyPluginCapabilities` | Claude |
| P6   | 文档（替换 `renderer-plugins.md` 接口部分、`AGENTS.md`）；`vp check`、`vp test`、`cargo test`                                                                                                                                   | Claude |

说明：

- `@hysilens/plugin-sdk` 与 `@hysilens/lyric-scenes` 已删除。
- 窗口、OBS 和播放器交互由用户手动验证。
