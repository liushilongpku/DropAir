# 目的：记录 DropAir 的协作与构建约定。

## 构建约定

- 远程编译统一使用 GitHub Actions；不要改用本地或其他远程机器作为远程编译环境。
- 将改动推送到 `main` 后，由仓库中的 GitHub Actions 工作流执行 macOS 和 Windows 构建。

## 完成流程

- 功能改动完成并通过可用的本地检查后，自动提交本次相关改动并推送到 `main`，触发 GitHub Actions 的 macOS 和 Windows 编译。
- 提交前排除与本次任务无关的修改和未跟踪文件；编译完成后记录两个工作流的结果。
