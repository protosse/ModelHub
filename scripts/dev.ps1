# ModelHub 开发启动脚本（Windows / PowerShell）
#
# 1. 检查基础工具（node / pnpm / cargo）
# 2. 必要时 pnpm install
# 3. 选择空闲前端端口（默认 1420，占用则顺延）
# 4. 同步 Vite + Tauri devUrl 后启动 pnpm tauri dev
#
# 用法（仓库根目录）：
#   pwsh -File scripts/dev.ps1
#   powershell -ExecutionPolicy Bypass -File scripts/dev.ps1
#   .\scripts\dev.ps1 --only-prepare
#   .\scripts\dev.ps1 --check
#   .\scripts\dev.ps1 --port 1431
#   pnpm run dev:tauri
#   scripts\dev.cmd
#
# 环境变量：
#   MODELHUB_DEV_PORT / TAURI_DEV_PORT   首选前端端口（默认 1420）
#   TAURI_DEV_HOST                       非空时额外要求 HMR 端口 (port+1) 空闲
#   ONLY_PREPARE=1                       同 --only-prepare
#Requires -Version 5.1
$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $Root

$OnlyPrepare = if ($env:ONLY_PREPARE -eq "1") { $true } else { $false }
$PreferredPort = 1420
if ($env:MODELHUB_DEV_PORT) {
  $PreferredPort = $env:MODELHUB_DEV_PORT
} elseif ($env:TAURI_DEV_PORT) {
  $PreferredPort = $env:TAURI_DEV_PORT
}

function Show-Usage {
  @"
ModelHub 开发启动脚本（Windows）

用法：
  pwsh -File scripts/dev.ps1
  powershell -ExecutionPolicy Bypass -File scripts/dev.ps1
  .\scripts\dev.ps1 --only-prepare   # 只检查工具链与依赖，不启动
  .\scripts\dev.ps1 --check          # 同 --only-prepare
  .\scripts\dev.ps1 --port 1431      # 指定首选前端端口（占用则自动顺延）
  pnpm run dev:tauri                 # 跨平台入口（推荐）
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
"@ | Write-Host
  exit 0
}

# ── args ──────────────────────────────────────────────
$i = 0
while ($i -lt $args.Count) {
  $a = [string]$args[$i]
  switch -Regex ($a) {
    '^(-h|--help)$' {
      Show-Usage
    }
    '^(--only-prepare|--check)$' {
      $OnlyPrepare = $true
      $i++
    }
    '^--port$' {
      $i++
      if ($i -ge $args.Count) { throw "--port 需要端口号" }
      $PreferredPort = [string]$args[$i]
      $i++
    }
    '^--port=(.+)$' {
      $PreferredPort = $Matches[1]
      $i++
    }
    default {
      throw "unknown argument: $a (try --help)"
    }
  }
}

# ── pretty log ────────────────────────────────────────
$script:UseColor = $Host.UI.SupportsVirtualTerminal -or ($env:WT_SESSION -ne $null) -or ($env:TERM_PROGRAM -ne $null)

function Write-Log([string]$Message) {
  if ($script:UseColor) {
    Write-Host "==> " -ForegroundColor Blue -NoNewline
    Write-Host $Message
  } else {
    Write-Host "==> $Message"
  }
}
function Write-Ok([string]$Message) {
  if ($script:UseColor) {
    Write-Host "  OK " -ForegroundColor Green -NoNewline
    Write-Host $Message
  } else {
    Write-Host "  OK $Message"
  }
}
function Write-WarnMsg([string]$Message) {
  if ($script:UseColor) {
    Write-Host "  !! " -ForegroundColor Yellow -NoNewline
    Write-Host $Message
  } else {
    Write-Host "  !! $Message"
  }
}
function Write-Dim([string]$Message) {
  if ($script:UseColor) {
    Write-Host "    $Message" -ForegroundColor DarkGray
  } else {
    Write-Host "    $Message"
  }
}
function Die([string]$Message) {
  if ($script:UseColor) {
    Write-Host "error: " -ForegroundColor Red -NoNewline
    Write-Host $Message
  } else {
    Write-Host "error: $Message"
  }
  exit 1
}

