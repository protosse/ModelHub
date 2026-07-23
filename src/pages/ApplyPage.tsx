import { useEffect, useState } from "react";
import type { AgentBindings, ApplyAgentResult, FullState } from "../types";
import * as api from "../api/tauri";
import type { ApplyPreview } from "../api/tauri";

const AGENTS = [
  { id: "claude", label: "Claude Code" },
  { id: "codex", label: "Codex" },
  { id: "opencode", label: "OpenCode" },
  { id: "pi", label: "Pi" },
] as const;

type Props = {
  readonly state: FullState;
  readonly draft: AgentBindings;
  readonly onToast: (msg: string) => void;
};

export function ApplyPage({ state, draft, onToast }: Props) {
  const [selected, setSelected] = useState<string[]>(AGENTS.map((a) => a.id));
  const [busy, setBusy] = useState(false);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [results, setResults] = useState<readonly ApplyAgentResult[] | null>(null);
  const [preview, setPreview] = useState<ApplyPreview | null>(null);

  const toggle = (id: string) => {
    setSelected((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
  };

  const loadPreview = async (agents: readonly string[]) => {
    if (agents.length === 0) {
      setPreview({ agents: [] });
      return;
    }
    setPreviewBusy(true);
    try {
      const p = await api.previewApply(agents, draft);
      setPreview(p);
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
    } finally {
      setPreviewBusy(false);
    }
  };

  useEffect(() => {
    void loadPreview(selected);
  }, [selected, draft, state.store.providers, state.store.models]);

  const run = async () => {
    setBusy(true);
    setResults(null);
    try {
      const res = await api.applyConfig(selected, draft);
      setResults(res.results);
      const failed = res.results.filter((r) => !r.ok).length;
      onToast(failed ? `应用完成，${failed} 个失败` : "应用成功");
      await loadPreview(selected);
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const b = draft;
  const providerName = (id: string | null) =>
    state.store.providers.find((p) => p.id === id)?.name ?? "—";
  const modelName = (id: string | null) => {
    const m = state.store.models.find((x) => x.id === id);
    return m ? `${m.displayName} (${m.modelId})` : "—";
  };

  return (
    <div className="mx-auto max-w-3xl space-y-4">
      <div className="card p-4">
        <h3 className="mb-3 font-semibold">目标绑定（本次会话草稿）</h3>
        <p className="mb-3 text-xs text-ink-3">
          来自「Agent 绑定」页内存草稿，不落盘。若未设置过草稿，请先到 Agent 绑定读取磁盘并保存。
        </p>
        <ul className="space-y-2 text-sm">
          <li>
            <span className="text-ink-3">Claude：</span>
            {b.claude.mode === "official"
              ? "官方模式"
              : `${providerName(b.claude.providerId)} / ${modelName(b.claude.modelId)}`}
          </li>
          <li>
            <span className="text-ink-3">Codex：</span>
            {b.codex.mode === "official"
              ? "官方模式"
              : `${providerName(b.codex.providerId)} / ${modelName(b.codex.modelId)}`}
          </li>
          <li>
            <span className="text-ink-3">OpenCode：</span>
            {providerName(b.opencode.providerId)} / {modelName(b.opencode.modelId)}
            <span className="text-ink-3">
              {" "}
              · 全量 {state.store.providers.filter((p) => p.enabled).length} providers
            </span>
          </li>
          <li>
            <span className="text-ink-3">Pi：</span>
            {providerName(b.pi.providerId)} / {modelName(b.pi.modelId)}
            <span className="text-ink-3">
              {" "}
              · 全量 {state.store.providers.filter((p) => p.enabled).length} providers
            </span>
          </li>
        </ul>
      </div>

      <div className="card p-4">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <h3 className="font-semibold">选择要同步的 Agent</h3>
          <div className="flex gap-2">
            <button
              type="button"
              className="btn-secondary !py-1 text-xs"
              onClick={() => setSelected(AGENTS.map((a) => a.id))}
            >
              全选
            </button>
            <button
              type="button"
              className="btn-secondary !py-1 text-xs"
              onClick={() => setSelected([])}
            >
              清空
            </button>
          </div>
        </div>
        <div className="flex flex-wrap gap-3">
          {AGENTS.map((a) => (
            <label
              key={a.id}
              className="flex cursor-pointer items-center gap-2 rounded-md border border-surface-3 bg-surface-0 px-3 py-2 text-sm"
            >
              <input
                type="checkbox"
                checked={selected.includes(a.id)}
                onChange={() => toggle(a.id)}
              />
              {a.label}
            </label>
          ))}
        </div>
        <div className="mt-4 flex flex-wrap gap-2">
          <button
            type="button"
            className="btn-secondary"
            disabled={previewBusy || selected.length === 0}
            onClick={() => void loadPreview(selected)}
          >
            {previewBusy ? "对比中…" : "刷新对比"}
          </button>
          <button
            type="button"
            className="btn-primary min-w-[8rem]"
            disabled={busy || selected.length === 0}
            onClick={() => void run()}
          >
            {busy
              ? "应用中…"
              : selected.length > 0
                ? `确认应用 (${selected.length})`
                : "确认应用"}
          </button>
        </div>
      </div>

      <div className="card p-4">
        <h3 className="mb-3 font-semibold">更改对比（磁盘现状 → Apply 后）</h3>
        {previewBusy && !preview ? (
          <p className="text-sm text-ink-3">加载对比…</p>
        ) : !preview || preview.agents.length === 0 ? (
          <p className="text-sm text-ink-3">请选择至少一个 Agent</p>
        ) : (
          <div className="space-y-4">
            {preview.agents.map((a) => (
              <div key={a.agent} className="rounded-lg border border-surface-3 bg-surface-0 p-3">
                <div className="mb-1 flex items-center justify-between gap-2">
                  <span className="font-medium capitalize">{a.agent}</span>
                  <span className="font-mono text-[11px] text-ink-3 truncate max-w-[60%]">
                    {a.file}
                  </span>
                </div>
                {a.note ? <p className="mb-2 text-xs text-ink-3">{a.note}</p> : null}
                <ul className="space-y-1 font-mono text-xs">
                  {a.lines.map((line, i) => (
                    <li
                      key={`${a.agent}-${i}`}
                      className={
                        line.kind === "add"
                          ? "text-ok"
                          : line.kind === "remove"
                            ? "text-danger"
                            : line.kind === "change"
                              ? "text-warn"
                              : "text-ink-3"
                      }
                    >
                      {line.text}
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </div>
        )}
      </div>

      {results ? (
        <div className="space-y-2">
          <h3 className="font-semibold">应用结果</h3>
          {results.map((r) => (
            <div
              key={r.agent}
              className={r.ok ? "card border-ok/30 p-4" : "card border-danger/40 p-4"}
            >
              <div className="flex items-center justify-between">
                <span className="font-medium">{r.agent}</span>
                <span className={r.ok ? "text-ok text-sm" : "text-danger text-sm"}>
                  {r.ok ? "成功" : "失败"}
                </span>
              </div>
              <p className="mt-2 text-sm text-ink-2">{r.message}</p>
              {r.files.length > 0 ? (
                <ul className="mt-2 font-mono text-[11px] text-ink-3">
                  {r.files.map((f) => (
                    <li key={f}>{f}</li>
                  ))}
                </ul>
              ) : null}
              {r.restartRequired ? (
                <div className="mt-3 rounded-md border border-warn/40 bg-warn/10 px-3 py-2 text-xs text-warn">
                  建议重启该 Agent / 新开会话后再使用。
                </div>
              ) : null}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}
