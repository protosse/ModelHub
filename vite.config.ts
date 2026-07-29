import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Dev launcher picks a free port via scripts/ensure-dev-port.mjs when 1420 is busy.
const host = process.env.TAURI_DEV_HOST;
const devPort = Number(process.env.MODELHUB_DEV_PORT ?? 1420);
const hmrPort = Number(process.env.MODELHUB_HMR_PORT ?? devPort + 1);

export default defineConfig(async () => ({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: devPort,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: hmrPort,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
