---
name: hysilens-code-style
description: 'Use when editing, reviewing, formatting, or adding comments to Rust/TS/TSX in the hysilens repo: Chinese comments without 。, multiline JSDoc/block comments, blank-line layout, oxlint and Cargo lint rules, and the verification commands.'
---

# hysilens 代码风格

本仓库的注释、空行与 lint 约定。改代码、加注释、调格式或 review 前先按此执行。

## 注释语言

- 注释和文档用中文；不要写中文句号 `。`（句中需要断句用 `；` 或换行）
- 技术名词保留英文，不要硬翻译：plugin、host、overlay、renderer、schema、payload、snapshot、token、cache、buffer、handle、lock、channel、hook、effect、props、ref、iframe、WebView、WinRT、COM、Toolhelp、adapter、manifest、catalog、thread、frame、debounce 等
- 只写签名看不出来的信息：单位、范围、默认值、状态转换、生命周期、资源归属、线程约束、错误条件
- 不要为了加而加。`describe`/`it`/`#[test]` 名称已说明行为时不要再加注释，显然的字段（`id`、`name`、`label`）不写注释
- 注释过期就改，不确定就不编

## TypeScript / TSX

- JSDoc 一律多行，即使只有一句话

  ```ts
  /**
   * 插件在面板中的显示名；保存前会去掉首尾空白
   */
  name: string;
  ```

- 接口和 type 字面量里，属性之间留空行；JSDoc 紧贴所属属性；上一属性与下一条注释之间留空行

  ```ts
  interface ScrollPosition {
    /**
     * 水平偏移，单位逻辑像素
     */
    x: number;

    /**
     * 垂直偏移，单位逻辑像素
     */
    y: number;
  }
  ```

- 解释性注释用 `/* ... */` 块注释，不用 `//`。`// @ts-expect-error`、`eslint-disable` 等指令必须保留 `//`
- 禁止行尾注释（oxlint `no-inline-comments`）
- 逻辑分组之间、`return` 上方留空行；`if`/`for` 必须带大括号且内容换行（`curly` + `stylistic/brace-style`）
- JSX 属性一行一个；对象字面量里的注释独占行，并与相邻属性用空行隔开
- 测试只调整 arrange/act/assert 空行，不改测试名

## Rust

- 每个源文件有 `//!` 模块文档；所有 `pub` item、字段、枚举变体有 `///`
- 公开的 `Result` API 用 `# Errors`；`unsafe` item 用 `# Safety`
- 每个 `unsafe` 块上方有 `// SAFETY:`，写清楚指针、长度、生命周期、句柄、线程等具体前置条件，不要写空话
- struct 字段、enum 变体逐项空行；doc/attribute 紧贴字段
- 函数内按 validation / 状态计算 / 副作用 / cleanup 分组空行；连续单行 `let` 可成组，多行语句、控制流、注释前后留空行；`Ok(..)`/`Err(..)`/`return` 上方空行
- rustfmt 只用稳定选项，不稳定选项（`wrap_comments`、`imports_granularity` 等）会让 `cargo fmt` 告警

## lint 与格式配置位置

- oxlint / oxfmt：`packages/vp-config/src/index.ts` 的 `qualityConfig`
  - `fmt`：100 列、单引号、import sort、package.json sort
  - `lint`：`curly`、`stylistic/brace-style`（`allowSingleLine: false`）、`stylistic/padding-line-between-statements`、`no-inline-comments`、`stylistic/multiline-comment-style: ['error', 'starred-block']`、`max-depth`、`complexity`
- Rust：根 `Cargo.toml` 的 `[workspace.lints]`，各 crate 用 `[lints] workspace = true` 继承
  - `missing_docs`、`missing_debug_implementations`、`rust_2018_idioms`、`unsafe_op_in_unsafe_fn`、`unused_must_use`
  - `clippy::undocumented_unsafe_blocks`、`dbg_macro`、`todo`、`unimplemented` 为 deny
- 格式化目标：`rustfmt.toml`（100 列、4 空格、reorder imports）

## 验证

```bash
vp check
vp run check:types
vp test run
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

UI 改动按 `AGENTS.md` 由用户手动验证，不自动启动应用

## 反模式

- 为凑数写注释、把签名复述一遍
- 单行 `/** ... */`
- 属性/字段之间没有空行，或注释与所属属性被空行拆开
- 注释里写 `。`
- 把 WinRT、COM、pointer chain 之类的术语硬翻成中文
- 顺便批量格式化无关文件，或改动生成文件与 `test/fixtures` 里的非注释内容
