#!/usr/bin/env bash
# ModelHub 开发启动脚本
#
# 对齐 CourserHub：端口占用时顺延，不杀其他进程；端口由 ensure-dev-port.mjs 统一探测
#（同时覆盖 127.0.0.1 与 ::1，避免只绑 IPv6 时漏检）。
#
# 1. 检查基础工具（node / pnpm / cargo）
# 2. 必要时 pnpm install
# 3. 选择空闲前端端口（默认 1420，占用则顺延）
# 4. 同步 Vite + Tauri devUrl 后启动 pnpm tauri dev
#
# 用法（仓库根目录）：
#   bash scripts/dev.sh
#   bash scripts/dev.sh --only-prepare   # 只检查/装依赖，不启动
#   bash scripts/dev.sh --check          # 同 --only-prepare
#   bash scripts/dev.sh --port 1431      # 指定首选端口
#   pnpm run dev:tauri
#
# 环境变量：
#   MODELHUB_DEV_PORT / TAURI_DEV_PORT   首选前端端口（默认 1420）
#   TAURI_DEV_HOST                       非空时额外要求 HMR 端口 (port+1) 空闲
#   ONLY_PREPARE=1                       同 --only-prepare
#   HTTP_PROXY / HTTPS_PROXY / ALL_PROXY
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ONLY_PREPARE="${ONLY_PREPARE:-0}"
PREFERRED_PORT="${MODELHUB_DEV_PORT:-${TAURI_DEV_PORT:-1420}}"

die_early() { echo "error: $*" >&2; exit 1; }

usage() {
  cat <<'USAGE'
ModelHub 开发启动脚本

用法：
  bash scripts/dev.sh
  bash scripts/dev.sh --only-prepare   # 只检查工具链与依赖，不启动
  bash scripts/dev.sh --check          # 同 --only-prepare
  bash scripts/dev.sh --port 1431      # 指定首选前端端口（占用则自动顺延）
  pnpm run dev:tauri                   # 同上
  pnpm run dev:tauri -- --port 1431

流程：
  1. 检查 node / pnpm / cargo
  2. 必要时 pnpm install
  3. node scripts/ensure-dev-port.mjs 选空闲端口（默认 1420；占用则顺延，不杀进程）
  4. pnpm tauri dev（MODELHUB_DEV_PORT + tauri --config 覆盖 devUrl / beforeDevCommand）

环境变量：
  MODELHUB_DEV_PORT / TAURI_DEV_PORT   首选端口
  TAURI_DEV_HOST                       远程/HMR 场景；需 port 与 port+1 都空闲
  ONLY_PREPARE=1
USAGE
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help) usage ;;
    --only-prepare|--check) ONLY_PREPARE=1; shift ;;
    --port)
      shift
      [[ $# -gt 0 ]] || die_early "--port 需要端口号"
      PREFERRED_PORT="$1"
      shift
      ;;
    --port=*)
      PREFERRED_PORT="${1#*=}"
      shift
      ;;
    *)
      echo "error: unknown argument: $1 (try --help)" >&2
      exit 1
      ;;
  esac
done

# ── pretty log ────────────────────────────────────────
if [[ -t 1 ]]; then
  C_INFO=$'\033[1;34m'
  C_OK=$'\033[1;32m'
  C_WARN=$'\033[1;33m'
  C_ERR=$'\033[1;31m'
  C_DIM=$'\033[2m'
  C_RST=$'\033[0m'
else
  C_INFO= C_OK= C_WARN= C_ERR= C_DIM= C_RST=
fi

log()  { printf '%s==>%s %s\n' "$C_INFO" "$C_RST" "$*"; }
ok()   { printf '%s  OK%s %s\n' "$C_OK" "$C_RST" "$*"; }
warn() { printf '%s  !!%s %s\n' "$C_WARN" "$C_RST" "$*"; }
die()  { printf '%serror:%s %s\n' "$C_ERR" "$C_RST" "$*" >&2; exit 1; }
dim()  { printf '%s    %s%s\n' "$C_DIM" "$*" "$C_RST"; }

have_cmd() { command -v "$1" >/dev/null 2>&1; }

# ── 0. banner ─────────────────────────────────────────
log "ModelHub dev bootstrap"
dim "root: $ROOT"

# ── 1. base tools ─────────────────────────────────────
log "Check toolchain"

have_cmd node || die "未找到 node。请安装 Node.js 18+：https://nodejs.org/"
have_cmd pnpm || die "未找到 pnpm。可执行: corepack enable && corepack prepare pnpm@latest --activate"
have_cmd cargo || die "未找到 cargo/rustc。请安装 Rust：https://rustup.rs/"

