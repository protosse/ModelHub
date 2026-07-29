# ModelHub

Provider-first 模型配置中枢，管理 Claude Code / Codex / OpenCode / Pi 的模型配置。

需求文档见 [`docs/REQUIREMENTS.md`](docs/REQUIREMENTS.md)。

## 开发

```bash
pnpm dev:tauri
```

跨平台入口 `scripts/dev-launch.mjs` 会按系统调用：

- Windows：`scripts/dev.ps1`（优先 `pwsh`；也可用 `scripts/dev.cmd`）
- macOS / Linux：`scripts/dev.sh`

也可直接：

```bash
# macOS / Linux
bash scripts/dev.sh

# Windows（推荐 pwsh）
pwsh -File scripts/dev.ps1
# 或
powershell -ExecutionPolicy Bypass -File scripts/dev.ps1
# 或 CMD
scripts\\dev.cmd
```

脚本会：

1. 检查 node / pnpm / cargo
2. 依赖缺失时自动 `pnpm install`
3. 用 `scripts/ensure-dev-port.mjs` 从首选端口（默认 `1420`，可用 `--port` / `MODELHUB_DEV_PORT`）起找空闲端口；**占用则自动顺延，不杀对方进程**（同时探测 `127.0.0.1` 与 `::1`，避免 Windows 上 Vite 只绑 IPv6 时漏检）
4. 同步 Vite（`MODELHUB_DEV_PORT` + 显式 `beforeDevCommand: vite --port`）与 Tauri `devUrl` 后启动 `pnpm tauri dev`

只检查、不启动：

```bash
pnpm dev:check
# 或
bash scripts/dev.sh --check
pwsh -File scripts/dev.ps1 --check
```

需要本机已安装 Node.js、pnpm 与 Rust 工具链。

数据目录：`~/.modelhub/`（`store.json` / `secrets.json` / `backups/`）

## 技术栈

- Tauri 2 + React + TypeScript + Tailwind
- Rust adapters 读写各 Agent 配置
