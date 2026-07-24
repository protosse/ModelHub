import type { Protocol } from "../types";

/**
 * Default client identity headers for connectivity tests.
 * Many third-party gateways only allow Claude Code / OpenAI-compatible clients.
 */
export function defaultTestHeaders(protocol: Protocol): Record<string, string> {
  if (protocol === "anthropic-messages") {
    return {
      "User-Agent": "claude-cli/2.1.79",
      "x-app": "cli",
    };
  }
  // openai-completions / openai-responses
  return {
    "User-Agent": "openai-node",
  };
}

export function headersToText(headers: Readonly<Record<string, string>>): string {
  return Object.entries(headers)
    .map(([k, v]) => `${k}: ${v}`)
    .join("\n");
}

/** Parse `Key: Value` lines; blank lines and `#` comments ignored. */
export function parseHeadersText(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    const idx = line.indexOf(":");
    if (idx <= 0) continue;
    const key = line.slice(0, idx).trim();
    const value = line.slice(idx + 1).trim();
    if (!key) continue;
    out[key] = value;
  }
  return out;
}

export function defaultTestHeadersText(protocol: Protocol): string {
  return headersToText(defaultTestHeaders(protocol));
}

/** Multi-provider: show both defaults as a starting point (user can edit). */
export function multiDefaultTestHeadersText(): string {
  return [
    "# Anthropic / Claude Code style (for anthropic-messages providers)",
    "User-Agent: claude-cli/2.1.79",
    "x-app: cli",
    "",
    "# Note: same headers are applied to every request in this run.",
    "# For OpenAI-only gateways you may prefer:",
    "# User-Agent: openai-node",
  ].join("\n");
}
