import type { BackupEntry } from "../types";

export type BackupAgentId = "claude" | "codex" | "opencode" | "pi" | string;

export type BackupFile = {
  readonly fileName: string;
  readonly path: string;
};

export type BackupSnapshot = {
  readonly id: string;
  readonly agent: BackupAgentId;
  readonly stamp: string;
  readonly dirPath: string;
  readonly files: readonly BackupFile[];
};

export const BACKUP_AGENT_FILTERS = [
  { id: "all", label: "全部" },
  { id: "claude", label: "Claude Code" },
  { id: "codex", label: "Codex" },
  { id: "opencode", label: "OpenCode" },
  { id: "pi", label: "Pi" },
] as const;

export type BackupAgentFilter = (typeof BACKUP_AGENT_FILTERS)[number]["id"];

const AGENT_LABELS: Readonly<Record<string, string>> = {
  claude: "Claude Code",
  codex: "Codex",
  opencode: "OpenCode",
  pi: "Pi",
};

export function agentLabel(agent: string): string {
  return AGENT_LABELS[agent] ?? agent;
}

/** Parent directory of a backup file path (the stamp folder). */
export function snapshotDirFromPath(filePath: string): string {
  const normalized = filePath.replace(/\\/g, "/");
  const idx = normalized.lastIndexOf("/");
  if (idx <= 0) return filePath;
  return filePath.slice(0, idx);
}

/**
 * Group flat file rows into one snapshot per agent+stamp.
 * Input is expected newest-first; output preserves that order.
 */
export function groupBackupSnapshots(
  entries: readonly BackupEntry[],
): readonly BackupSnapshot[] {
  const order: string[] = [];
  const map = new Map<string, BackupSnapshot>();

  for (const entry of entries) {
    const id = `${entry.agent}::${entry.stamp}`;
    const existing = map.get(id);
    const file: BackupFile = { fileName: entry.fileName, path: entry.path };
    if (existing) {
      map.set(id, {
        ...existing,
        files: [...existing.files, file].sort((a, b) =>
          a.fileName.localeCompare(b.fileName),
        ),
      });
      continue;
    }
    order.push(id);
    map.set(id, {
      id,
      agent: entry.agent,
      stamp: entry.stamp,
      dirPath: snapshotDirFromPath(entry.path),
      files: [file],
    });
  }

  return order.map((id) => map.get(id)!);
}

/**
 * Parse backup stamp to a Date.
 * Supports:
 * - YYYYMMDD-HHMMSS
 * - YYYYMMDD-HHMMSS-mmm  (ms, current writer)
 * Stamps are UTC.
 */
export function parseBackupStamp(stamp: string): Date | null {
  const m = /^(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})(?:-(\d{1,3}))?$/.exec(
    stamp,
  );
  if (!m) return null;
  const [, y, mo, d, h, mi, s, frac] = m;
  const ms = frac ? Number(frac.padEnd(3, "0").slice(0, 3)) : 0;
  const date = new Date(
    Date.UTC(
      Number(y),
      Number(mo) - 1,
      Number(d),
      Number(h),
      Number(mi),
      Number(s),
      ms,
    ),
  );
  if (
    Number.isNaN(date.getTime()) ||
    date.getUTCFullYear() !== Number(y) ||
    date.getUTCMonth() !== Number(mo) - 1 ||
    date.getUTCDate() !== Number(d) ||
    date.getUTCHours() !== Number(h) ||
    date.getUTCMinutes() !== Number(mi) ||
    date.getUTCSeconds() !== Number(s)
  ) {
    return null;
  }
  return date;
}

export function formatBackupStamp(stamp: string): string {
  const date = parseBackupStamp(stamp);
  if (!date) return stamp;
  const y = date.getFullYear();
  const mo = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  const h = String(date.getHours()).padStart(2, "0");
  const mi = String(date.getMinutes()).padStart(2, "0");
  const s = String(date.getSeconds()).padStart(2, "0");
  return `${y}-${mo}-${d} ${h}:${mi}:${s}`;
}

export function formatRelativeStamp(stamp: string, now = new Date()): string | null {
  const date = parseBackupStamp(stamp);
  if (!date) return null;
  const diffMs = now.getTime() - date.getTime();
  if (diffMs < 0) return null;
  const sec = Math.floor(diffMs / 1000);
  if (sec < 60) return "刚刚";
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min} 分钟前`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr} 小时前`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day} 天前`;
  return null;
}

export function latestStampByAgent(
  snapshots: readonly BackupSnapshot[],
): Readonly<Record<string, string>> {
  const out: Record<string, string> = {};
  for (const s of snapshots) {
    if (!out[s.agent] || s.stamp > out[s.agent]!) {
      out[s.agent] = s.stamp;
    }
  }
  return out;
}