function Test-Command([string]$Name) {
  return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Get-PortOwner([int]$Port) {
  try {
    $conns = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
    if (-not $conns) { return $null }
    $lines = foreach ($c in $conns) {
      $proc = Get-Process -Id $c.OwningProcess -ErrorAction SilentlyContinue
      $pname = if ($proc) { $proc.ProcessName } else { "?" }
      "PID $($c.OwningProcess) ($pname)  $($c.LocalAddress):$($c.LocalPort)"
    }
    return ($lines -join "`n")
  } catch {
    return $null
  }
}

# 通过 ensure-dev-port.mjs 选端口（双栈探测；stdout=端口，stderr=诊断）
function Select-DevPortViaNode([int]$Start) {
  if ($Start -lt 1 -or $Start -gt 65535) {
    Die "无效端口：$Start（期望 1–65535）"
  }
  $picker = Join-Path $PSScriptRoot "ensure-dev-port.mjs"
  if (-not (Test-Path $picker)) {
    Die "缺少 $picker"
  }

  $stdoutFile = [System.IO.Path]::GetTempFileName()
  $stderrFile = [System.IO.Path]::GetTempFileName()
  try {
    $proc = Start-Process -FilePath "node" -ArgumentList @($picker, "$Start") -WorkingDirectory $Root -NoNewWindow -Wait -PassThru -RedirectStandardOutput $stdoutFile -RedirectStandardError $stderrFile
    $code = $proc.ExitCode
    $outText = (Get-Content -LiteralPath $stdoutFile -Raw -ErrorAction SilentlyContinue)
    $errText = (Get-Content -LiteralPath $stderrFile -Raw -ErrorAction SilentlyContinue)
    if ($errText) {
      foreach ($line in ($errText -split "`r?`n")) {
        if (-not [string]::IsNullOrWhiteSpace($line)) { Write-Dim $line }
      }
    }
    if ($code -ne 0) {
      Die "ensure-dev-port.mjs 失败 (exit $code)"
    }
    $portStr = ([string]$outText).Trim()
    if ($portStr -notmatch '^\d+$') {
      Die "ensure-dev-port.mjs 未返回端口（得到: $portStr）"
    }
    return [int]$portStr
  } finally {
    Remove-Item -LiteralPath $stdoutFile -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $stderrFile -Force -ErrorAction SilentlyContinue
  }
}

# 解析首选端口为 int

try {
  $PreferredPortInt = [int]$PreferredPort
} catch {
  Die "无效端口：$PreferredPort（期望 1–65535）"
}

# ── 0. banner ─────────────────────────────────────────
Write-Log "ModelHub dev bootstrap (Windows)"
Write-Dim "root: $Root"

# ── 1. base tools ─────────────────────────────────────
Write-Log "Check toolchain"

if (-not (Test-Command "node")) {
  Die "未找到 node。请安装 Node.js 18+：https://nodejs.org/"
}
if (-not (Test-Command "pnpm")) {
  Die "未找到 pnpm。可执行: corepack enable && corepack prepare pnpm@latest --activate"
}
if (-not (Test-Command "cargo")) {
  Die "未找到 cargo/rustc。请安装 Rust：https://rustup.rs/"
}

$NodeV = (node -v 2>$null)
$PnpmV = (pnpm -v 2>$null)
$RustV = (rustc --version 2>$null)
Write-Ok "node $NodeV | pnpm $PnpmV"
Write-Ok "$RustV"

# ── 2. frontend deps ──────────────────────────────────
$NeedInstall = $false
$nm = Join-Path $Root "node_modules"
$tauriCli = Join-Path $nm "@tauri-apps\cli"
$tauriBin = Join-Path $nm ".bin\tauri.cmd"
$tauriBinAlt = Join-Path $nm ".bin\tauri"
$viteBin = Join-Path $nm ".bin\vite.cmd"
$viteBinAlt = Join-Path $nm ".bin\vite"

if (-not (Test-Path $nm)) {
  $NeedInstall = $true
} elseif (-not (Test-Path $tauriCli)) {
  $NeedInstall = $true
} elseif (-not (Test-Path $tauriBin) -and -not (Test-Path $tauriBinAlt)) {
  $NeedInstall = $true
} elseif (-not (Test-Path $viteBin) -and -not (Test-Path $viteBinAlt)) {
  $NeedInstall = $true
}

if ($NeedInstall) {
  Write-Log "Install frontend deps (pnpm install)"
  $lock = Join-Path $Root "pnpm-lock.yaml"
  if (Test-Path $lock) {
    $prev = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    pnpm install --frozen-lockfile
    $code = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($code -ne 0) {
      Write-WarnMsg "pnpm install --frozen-lockfile 失败，改试 pnpm install"
      pnpm install
      if ($LASTEXITCODE -ne 0) { Die "pnpm install 失败 (exit $LASTEXITCODE)" }
    }
  } else {
    pnpm install
    if ($LASTEXITCODE -ne 0) { Die "pnpm install 失败 (exit $LASTEXITCODE)" }
  }
  Write-Ok "pnpm install done"
} else {
  Write-Ok "node_modules present"
}

$TauriV = $null
try {
  $TauriV = (pnpm exec tauri --version 2>$null | Out-String).Trim()
} catch {
  $TauriV = $null
}
if ($TauriV) {
  Write-Ok "tauri $TauriV"
} else {
  Die "Tauri CLI 不可用（pnpm exec tauri --version 失败）"
}

# ── 3. free dev port（不杀进程；Node 双栈探测）────────
Write-Log "Pick dev port"
$owner = Get-PortOwner $PreferredPortInt
if ($owner) {
  Write-WarnMsg "端口 $PreferredPortInt 已被占用（保留对方进程，改用下一空闲口）："
  foreach ($line in ($owner -split "`n")) {
    Write-Dim $line
  }
}

$DevPort = Select-DevPortViaNode $PreferredPortInt
if ($DevPort -ne $PreferredPortInt) {
  Write-WarnMsg "改用端口 $DevPort"
} else {
  Write-Ok "frontend port: $DevPort"
}

$HmrPort = $DevPort + 1
if (-not [string]::IsNullOrEmpty($env:TAURI_DEV_HOST)) {
  Write-Ok "HMR port: $HmrPort (TAURI_DEV_HOST=$($env:TAURI_DEV_HOST))"
}
$env:MODELHUB_DEV_PORT = "$DevPort"
$env:MODELHUB_HMR_PORT = "$HmrPort"
$env:TAURI_DEV_PORT = "$DevPort"

if ($OnlyPrepare) {
  Write-Ok "prepare/check only — skip pnpm tauri dev"
  Write-Dim "would use devUrl http://localhost:${DevPort}"
  exit 0
}

# 用临时 JSON 覆盖 tauri.conf.json 的 devUrl + beforeDevCommand。
# 显式 --port，避免 beforeDevCommand 子进程未继承 MODELHUB_DEV_PORT 时仍钉死 1420。
$TauriDevConfigPath = Join-Path $env:TEMP "modelhub-tauri-dev-$DevPort.json"
$ViteCmd = "pnpm exec vite --port $DevPort --strictPort"
$TauriDevConfigObj = @{
  build = @{
    devUrl = "http://localhost:$DevPort"
    beforeDevCommand = $ViteCmd
  }
}
# PS 5.1 无 utf8NoBOM；统一用无 BOM UTF-8，避免 tauri 解析 BOM 失败
$TauriDevConfigJson = $TauriDevConfigObj | ConvertTo-Json -Compress -Depth 5
$Utf8NoBom = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($TauriDevConfigPath, $TauriDevConfigJson, $Utf8NoBom)

Write-Host ""
Write-Log "Start: pnpm tauri dev (port $DevPort)"
Write-Dim "devUrl: http://localhost:${DevPort}"
Write-Dim "MODELHUB_DEV_PORT=$($env:MODELHUB_DEV_PORT) MODELHUB_HMR_PORT=$($env:MODELHUB_HMR_PORT)"
Write-Dim "config: $TauriDevConfigPath"
Write-Dim "Ctrl+C to stop"
Write-Host ""

# 前台启动；不捕获 exit，让用户看到完整输出
pnpm tauri dev --config $TauriDevConfigPath
exit $LASTEXITCODE
