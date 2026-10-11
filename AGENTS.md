# 目的：记录 DropAir 的协作与构建约定。

## 构建约定

- 远程编译统一使用 GitHub Actions；不要改用本地或其他远程机器作为远程编译环境。
- 将改动推送到 `main` 后，由仓库中的 GitHub Actions 工作流执行 macOS 和 Windows 构建。

## 完成流程

- 功能改动完成并通过可用的本地检查后，自动提交本次相关改动并推送到 `main`，触发 GitHub Actions 的 macOS 和 Windows 编译。
- 提交前排除与本次任务无关的修改和未跟踪文件；编译完成后记录两个工作流的结果。
- 每次代码、配置、文档或产品行为修改都必须发布新的版本：同步更新 `package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 中的版本号，提交并推送 `main` 后创建匹配的 `vX.Y.Z` tag 并推送。
- 必须等待该 tag 对应的 GitHub Actions `Release` 工作流完成，确认 Release 已发布并包含 `latest.json` 及目标平台安装包后，记录 Release、macOS 和 Windows 工作流结果；不得复用已有 tag 或只停留在 `main` 提交。
