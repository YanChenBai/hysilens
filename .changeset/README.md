# Changesets

运行 `vp run changeset` 描述改动及其版本号变更：

- `@hysilens/plugin`：发布到 npm，并创建 `@hysilens/plugin@<version>` GitHub Release。
- `@hysilens/desktop-ui`：桌面端版本，发布时构建 `hysilens.exe` 并创建 `v<version>` GitHub Release。

改动合入 `main` 后，Release 工作流会打开版本 PR；合并该 PR 即完成发布。
