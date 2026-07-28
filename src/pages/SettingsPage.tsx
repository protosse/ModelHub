import { useEffect, useMemo, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type { AppConfig, FullState } from "../types";
import * as api from "../api/tauri";

type Props = {
  readonly state: FullState;
  readonly onRefresh: () => Promise<void>;
  readonly onToast: (msg: string) => void;
};

const APP_VERSION = "0.1.0";
const KEEP_MIN = 1;
const KEEP_MAX = 50;

async function copyText(text: string): Promise<void> {
  await navigator.clipboard.writeText(text);
}

function clampKeep(n: number): number {
  if (!Number.isFinite(n)) return 10;
  return Math.min(KEEP_MAX, Math.max(KEEP_MIN, Math.round(n)));
}

type PathRow = {
  readonly id: string;
  readonly label: string;
  readonly path: string;
  readonly exists: boolean;
};

export function SettingsPage({ state, onRefresh, onToast }: Props) {
  const c = state.config;
  const p = state.paths;
  const [keepDraft, setKeepDraft] = useState(String(c.backupKeepCount));
  const [saving, setSaving] = useState(false);

  // External refresh (e.g. restore elsewhere) should sync the input.
  useEffect(() => {
    setKeepDraft(String(c.backupKeepCount));
  }, [c.backupKeepCount]);

  const paths = useMemo<readonly PathRow[]>(
    () => [
      {
        id: "claude",
        label: "Claude",
        path: p.claudeSettings,
        exists: p.claudeExists,
      },
      {
        id: "codex",
        label: "Codex",
        path: p.codexConfig,
        exists: p.codexExists,
      },
      {
        id: "opencode",
        label: "OpenCode",
        path: p.opencodeConfig,
        exists: p.opencodeExists,
      },
      {
        id: "pi",
        label: "Pi",
        path: p.piModels,
        exists: p.piExists,
      },
    ],
    [p],
  );

  const keepDirty =
    clampKeep(Number(keepDraft)) !== c.backupKeepCount ||
    keepDraft.trim() !== String(c.backupKeepCount);

  const copy = async (text: string, okMsg: string) => {
    try {
      await copyText(text);
      onToast(okMsg);
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
    }
  };

  const openPath = async (path: string) => {
    try {
      await revealItemInDir(path);
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
    }
  };

  const saveKeep = async () => {
    const next = clampKeep(Number(keepDraft));
    setKeepDraft(String(next));
    if (next === c.backupKeepCount) {
      onToast("备份保留份数未变化");
      return;
    }
    setSaving(true);
    try {
      const config: AppConfig = {
        ...c,
        backupKeepCount: next,
      };
      await api.saveAppConfig(config);
      await onRefresh();
      onToast(`备份保留份数已设为 ${next}（每个 Agent 独立）`);
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
      setKeepDraft(String(c.backupKeepCount));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="mx-auto max-w-2xl space-y-4 p-1">
      {/* 应用 */}
      <section className="card space-y-4 p-4">
        <header>
          <h3 className="text-sm font-semibold text-ink-1">应用</h3>
          <p className="mt-0.5 text-xs text-ink-3">
            写入 <span className="font-mono">~/.modelhub/config.json</span>
          </p>
        </header>

        <div className="grid gap-3 sm:grid-cols-[7.5rem_1fr] sm:items-center">
          <div className="text-xs text-ink-3">语言</div>
          <div className="flex flex-wrap items-center gap-2 text-sm">
            <span className="rounded-md border border-surface-3 bg-surface-0 px-2.5 py-1.5 text-ink-1">
              简体中文
            </span>
            <span className="text-xs text-ink-3">目前仅中文</span>
          </div>

          <div className="text-xs text-ink-3">备份保留份数</div>
          <div className="flex flex-wrap items-center gap-2">
            <input
              type="number"
              min={KEEP_MIN}
              max={KEEP_MAX}
              className="input w-24"
              value={keepDraft}
              disabled={saving}
              onChange={(e) => setKeepDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void saveKeep();
                }
              }}
              onBlur={() => {
                const n = clampKeep(Number(keepDraft));
                if (String(n) !== keepDraft.trim()) setKeepDraft(String(n));
              }}
              aria-label="备份保留份数"
            />
            <button
              type="button"
              className="btn-primary"
              disabled={saving || !keepDirty}
              onClick={() => void saveKeep()}
            >
              {saving ? "保存中…" : "保存"}
            </button>
            <span className="text-xs text-ink-3">
              每个 Agent 保留最近 {KEEP_MIN}–{KEEP_MAX} 组快照（当前{" "}
              {c.backupKeepCount}）
            </span>
          </div>

          <div className="text-xs text-ink-3">数据目录</div>
          <div className="min-w-0">
            <PathActions
              path={p.modelhubDir}
              onCopy={() => void copy(p.modelhubDir, "已复制数据目录")}
              onOpen={() => void openPath(p.modelhubDir)}
            />
          </div>
        </div>
      </section>

      {/* Agent 路径 */}
      <section className="card space-y-3 p-4">
        <header>
          <h3 className="text-sm font-semibold text-ink-1">Agent 路径</h3>
          <p className="mt-0.5 text-xs text-ink-3">
            当前检测结果（只读）。路径覆盖编辑后续再支持。
          </p>
        </header>

        <ul className="divide-y divide-surface-3 overflow-hidden rounded-lg border border-surface-3">
          {paths.map((row) => (
            <li
              key={row.id}
              className="flex flex-col gap-2 bg-surface-0/40 px-3 py-2.5 sm:flex-row sm:items-center"
            >
              <div className="flex w-28 shrink-0 items-center gap-2">
                <span
                  className={`h-1.5 w-1.5 rounded-full ${
                    row.exists ? "bg-emerald-400" : "bg-ink-3"
                  }`}
                  title={row.exists ? "已找到" : "未找到"}
                />
                <span className="text-sm font-medium text-ink-1">{row.label}</span>
              </div>
              <div className="min-w-0 flex-1">
                <PathActions
                  path={row.path}
                  onCopy={() => void copy(row.path, `已复制 ${row.label} 路径`)}
                  onOpen={() => void openPath(row.path)}
                  exists={row.exists}
                />
              </div>
            </li>
          ))}
        </ul>
      </section>

      {/* 关于 */}
      <section className="card space-y-1 p-4 text-xs text-ink-3">
        <div className="text-sm font-semibold text-ink-1">关于</div>
        <p>
          ModelHub <span className="font-mono text-ink-2">v{APP_VERSION}</span>
          {" · "}
          只管理模型相关配置，不代理请求、不改 Agent 二进制。
        </p>
      </section>
    </div>
  );
}

function PathActions({
  path,
  onCopy,
  onOpen,
  exists,
}: {
  readonly path: string;
  readonly onCopy: () => void;
  readonly onOpen: () => void;
  readonly exists?: boolean;
}) {
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-1.5">
      <code
        className={`min-w-0 flex-1 break-all rounded-md border border-surface-3 bg-surface-0 px-2 py-1 font-mono text-[11px] leading-relaxed ${
          exists === false ? "text-ink-3" : "text-ink-2"
        }`}
        title={path}
      >
        {path}
      </code>
      <button type="button" className="btn-ghost shrink-0 px-2 py-1 text-xs" onClick={onCopy}>
        复制
      </button>
      <button type="button" className="btn-ghost shrink-0 px-2 py-1 text-xs" onClick={onOpen}>
        打开
      </button>
    </div>
  );
}
