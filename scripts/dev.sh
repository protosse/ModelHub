#!/usr/bin/env bash
# ModelHub 开发启动脚本
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
  3. 选择空闲前端端口（默认 1420；占用则 1421…）
  4. pnpm tauri dev（同步 Vite MODELHUB_DEV_PORT + tauri --config devUrl）

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

# TCP 端口是否已被监听
port_in_use() {
  local port="$1"
  if have_cmd lsof; then
    lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1 && return 0
    return 1
  fi
  if have_cmd nc; then
    nc -z 127.0.0.1 "$port" >/dev/null 2>&1 && return 0
    return 1
  fi
  if (echo >/dev/tcp/127.0.0.1/"$port") >/dev/null 2>&1; then
    return 0
  fi
  return 1
}

port_owner() {
  local port="$1"
  if have_cmd lsof; then
    lsof -nP -iTCP:"$port" -sTCP:LISTEN 2>/dev/null || true
  fi
}

# 从首选端口起顺延；若启用 TAURI_DEV_HOST 则同时要求 port+1（HMR）空闲
pick_dev_port() {
  local start="$1"
  local max_tries="${2:-40}"
  local need_hmr=0
  if [[ -n "${TAURI_DEV_HOST:-}" ]]; then
    need_hmr=1
  fi

  if ! [[ "$start" =~ ^[0-9]+$ ]] || (( start < 1 || start > 65535 )); then
    die "无效端口：$start（期望 1–65535）"
  fi

  local p="$start"
  local i=0
  while (( i < max_tries )); do
    if (( p > 65534 )); then
      break
    fi
    if ! port_in_use "$p"; then
      if (( need_hmr == 0 )) || ! port_in_use "$((p + 1))"; then
        echo "$p"
        return 0
      fi
    fi
    p=$((p + 1))
    i=$((i + 1))
  done
  die "在 $start 起连续 ${max_tries} 个端口均不可用（含 HMR 需求时还需 port+1）"
}

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

# ── 3. free dev port ──────────────────────────────────
log "Pick dev port"
if port_in_use "$PREFERRED_PORT"; then
  owner="$(port_owner "$PREFERRED_PORT")"
  if [[ -n "$owner" ]]; then
    warn "端口 $PREFERRED_PORT 已被占用："
    # shellcheck disable=SC2001
    echo "$owner" | sed 's/^/    /' >&2 || true
  else
    warn "端口 $PREFERRED_PORT 已被占用"
  fi
fi

DEV_PORT="$(pick_dev_port "$PREFERRED_PORT")"
if [[ "$DEV_PORT" != "$PREFERRED_PORT" ]]; then
  warn "改用端口 $DEV_PORT"
else
  ok "frontend port: $DEV_PORT"
fi

HMR_PORT=$((DEV_PORT + 1))
if [[ -n "${TAURI_DEV_HOST:-}" ]]; then
  # pick_dev_port 已保证 port+1 空闲；仍显式导出给 vite
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

# tauri.conf.json 里 devUrl 写死 1420，用 --config 覆盖为当前端口
TAURI_DEV_CONFIG="{\"build\":{\"devUrl\":\"http://localhost:${DEV_PORT}\"}}"

echo
log "Start: pnpm tauri dev (port $DEV_PORT)"
dim "devUrl: http://localhost:${DEV_PORT}"
dim "MODELHUB_DEV_PORT=$MODELHUB_DEV_PORT MODELHUB_HMR_PORT=$MODELHUB_HMR_PORT"
dim "Ctrl+C to stop"
echo
exec pnpm tauri dev --config "$TAURI_DEV_CONFIG"
