# 桌面主界面 IPC 迁移

主窗口目前通过本地 HTTP 请求和 WebSocket 控制进程内服务；桌面已有的悬浮窗开关则使用 Tauri IPC
本次把主窗口的控制与状态订阅统一到 IPC，继续由 `hysilens-core` 持有播放器、配置、plugin registry 和诊断状态

## 边界

- 桌面顶层 `main` 窗口的 `/control`、`/settings` 页面使用 IPC
- 浏览器控制页和 headless 服务保留 HTTP/WebSocket 通信
- OBS、歌词窗口、预览 iframe 与 plugin 的页面及资源继续通过本地 HTTP 提供
- 不通过 IPC 代理任意 URL，不重新启动第二份采集服务
- 保持现有界面、React Query 缓存键、plugin SDK 和 snapshot 协议

## 实施顺序

1. 提取核心控制入口，HTTP handler 与 IPC 共用服务操作和校验
2. Tauri host 管理当前核心服务句柄，服务重启时更新句柄
3. 注册有明确参数的控制 command，只授权当前主窗口、顶层 frame 和控制页面
4. 增加前端控制 client，迁移设置、字体、目录选择、诊断、酷狗增强和 plugin 管理
5. plugin 安装通过 IPC 二进制请求传递 ZIP，不转成 JSON 数字数组
6. 主窗口的状态、plugin 变更通知和播放控制迁到 IPC channel；浏览器继续使用 WebSocket
7. 补充通信选择、错误传播、权限边界、订阅清理及重连测试

## 生命周期与错误

- IPC 与 HTTP 使用同一份 reducer、设置及 plugin registry
- 服务未就绪或重启期间返回明确状态，订阅在新服务就绪后恢复
- 页面卸载或订阅替换时释放旧 channel；服务重启时更新 snapshot epoch 并刷新缓存
- IPC 失败不静默切回 HTTP，避免重复执行删除、安装、启动或诊断操作
- plugin iframe、OBS 页面及歌词窗口不能取得主窗口的控制权限
- 诊断导出保留并发保护，plugin 删除保留内置保护和链接处理

## 验证

- 前端：`vp check`、`vp run check:types`、`vp test run`
- Rust 编译前：`vp run @hysilens/desktop#build:web`
- Rust：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --all-targets`
- 手动：主窗口与浏览器控制页、服务端口变更、主窗口关闭再打开、plugin 安装与删除、目录选择、诊断、酷狗增强
- 按 `AGENTS.md` 不自动启动应用、服务器或 smoke；窗口和播放器交互由用户手动验证

## 完成条件

- 桌面主窗口不再用 HTTP/WebSocket 发起控制操作或订阅状态
- 浏览器控制页、OBS 和 plugin 资源保持可用
- 参数校验、权限限制、错误反馈和订阅释放有对应验证
- PR 正文记录实际验证结果，未完成手动验证前保持 Draft
