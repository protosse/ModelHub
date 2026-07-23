# ModelHub

Provider-first 模型配置中枢，管理 Claude Code / Codex / OpenCode / Pi 的模型配置。

需求文档见 [`docs/REQUIREMENTS.md`](docs/REQUIREMENTS.md)。

## 开发

```bash
pnpm install
pnpm tauri dev
```

需要本机已安装 Node.js、pnpm 与 Rust 工具链。

数据目录：`~/.modelhub/`（`store.json` / `secrets.json` / `backups/`）

## 技术栈

- Tauri 2 + React + TypeScript + Tailwind
- Rust adapters 读写各 Agent 配置
