#!/usr/bin/env node
/**
 * Cross-platform launcher for ModelHub dev bootstrap.
 * Windows → scripts/dev.ps1
 * macOS / Linux → scripts/dev.sh
 *
 * Usage (from repo root, via package.json):
 *   pnpm dev:tauri
 *   pnpm dev:tauri -- --port 1431
 *   pnpm dev:check
 *   scripts\\dev.cmd   (Windows CMD)
 */
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
const args = process.argv.slice(2);
const isWin = process.platform === "win32";

/**
 * @param {string} cmd
 * @param {string[]} argv
 * @returns {Promise<number>}
 */
function run(cmd, argv) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, argv, {
      cwd: root,
      stdio: "inherit",
      env: process.env,
      shell: false,
      windowsHide: true,
    });
    child.on("error", reject);
    child.on("exit", (code, signal) => {
      if (signal) resolve(1);
      else resolve(code ?? 1);
    });
  });
}

async function main() {
  if (isWin) {
    const ps1 = path.join(__dirname, "dev.ps1");
    const shells = [
      { cmd: "pwsh", argv: ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", ps1, ...args] },
      {
        cmd: "powershell.exe",
        argv: ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", ps1, ...args],
      },
    ];

    let lastErr = null;
    for (const shell of shells) {
      try {
        const code = await run(shell.cmd, shell.argv);
        process.exit(code);
      } catch (err) {
        lastErr = err;
        // ENOENT → try next shell
        continue;
      }
    }
    console.error(
      "error: 未找到 PowerShell（pwsh 或 powershell.exe）。请安装 PowerShell 或手动运行 scripts\\dev.ps1",
    );
    if (lastErr) console.error(`  (${lastErr.message})`);
    process.exit(1);
  }

  const sh = path.join(__dirname, "dev.sh");
  try {
    const code = await run("bash", [sh, ...args]);
    process.exit(code);
  } catch (err) {
    console.error(`error: failed to start bash scripts/dev.sh: ${err.message}`);
    process.exit(1);
  }
}

main();
