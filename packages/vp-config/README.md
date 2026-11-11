# @hysilens/vp-config

工作区内部的 Vite+ lint/fmt 配置，以源码导出，无需先构建。

```ts
import { qualityConfig } from '@hysilens/vp-config';
import { defineConfig } from 'vite-plus';

export default defineConfig({
  ...qualityConfig,
  // 应用自身的 plugins、build、test 等配置放在这里。
});
```

根配置统一使用此预设，包内 `vp check` 也使用工作区根配置。

- 格式：单引号、100 列、自动整理 imports、Tailwind 类名和 package.json。
- 分段：export、代码块、多行变量声明之间保留空行，类方法之间保留空行。
- TypeScript：类型检查、类型导入一致性、穷尽分支和常见错误检查。
- TSX/JSX：仅针对 React 文件启用 Hooks、依赖项、列表 key、属性和 ARIA 检查；使用自动 JSX runtime，无需导入 React。
- `vp check --fix` 自动修复可修复的问题；`vp check` 用于 CI 验证。
