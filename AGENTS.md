# 目的：记录 DropAir 的协作与构建约定。

## 构建约定

- 远程编译统一使用 GitHub Actions；不要改用本地或其他远程机器作为远程编译环境。
- 将改动推送到 `main` 后，由仓库中的 GitHub Actions 工作流执行 macOS 和 Windows 构建。