NODE_V="$(node -v 2>/dev/null || true)"
PNPM_V="$(pnpm -v 2>/dev/null || true)"
RUST_V="$(rustc --version 2>/dev/null || true)"
ok "node $NODE_V · pnpm $PNPM_V"
ok "$RUST_V"

# ── 2. frontend deps ──────────────────────────────────
need_install=0
if [[ ! -d "$ROOT/node_modules" ]]; then
  need_install=1
elif [[ ! -d "$ROOT/node_modules/@tauri-apps/cli" ]]; then
  need_install=1
elif [[ ! -x "$ROOT/node_modules/.bin/tauri" && ! -f "$ROOT/node_modules/.bin/tauri.cmd" ]]; then
  need_install=1
elif [[ ! -x "$ROOT/node_modules/.bin/vite" && ! -f "$ROOT/node_modules/.bin/vite.cmd" ]]; then
  need_install=1
fi

if (( need_install == 1 )); then
  log "Install frontend deps (pnpm install)"
  if [[ -f "$ROOT/pnpm-lock.yaml" ]]; then
    # 优先锁文件；失败再回退普通 install（避免本地半残 node_modules 卡死）
    if ! pnpm install --frozen-lockfile; then
      warn "pnpm install --frozen-lockfile 失败，改试 pnpm install"
      pnpm install
    fi
  else
    pnpm install
  fi
  ok "pnpm install done"
else
  ok "node_modules present"
fi

TAURI_V="$(pnpm exec tauri --version 2>/dev/null || true)"
if [[ -n "$TAURI_V" ]]; then
  ok "tauri $TAURI_V"
else
  die "Tauri CLI 不可用（pnpm exec tauri --version 失败）"
fi

# ── 3. free dev port（ensure-dev-port.mjs；不杀进程）──
log "Pick dev port"
PICKER="$ROOT/scripts/ensure-dev-port.mjs"
[[ -f "$PICKER" ]] || die "缺少 $PICKER"

ERR_FILE="$(mktemp)"
set +e
DEV_PORT="$(node "$PICKER" "$PREFERRED_PORT" 2>"$ERR_FILE")"
pick_code=$?
set -e
if [[ -s "$ERR_FILE" ]]; then
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ -n "$line" ]] && dim "$line"
  done < "$ERR_FILE"
fi
rm -f "$ERR_FILE"

if [[ $pick_code -ne 0 ]] || ! [[ "$DEV_PORT" =~ ^[0-9]+$ ]]; then
  die "ensure-dev-port.mjs 失败（exit $pick_code, out='$DEV_PORT'）"
fi

if [[ "$DEV_PORT" != "$PREFERRED_PORT" ]]; then
  warn "端口 $PREFERRED_PORT 已被占用（保留对方进程）→ 改用 $DEV_PORT"
else
  ok "frontend port: $DEV_PORT"
fi

HMR_PORT=$((DEV_PORT + 1))
if [[ -n "${TAURI_DEV_HOST:-}" ]]; then
  ok "HMR port: $HMR_PORT (TAURI_DEV_HOST=$TAURI_DEV_HOST)"
fi
export MODELHUB_DEV_PORT="$DEV_PORT"
export MODELHUB_HMR_PORT="$HMR_PORT"
# 与部分工具习惯对齐；避免外部残留 TAURI_DEV_PORT 盖掉自动选择
export TAURI_DEV_PORT="$DEV_PORT"

if [[ "$ONLY_PREPARE" == "1" ]]; then
  ok "prepare/check only — skip pnpm tauri dev"
  dim "would use devUrl http://localhost:${DEV_PORT}"
  exit 0
fi

# 覆盖 tauri.conf.json 的 devUrl + beforeDevCommand（显式 --port，防子进程丢 env）
TAURI_DEV_CONFIG_PATH="${TMPDIR:-/tmp}/modelhub-tauri-dev-${DEV_PORT}.json"
cat > "$TAURI_DEV_CONFIG_PATH" <<EOF
{"build":{"devUrl":"http://localhost:${DEV_PORT}","beforeDevCommand":"pnpm exec vite --port ${DEV_PORT} --strictPort"}}
EOF

echo
log "Start: pnpm tauri dev (port $DEV_PORT)"
dim "devUrl: http://localhost:${DEV_PORT}"
dim "MODELHUB_DEV_PORT=$MODELHUB_DEV_PORT MODELHUB_HMR_PORT=$MODELHUB_HMR_PORT"
dim "config: $TAURI_DEV_CONFIG_PATH"
dim "Ctrl+C to stop"
echo
exec pnpm tauri dev --config "$TAURI_DEV_CONFIG_PATH"
