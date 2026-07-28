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
      // Relays that force 1M context (anyrouter etc.) reject without this beta.
      "anthropic-beta": "context-1m-2025-08-07",
    };
  }
  if (protocol === "openai-responses") {
    return {
      "User-Agent": "codex_cli_rs/0.144.4",
    };
  }
  // openai-completions
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

/** Non-comment Key:Value lines from the override textarea. */
export function headerOverrideLines(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("#"));
}

/** Compact summary of protocol-auto defaults for collapsed UI. */
export function protocolAutoHeadersSummary(protocol?: Protocol): string {
  if (protocol === "anthropic-messages") {
    return "User-Agent: claude-cli · x-app: cli · anthropic-beta: context-1m";
  }
  if (protocol === "openai-completions") {
    return "User-Agent: openai-node";
  }
  if (protocol === "openai-responses") {
    return "User-Agent: codex_cli_rs/0.144.4";
  }
  // multi / unknown: all protocol families
  return "anthropic → claude-cli+1m · completions → openai-node · responses → codex_cli_rs";
}
