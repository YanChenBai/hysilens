---
name: hysilens-plugin-development
description: '开发、修复和打包 Hysilens 歌词渲染插件，包括 definePlugin 生命周期、配置 Schema、歌词计时与切句动画、devtools 复现、字体资源和安装包校验。用于内置或外部 renderer，不用于播放器采集、CDP 或宿主窗口功能。'
---

# Hysilens 插件开发

## 确认项目与 SDK

- 先读当前仓库的 `AGENTS.md`、`package.json`、插件的 `hysilens.config.ts` 与构建任务，检查 Git 状态；不要把相邻仓库的改动混在一起
- 主仓库 `hysilens` 的 `plugins/<id>` 是内置插件；相邻的 `hysilens-plugins` 是独立 Git 仓库，里面的插件使用已安装的 SDK，可能落后于主仓库源码
- 确认实际使用的 `@hysilens/plugin` 版本、exports 和类型；不要把主仓库新增的 API 或测试歌词误认为已发布 SDK 中也有
- 在主仓库内按需查阅 `docs/renderer-plugins.md`、`docs/plugin-system.md`，以 `packages/plugin/src/runtime.ts`、`config.ts`、`protocol.ts`、`cli.ts` 和实际调用方为准；文档中的历史路径不一定仍然有效
- 主仓库 TS/TSX 修改遵循其 `hysilens-code-style` 技能；外部仓库遵循自己的规则与现有风格

## 创建与配置

- 新建插件优先使用安装的 SDK 提供的 `hysilens create <dir> --name <名称>`，现有插件按原有结构修改，不要求迁移渲染框架
- `hysilens.config.ts` 只在构建期执行；运行时代码用 `import type config from '../hysilens.config'` 和 `definePlugin<typeof config>`，不要把配置模块或 zod 值导入 iframe
- 插件 ID、安装目录和构建 manifest 必须对应；`builtin:` 保留给内置插件，外部插件使用普通 ID
- 配置字段都要有默认值，并能完整转换为 JSON Schema；不要使用无法被宿主表达的 transform、preprocess、refine 等校验逻辑
- 字体优先声明显式 `font()` 配置，并由插件应用 `config().fontFamily`；`features.font` 是兼容路径，不要把两种方式叠加成重复控件
- 仅声明真正实现的 features：`romanization`、`translation`、`reveal` 需要插件渲染支持；`scale` 由宿主缩放 iframe，不在插件里重复缩放
- 不手写或手改生成的 `dist/hysilens.json`，由 SDK 构建生成

## 生命周期与歌词时间

使用默认导出的 `definePlugin({ setup(ctx) { ... } })`，而不是旧版全局对象或自建宿主通信协议

- `setup` 必须同步，所有生命周期钩子在其中同步注册；异步初始化放进 `onMounted`
- DOM 挂到 `ctx.root`，React/Vue 等框架在 `onMounted` 挂载，在 `onUnmounted` 卸载
- `config()`、`lyrics()`、`playback()`、`time()` 是实时读取函数；不要在 setup 时解构一次结果后长期复用
- `time()` 是已应用 offset 的歌词时间，单位毫秒，未知时为 `null`；不再额外加 offset，也不使用 `Date.now()` 自己维护歌词时钟
- 宿主发送时钟锚点，SDK 推算中间时间；逐字动画可在 `requestAnimationFrame` 中读取 `time()`，不要等待宿主每帧推送
- 暂停时歌词时间应冻结；`playback().visible` 与 `playing` 含义不同，按 visible 控制隐藏，不自行把两者视为等价
- 在 `onUnmounted` 取消 RAF、定时器、observer 和自建订阅，移除拥有的 DOM；事件监听器可使用 `ctx.signal`，该 signal 在卸载钩子完成后 abort
- 空文档、空轨道和未知时间都应能清理或隐藏旧画面；切歌时重建文档相关状态，不沿用旧句索引

## 排版与切句

