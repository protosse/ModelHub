#!/usr/bin/env node
/**
 * Pick a free frontend port for ModelHub Tauri dev.
 * Does NOT kill other processes — if preferred port is busy, try next.
 * Probes both 127.0.0.1 and ::1 so IPv6-only listeners (common on Windows Vite) are detected.
 *
 * Prints ONLY the chosen port to stdout (for shell capture).
 * Diagnostics go to stderr.
 *
 * Usage:
 *   node scripts/ensure-dev-port.mjs
 *   node scripts/ensure-dev-port.mjs 1431
 * Env:
 *   MODELHUB_DEV_PORT / TAURI_DEV_PORT  preferred (default 1420)
 *   TAURI_DEV_HOST                         if set, also require port+1 free (HMR)
 */
import net from "node:net";
import process from "node:process";

const preferred = Number(
  process.argv[2] ||
    process.env.MODELHUB_DEV_PORT ||
    process.env.TAURI_DEV_PORT ||
    1420,
);
const maxTries = Number(process.env.MODELHUB_PORT_TRIES || 40);
const needHmr = Boolean(process.env.TAURI_DEV_HOST);

function log(msg) {
  console.error(`[dev-port] ${msg}`);
}

/**
 * True if anything is accepting TCP on this port on either loopback stack.
 * Windows Vite often listens on [::1] only; probing 127.0.0.1 alone misses it.
 */
function probeHost(host, port) {
  return new Promise((resolve) => {
    const socket = net.connect({ host, port }, () => {
      socket.destroy();
      resolve(true);
    });
    socket.setTimeout(200);
    socket.on("timeout", () => {
      socket.destroy();
      resolve(false);
    });
    socket.on("error", () => {
      socket.destroy();
      resolve(false);
    });
  });
}

async function portInUse(port) {
  if (await probeHost("127.0.0.1", port)) return true;
  if (await probeHost("::1", port)) return true;
  return false;
}

async function pickPort(start) {
  if (!Number.isFinite(start) || start < 1 || start > 65535) {
    throw new Error(`invalid preferred port: ${start}`);
  }
  let p = Math.floor(start);
  for (let n = 0; n < maxTries; n++) {
    if (p > 65534) break;
    const busy = await portInUse(p);
    if (!busy) {
      if (!needHmr || !(await portInUse(p + 1))) {
        return p;
      }
    } else if (n === 0) {
      log(`:${p} in use — will try next free port (not killing owner)`);
    }
    p += 1;
  }
  throw new Error(
    `no free port in ${start}..${start + maxTries - 1}` +
      (needHmr ? " (need port+1 for HMR)" : ""),
  );
}

const port = await pickPort(preferred);
if (port !== Math.floor(preferred)) {
  log(`using :${port} (preferred :${Math.floor(preferred)} busy)`);
} else {
  log(`using :${port}`);
}
process.stdout.write(String(port));
