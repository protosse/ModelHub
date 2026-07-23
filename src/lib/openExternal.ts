import { openUrl } from "@tauri-apps/plugin-opener";

/** Ensure scheme so the system browser can open the address. */
export function toBrowserUrl(raw: string): string | null {
  const s = raw.trim();
  if (!s) return null;
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(s)) return s;
  // bare host/path → assume https
  return `https://${s}`;
}

/** Open URL in the OS default browser (Tauri opener plugin). */
export async function openExternalUrl(raw: string): Promise<void> {
  const url = toBrowserUrl(raw);
  if (!url) throw new Error("链接为空");
  await openUrl(url);
}
