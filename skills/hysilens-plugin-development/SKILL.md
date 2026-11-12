---
name: hysilens-plugin-development
description: '使用 @hysilens/plugin 创建、开发、调试和打包 Hysilens 歌词渲染插件，处理配置、生命周期、歌词计时、动画、字体资源和 Chromium 90 兼容。'
---

# Hysilens 插件开发

使用 `@hysilens/plugin` 的公开 API 和 `hysilens` CLI 开发歌词渲染插件
先查看项目的 `package.json`、`hysilens.config.ts`、入口文件与项目约定，按现有框架和脚本完成任务
API 参数和类型以项目依赖的公开类型声明为准

## 创建项目

```powershell
vp dlx @hysilens/plugin create my-lyrics --name 我的歌词
cd my-lyrics
vp install
```

生成的项目通过 `@hysilens/plugin` 定义插件，通过 `@hysilens/plugin/config` 声明配置
React、Vue、Canvas、WebGL 和原生 DOM 均可使用；按渲染需求选择框架

## 配置与入口

- 在 `hysilens.config.ts` 中使用 `defineConfig` 声明 ID、名称、建议画布尺寸、features 和配置字段
- 每个配置字段提供默认值，配置须能转换为 JSON Schema；使用 SDK 的字段辅助函数，不使用 transform、preprocess 或 refine
- 字体使用 `font()` 字段声明，并在渲染中应用 `ctx.config().fontFamily`；字体族名与 CSS 的 `@font-face` 保持一致
- 仅声明实际实现的 features；`romanization`、`translation`、`reveal` 需要对应的渲染支持，`scale` 由宿主应用
- 配置模块只在构建期执行；入口通过 `import type config from '../hysilens.config'` 引用类型，避免把配置模块和 zod 打进渲染代码
- 使用默认导出的 `definePlugin<typeof config>({ setup(ctx) { ... } })` 定义插件
- `dist/hysilens.json` 由构建生成，修改插件描述时编辑配置文件

## 生命周期与时间

- `setup` 同步执行，在其中注册生命周期钩子；异步初始化放在 `onMounted` 中
- DOM 挂到 `ctx.root`；框架组件在 `onMounted` 挂载，在 `onUnmounted` 卸载
- `ctx.config()`、`ctx.lyrics()`、`ctx.playback()`、`ctx.time()` 是实时读取函数；配置和歌词变化后更新相关状态
- `ctx.time()` 是已应用 offset 的歌词时间，单位毫秒，未知时为 `null`；直接用它计算动画进度
- 动画循环通过 `requestAnimationFrame` 读取歌词时间，暂停时冻结，seek 后按当前位置重算
- 用 `ctx.playback().visible` 控制画面显隐，播放与可见状态分别处理
- `onUnmounted` 中取消 RAF、定时器、observer 和订阅；事件监听器可使用卸载后自动 abort 的 `ctx.signal`
- 空歌词、空轨道和未知时间应清理或隐藏画面；切歌后重新建立文档相关状态

## 歌词排版与动画

- 行起点已排序，行和 token 的结束时间可能跨过下一句起点；`endMs: null` 按下一句起点确定结束边界
- 单句布局在下一句开始时替换旧句；多句、接力和叠放布局按设计分配位置
- 区分切句与歌词空档：空档和末句可保留退场动画，快速切句与反向 seek 不应累积节点
- 逐字时间处理空 token、整行时间、英文空格、emoji 和组合字符；使用字素分段计算字符，不按 UTF-16 长度拆字
- 字体加载、画布 resize 和配置变化后重新测量换行与位置；同时考虑放大、透视、整体缩放和裁切范围
- 按插件的动态效果配置控制动画；面向直播输出时，明确系统 `prefers-reduced-motion` 与插件开关的优先级，避免宿主偏好静默关闭已启用的效果

## Chromium 90 兼容

- 浏览器源以 Chromium 90 为基线，JS 和 CSS 构建目标使用 `chrome90`
- 编译目标负责语法转换，不会补齐运行时 API；使用基线支持的 API，或在插件中实现普通兼容函数
- 避免 `Array.at`、`findLast`、`findLastIndex`、`structuredClone` 等超出基线的调用；使用索引、循环或局部辅助函数，不修改全局对象或内建原型
- CSS 的容器单位、容器查询、动态混色等特性需要基线可用的写法或回退样式；核对构建产物，不能只看编译目标
- 实际内核测试使用 Chromium 90 或目标直播软件；修改现代浏览器的 User-Agent 不会改变内核能力

## 资源与运行环境

- 插件运行在 `sandbox="allow-scripts"` 的 iframe 中，使用 `ctx.root` 和 SDK 提供的数据完成渲染
- CSS、字体和图片通过模块导入或 `public/` 随插件发布；动态读取资源使用 `ctx.assetUrl(path)`
- 不依赖 parent DOM、localStorage、cookie 或 Tauri 接口，不硬编码宿主端口和用户磁盘路径
- 字体保留原版权与完整许可证，打包时确认离线资源齐全

## 调试、验证与交付

使用项目的包管理器和本地 CLI；以下是 Vite+ 项目的常用命令：

```powershell
vp run dev
vp run build
vp exec hysilens validate
vp run pack
```

- `hysilens dev` 提供歌词预览与调试工具；按任务授权和项目约定启动，或使用已经运行的服务
- 根据项目现有脚本运行格式、类型检查和测试；计时或场景问题补充能复现问题的回归用例
- 用演示歌词或项目自己的测试数据检查逐字揭示、切句、空档、暂停、正反向 seek、resize 和卸载
- 构建成功后运行 `hysilens validate` 校验产物；`hysilens pack` 生成可安装的单插件 ZIP
- `hysilens link` 用于本地安装调试，在任务包含安装或链接时使用
- 修改后重新构建与打包，交付准确的 ZIP 路径，并说明已完成的检查和实际播放验证情况