- 行起点已排序，但行与 token 的结束时间可能跨过下一句起点；`endMs: null` 表示该句保持到下一句开始，不应直接算成零或无穷大的退场动画
- 同一位置只显示一句的设计，在下一句开始时移除或隐藏旧句，避免旧句等待消散而与新句重叠；多句叠放或接力排版则按设计分配独立位置，不把单句规则强加给所有主题
- 区分“下一句已开始”和“当前句结束后的空档”：空档、末句可以保留消散；快速切句、反向 seek、重复相同时间更新都不能累积旧节点
- 逐字处理考虑空 token、只有整行时间、英文词间空白、emoji 与组合字符，不用 UTF-16 字符长度假装字素数量
- 测量字体与换行后再定位；字体加载、窗口 resize、配置变化后重新测量，避免用固定行高推断多行歌词的位置
- 排查“有空间却提前换行”时，同时检查布局宽度、当前行放大、透视投影、整体 fit 和裁切范围；不要仅调字号掩盖宽度问题
- 动画遵从 `prefers-reduced-motion`，在暂停与 seek 后按当前时间重算，而不是靠累计播放动画维持状态

## 资源与沙箱

- 插件在 `sandbox="allow-scripts"` 的 opaque-origin iframe 中运行；不要依赖 parent DOM、localStorage、cookie 或 Tauri 写接口
- CSS、字体、图片通过模块导入或 `public/` 发布；动态读取插件文件使用 `ctx.assetUrl(path)`，不硬编码宿主端口和用户磁盘路径
- 字体在 CSS 中声明 `@font-face`，保留可再分发字体的原版权与完整许可证，打包验证离线资源；配置里的字体族名须与 CSS 一致
- 不为了展示效果自行添加远程字体、CDN 或网络服务依赖；有明确需求时再评估并说明实际依赖

## 验证、devtools 与交付

按当前包的 scripts/task 执行构建，使用 Vite+ 时区分内置命令 `vp <name>` 与项目任务 `vp run <name>`，TS 脚本使用 `vpx`

典型验证命令，按仓库定义和改动范围选择：

```powershell
vp check
vp run check:types
vp test run
vp run --filter <插件包名> build
```

- 修复计时或场景归属问题时，补能触发旧行为的回归测试，覆盖需要的正向/反向 seek、快速切句、隐藏和卸载；不要用只检查实现细节的断言充数
- devtools 的测试歌词包含中文、英文、日文和边界场景；较新的 SDK 还提供 `overlap`（切句重叠 · 密集歌词），先确认当前安装版本确实包含该用例
- 新增可复用歌词场景时改主仓库 `packages/plugin/src/samples.ts` 并补对应测试；外部插件正在使用旧 SDK 时，不能声称该场景已自动出现在它的 devtools 中
- 按当前任务授权和仓库约定决定是否运行 `hysilens dev` 或使用已有服务器；主仓库约定不自动启动应用、服务器或 smoke，视觉效果由用户手动验证
- 内置插件构建通过后，使用主仓库任务 `vp run @hysilens/desktop#build:web` 更新嵌入资源；内置名单当前由 `apps/desktop/scripts/bundle-plugins.ts` 管理，必要时核对当前实现
- 外部插件在插件目录用本地 CLI 校验和打包；Windows 可用 `vp exec hysilens validate`、`vp exec hysilens pack --no-build`，或 `.\node_modules\.bin\hysilens.cmd validate`，前提是 build 已成功且产物最新
- 新目录或疑似旧产物可显式验证 `hysilens validate <dir>`；打包后提供准确的 ZIP 路径，不把整个仓库、多个插件包的外层 ZIP 或旧包当成可直接安装的单插件包
- `hysilens link` 会写入用户插件目录，仅在安装/链接调试属于任务范围时执行；生成 ZIP 不代表已安装或已视觉验收
- 构建失败时区分源码错误、旧 SDK、缓存命中和系统权限错误；Vite+ 环境异常时运行 `vp env doctor`，保留原始错误，不把缓存恢复表述为一次新的完整构建
- 最终说明改了什么、哪些检查通过、产物位置，以及尚未完成的实际播放或视觉验证；提交、push、SDK 发布与插件安装分别按用户授权处理
