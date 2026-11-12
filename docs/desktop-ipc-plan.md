# 桌面主界面 IPC 迁移

主窗口的控制与状态订阅统一使用 Tauri IPC，由 `hysilens-core` 持有播放器、配置、plugin registry 和诊断状态
原来用于主界面的 HTTP 控制路由已经移除，主窗口不保留 HTTP/WebSocket 回退

## 边界

- 桌面顶层 `main` 窗口的 `/control`、`/settings` 页面使用 IPC
- 主控制页只使用 IPC，浏览器访问控制页时不回退到 HTTP/WebSocket
- headless 服务保留 OBS 与歌词渲染所需的只读资源和状态通道
- OBS、歌词窗口、预览 iframe 与 plugin 的页面及资源继续通过本地 HTTP 提供
- 不通过 IPC 代理任意 URL，不重新启动第二份采集服务
- 保持现有界面、React Query 缓存键、plugin SDK 和 snapshot 协议

## 实施结果

- [x] 提取进程内核心控制入口，移除原来用于主界面的 HTTP 控制路由
- [x] Tauri host 管理当前核心服务句柄，服务重启时更新句柄
- [x] 注册有明确参数的控制 command，复用主控制窗口的权限校验
- [x] 迁移设置、字体、目录选择、诊断、酷狗增强和 plugin 管理
- [x] plugin ZIP 通过二进制 IPC 安装，不转成 JSON 数字数组
- [x] 主窗口的状态、plugin 变更通知和播放控制使用 IPC channel
- [x] 补充无 HTTP 降级、错误传播、旧接口移除、状态增量、订阅清理及重连通知的单元测试

HTTP 只保留 `/api/events`、`/api/plugins`、`/api/plugins/{id}/config`、`/api/plugins/{id}/view` 与 `/plugins/{id}/{path}`，供歌词页面读取状态、元数据和资源
事件 WebSocket 只接受 clock 探测，不再接受播放控制命令

## 生命周期与错误

- IPC 操作和歌词渲染使用同一份 reducer、设置及 plugin registry
- 服务未就绪或重启期间返回明确状态，订阅在新服务就绪后恢复
- 页面卸载或订阅替换时释放旧 channel；服务重启时更新 snapshot epoch 并刷新缓存
- IPC 失败不静默切回 HTTP，避免重复执行删除、安装、启动或诊断操作
- 主界面不定时调用 control command；播放状态附带发送时间戳，时钟对齐随状态推送完成
- 酷狗增强状态由核心服务探测进程和调试端口，每轮完成后等待 500ms 再采样、变化才通过 channel 推送；主界面只消费缓存
- plugin iframe、OBS 页面及歌词窗口不能取得主窗口的控制权限
- 诊断导出保留并发保护，plugin 删除保留内置保护和链接处理

## 验证

- `vp run check:types` 与 `vp run @hysilens/desktop#build:web` 通过
- `vp test run`：105 项通过
- `cargo fmt --all --check` 与 `cargo clippy --workspace --all-targets -- -D warnings` 通过
- `cargo test --workspace --all-targets`：143 项通过，6 项忽略
- 全仓 `vp check` 仍被未改动的 `plugins/scroll/package.json`、`plugins/scroll/tsconfig.json` 格式问题阻挡
- 尚未手动验证：主窗口、浏览器控制页拒绝降级、服务端口变更、主窗口关闭再打开、plugin 安装与删除、目录选择、诊断、酷狗增强
- 按 `AGENTS.md` 不自动启动应用、服务器或 smoke；窗口和播放器交互由用户手动验证

## 完成条件

- 桌面主窗口不再用 HTTP/WebSocket 发起控制操作或订阅状态
- 浏览器控制页不发送 HTTP 控制请求，OBS 和 plugin 资源保持可用
- 参数校验、权限限制、错误反馈和订阅释放有对应验证
- PR 正文记录实际验证结果，未完成手动验证前保持 Draft
