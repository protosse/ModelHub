import { useEffect, useMemo, useState } from "react";
import type { Model, Provider, TestPrompt } from "../types";
import {
  createMultiTestSession,
  getMultiTestSession,
  requestStopMultiTest,
  startMultiTest,
  subscribeMultiTestSession,
  type MultiRowState,
  type MultiRowStatus,
} from "../lib/multiTestSession";
import { getModelTestDisplay } from "../lib/testDisplay";
import { subscribeLastTestResults } from "../lib/lastTestResults";
import { Modal } from "./Modal";

type Props = {
  readonly providers: readonly Provider[];
  readonly models: readonly Model[];
  readonly prompts: readonly TestPrompt[];
  readonly onClose: () => void;
  readonly onToast: (msg: string) => void;
};

const FALLBACK_PROMPT = "请只回复一个单词：ok";
const DEFAULT_TIMEOUT = 30;
const CONCURRENCY = 3;

type ListSort = "default" | "latency_asc" | "latency_desc";

function clampTimeout(n: number): number {
  if (!Number.isFinite(n)) return DEFAULT_TIMEOUT;
  return Math.min(300, Math.max(5, Math.round(n)));
}

function sameProviderSet(
  a: readonly string[],
  b: readonly string[],
): boolean {
  if (a.length !== b.length) return false;
  const set = new Set(a);
  return b.every((id) => set.has(id));
}

export function MultiProviderTestModal({
  providers,
  models,
  prompts,
  onClose,
  onToast,
}: Props) {
  const selectedIds = useMemo(() => providers.map((p) => p.id), [providers]);

  const existing = getMultiTestSession();
  // Resume UI for ANY busy multi session, or a finished session that overlaps selection.
  const activeSession = (() => {
    const s = getMultiTestSession();
    if (!s) return null;
    if (s.busy) return s;
    const overlap = s.providerIds.some((id) => selectedIds.includes(id));
    if (overlap && s.rows.some((r) => r.status === "ok" || r.status === "fail" || r.logs.length > 0)) {
      return s;
    }
    if (sameProviderSet(s.providerIds, selectedIds)) return s;
    return null;
  })();

  const resumeSame = Boolean(
    activeSession &&
      (activeSession.busy ||
        sameProviderSet(activeSession.providerIds, selectedIds) ||
        activeSession.providerIds.some((id) => selectedIds.includes(id))),
  );

  const defaultPrompt = useMemo(() => {
    if (resumeSame && existing) return existing.prompt;
    const seeded = prompts.find((p) => p.isDefault) ?? prompts[0];
    return seeded?.content ?? FALLBACK_PROMPT;
  }, [prompts, resumeSame, existing]);

  const [selectedPromptId, setSelectedPromptId] = useState(() => {
    const seeded = prompts.find((p) => p.isDefault) ?? prompts[0];
    return seeded?.id ?? "";
  });
  const [prompt, setPrompt] = useState(defaultPrompt);
  const [timeoutSecs, setTimeoutSecs] = useState(
    resumeSame && existing ? existing.timeoutSecs : DEFAULT_TIMEOUT,
  );
  const [onlyEnabled, setOnlyEnabled] = useState(
    resumeSame && existing ? existing.onlyEnabled : false,
  );
  const [tick, setTick] = useState(0);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  /** providerId -> highlighted for list filter; missing key treated as true (default all on). */
  const [providerFilter, setProviderFilter] = useState<Record<string, boolean>>({});
  const [listSort, setListSort] = useState<ListSort>("default");

  useEffect(() => {
    const un1 = subscribeMultiTestSession(() => setTick((n) => n + 1));
    const un2 = subscribeLastTestResults(() => setTick((n) => n + 1));
    return () => {
      un1();
      un2();
    };
  }, []);

  void tick;
  const session = getMultiTestSession();
  // Prefer live multi session when busy, even if checkbox set changed.
  const viewSession =
    session?.busy
      ? session
      : session &&
          (sameProviderSet(session.providerIds, selectedIds) ||
            session.providerIds.some((id) => selectedIds.includes(id)))
        ? session
        : null;

  const busy = viewSession?.busy ?? false;
  const selectionMismatch =
    Boolean(viewSession?.busy) && !sameProviderSet(viewSession!.providerIds, selectedIds);

  const applyPrompt = (id: string) => {
    setSelectedPromptId(id);
    const p = prompts.find((x) => x.id === id);
    if (p) setPrompt(p.content);
  };

  const displayRows: MultiRowState[] = useMemo(() => {
    if (viewSession?.rows?.length) {
      // If selection differs while busy: show full running session rows
      if (viewSession.busy || selectionMismatch) {
        return viewSession.rows;
      }
      // Overlap: show session rows for selected providers + seed others from last
      const inSession = new Set(viewSession.providerIds);
      const fromSession = viewSession.rows.filter((r) => selectedIds.includes(r.providerId));
      const extra: MultiRowState[] = [];
      for (const p of providers) {
        if (inSession.has(p.id)) continue;
        for (const m of models.filter((x) => x.providerId === p.id)) {
          const d = getModelTestDisplay(m.id);
          extra.push({
            modelId: m.id,
            providerId: p.id,
            providerName: p.name,
            modelApiId: m.modelId,
            displayName: m.displayName,
            status: d.status as MultiRowStatus,
            result: d.result,
            error: d.error,
            logs: [...d.logs],
          });
        }
      }
      return [...fromSession, ...extra];
    }

    // No multi session: build from shared last/batch/multi display cache
    const out: MultiRowState[] = [];
    for (const p of providers) {
      for (const m of models.filter((x) => x.providerId === p.id)) {
        const d = getModelTestDisplay(m.id);
        out.push({
          modelId: m.id,
          providerId: p.id,
          providerName: p.name,
          modelApiId: m.modelId,
          displayName: m.displayName,
          status: d.status as MultiRowStatus,
          result: d.result,
          error: d.error,
          logs: [...d.logs],
        });
      }
    }
    return out;
  }, [viewSession, providers, models, selectedIds, selectionMismatch, tick]);

  /** Providers that appear in the current model list (session or selection). */
  const listProviders = useMemo(() => {
    const byId = new Map<string, { id: string; name: string }>();
    for (const r of displayRows) {
      if (!byId.has(r.providerId)) {
        byId.set(r.providerId, { id: r.providerId, name: r.providerName });
      }
    }
    // Keep a stable order: prefer current `providers` prop order, then rest
    const ordered: { id: string; name: string }[] = [];
    for (const p of providers) {
      const hit = byId.get(p.id);
      if (hit) {
        ordered.push(hit);
        byId.delete(p.id);
      }
    }
    for (const hit of byId.values()) ordered.push(hit);
    return ordered;
  }, [displayRows, providers]);

  const isProviderOn = (id: string): boolean => providerFilter[id] !== false;

  const allProvidersOn = useMemo(
    () => listProviders.length > 0 && listProviders.every((p) => isProviderOn(p.id)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [listProviders, providerFilter],
  );

  const toggleAllProviders = () => {
    const nextOn = !allProvidersOn;
    const next: Record<string, boolean> = { ...providerFilter };
    for (const p of listProviders) next[p.id] = nextOn;
    setProviderFilter(next);
  };

  const toggleProvider = (id: string) => {
    setProviderFilter((prev) => ({
      ...prev,
      [id]: prev[id] === false ? true : false,
    }));
  };

  const filteredRows = useMemo(() => {
    const rows = displayRows.filter((r) => isProviderOn(r.providerId));
    if (listSort === "default") return rows;

    const latency = (r: MultiRowState): number | null => {
      const ms = r.result?.latencyMs;
      return typeof ms === "number" && Number.isFinite(ms) ? ms : null;
    };

    return [...rows].sort((a, b) => {
      const la = latency(a);
      const lb = latency(b);
      // Rows without latency always sink to the bottom, keep relative order among them.
      if (la == null && lb == null) return 0;
      if (la == null) return 1;
      if (lb == null) return -1;
      return listSort === "latency_asc" ? la - lb : lb - la;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [displayRows, providerFilter, listSort]);

  const queuePreview = useMemo(() => {
    let n = 0;
    for (const p of providers) {
      for (const m of models.filter((x) => x.providerId === p.id)) {
        if (onlyEnabled && !m.enabled) continue;
        n += 1;
      }
    }
    return n;
  }, [providers, models, onlyEnabled]);

  const summary = useMemo(() => {
    let ok = 0;
    let fail = 0;
    let pending = 0;
    let running = 0;
    let skipped = 0;
    for (const r of displayRows) {
      if (r.status === "ok") ok += 1;
      else if (r.status === "fail") fail += 1;
      else if (r.status === "pending") pending += 1;
      else if (r.status === "running") running += 1;
      else if (r.status === "skipped") skipped += 1;
    }
    return { ok, fail, pending, running, skipped };
  }, [displayRows]);

  const runAll = async () => {
    if (busy) return;
    const existingBusy = getMultiTestSession();
    if (existingBusy?.busy) {
      onToast("已有跨提供商测试在进行中，请等待结束后再开新一轮");
      return;
    }
    const text = prompt.trim();
    if (!text) {
      onToast("提示词不能为空");
      return;
    }
    if (providers.length === 0) {
      onToast("请先勾选提供商");
      return;
    }
    const timeout = clampTimeout(timeoutSecs);
    setTimeoutSecs(timeout);
    if (queuePreview === 0) {
      onToast(onlyEnabled ? "没有已启用的模型可测" : "没有模型可测");
      return;
    }

    const modelsByProvider = new Map<string, Model[]>();
    for (const p of providers) {
      modelsByProvider.set(
        p.id,
        models.filter((m) => m.providerId === p.id),
      );
    }

    createMultiTestSession({
      providers,
      modelsByProvider,
      prompt: text,
      timeoutSecs: timeout,
      onlyEnabled,
      concurrency: CONCURRENCY,
    });
    setExpanded({});
    setTick((n) => n + 1);
    await startMultiTest();
  };

  const stop = () => {
    requestStopMultiTest();
    setTick((n) => n + 1);
  };

  const headerProviders = viewSession?.busy
    ? `进行中的会话：${viewSession.providerIds.length} 个提供商`
    : `已选 ${providers.length} 个`;

  return (
    <Modal onClose={onClose} xwide>
      <h3 className="mb-1 text-base font-semibold">批量测试所选提供商</h3>
      <p className="mb-4 text-xs text-ink-3">
        全局并发 {CONCURRENCY}；同一提供商下的模型串行。关闭弹窗不会中断测试；结果与日志在列表/详情间共享。
      </p>

      {selectionMismatch ? (
        <div className="mb-3 rounded-md border border-accent/30 bg-accent/10 px-3 py-2 text-xs text-ink-2">
          当前有其它勾选组合的测试仍在进行，下方显示的是<strong>进行中的会话</strong>
          （不是当前勾选）。完成后可再开新一轮。
        </div>
      ) : null}

      <dl className="mb-4 grid grid-cols-3 gap-2 text-sm">
        <div className="text-ink-3">提供商</div>
        <div className="col-span-2 text-xs">
          {headerProviders}
          {busy ? " · 测试进行中" : ""}
        </div>
        <div className="text-ink-3">预计模型</div>
        <div className="col-span-2 text-xs">
          {busy ? displayRows.length : queuePreview} 个
          {!busy && onlyEnabled ? "（仅已启用）" : ""}
        </div>
        <div className="text-ink-3">并发</div>
        <div className="col-span-2 text-xs">
          {CONCURRENCY}（跨提供商并行，同提供商串行）
        </div>
      </dl>

      <label className="mb-1 block text-xs text-ink-3">已保存提示词</label>
      <div className="mb-3 flex flex-wrap items-center gap-3">
        <select
          className="input min-w-[12rem] flex-1"
          value={selectedPromptId}
          disabled={busy}
          onChange={(e) => applyPrompt(e.target.value)}
        >
          {prompts.length === 0 ? <option value="">（无）</option> : null}
          {prompts.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
              {p.isDefault ? "（默认）" : ""}
            </option>
          ))}
        </select>
        <label className="flex items-center gap-1.5 text-xs text-ink-2">
          <input
            type="checkbox"
            checked={onlyEnabled}
            disabled={busy}
            onChange={(e) => setOnlyEnabled(e.target.checked)}
          />
          仅测试已启用
        </label>
      </div>

      <label className="mb-1 block text-xs text-ink-3">提示词</label>
      <textarea
        className="input mb-3 min-h-[96px] w-full resize-y font-mono text-xs"
        value={busy && viewSession ? viewSession.prompt : prompt}
        disabled={busy}
        onChange={(e) => setPrompt(e.target.value)}
        placeholder={FALLBACK_PROMPT}
      />

      <div className="mb-4 flex flex-wrap items-end gap-3">
        <div className="w-28">
          <label className="mb-1 block text-xs text-ink-3">超时（秒）</label>
          <input
            type="number"
            min={5}
            max={300}
            step={1}
            className="input w-full tabular-nums"
            value={busy && viewSession ? viewSession.timeoutSecs : timeoutSecs}
            disabled={busy}
            onChange={(e) => setTimeoutSecs(Number(e.target.value))}
          />
        </div>
        <span className="mb-2 flex-1 text-xs text-ink-3">
          成功 {summary.ok} · 失败 {summary.fail}
          {summary.running ? ` · 进行中 ${summary.running}` : ""}
          {summary.skipped ? ` · 跳过 ${summary.skipped}` : ""}
          {summary.pending && busy ? ` · 待测 ${summary.pending}` : ""}
        </span>
        <div className="flex justify-end gap-2">
          {busy ? (
            <button type="button" className="btn-secondary" onClick={stop}>
              停止
            </button>
          ) : (
            <button type="button" className="btn-secondary" onClick={onClose}>
              关闭
            </button>
          )}
          <button
            type="button"
            className="btn-primary min-w-[7rem]"
            disabled={busy || queuePreview === 0 || providers.length === 0}
            onClick={() => void runAll()}
          >
            {busy ? "测试中…" : `开始测试（${queuePreview}）`}
          </button>
        </div>
      </div>

      <div className="mb-3 space-y-2">
        {listProviders.length > 0 ? (
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="mr-1 text-xs text-ink-3">筛选</span>
            <button
              type="button"
              className={
                allProvidersOn
                  ? "rounded-full border border-accent/50 bg-accent/15 px-2.5 py-0.5 text-xs text-accent"
                  : "rounded-full border border-surface-3 bg-surface-0 px-2.5 py-0.5 text-xs text-ink-3"
              }
              onClick={toggleAllProviders}
              title={allProvidersOn ? "取消全部" : "全选提供商"}
            >
              全部
            </button>
            {listProviders.map((p) => {
              const on = isProviderOn(p.id);
              return (
                <button
                  key={p.id}
                  type="button"
                  className={
                    on
                      ? "rounded-full border border-accent/50 bg-accent/15 px-2.5 py-0.5 text-xs text-accent"
                      : "rounded-full border border-surface-3 bg-surface-0 px-2.5 py-0.5 text-xs text-ink-3"
                  }
                  onClick={() => toggleProvider(p.id)}
                  title={on ? `隐藏 ${p.name}` : `显示 ${p.name}`}
                >
                  {p.name}
                </button>
              );
            })}
          </div>
        ) : null}
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="mr-1 text-xs text-ink-3">排序</span>
          {(
            [
              ["default", "默认"],
              ["latency_asc", "响应时间 ↑"],
              ["latency_desc", "响应时间 ↓"],
            ] as const
          ).map(([id, label]) => {
            const on = listSort === id;
            return (
              <button
                key={id}
                type="button"
                className={
                  on
                    ? "rounded-full border border-accent/50 bg-accent/15 px-2.5 py-0.5 text-xs text-accent"
                    : "rounded-full border border-surface-3 bg-surface-0 px-2.5 py-0.5 text-xs text-ink-3"
                }
                onClick={() => setListSort(id)}
                title={
                  id === "default"
                    ? "会话原始顺序"
                    : id === "latency_asc"
                      ? "响应时间从小到大（无数据在后）"
                      : "响应时间从大到小（无数据在后）"
                }
              >
                {label}
              </button>
            );
          })}
        </div>
      </div>

      <div className="max-h-[28rem] space-y-2 overflow-auto">
        {filteredRows.length === 0 ? (
          <div className="rounded-md border border-dashed border-surface-3 px-3 py-6 text-center text-xs text-ink-3">
            没有可显示的模型（请点亮上方提供商筛选）
          </div>
        ) : null}
        {filteredRows.map((r) => {
          const isOpen = Boolean(expanded[r.modelId]);
          const hasLogs = r.logs.length > 0;
          return (
            <div
              key={r.modelId}
              className="rounded-md border border-surface-3 bg-surface-1"
            >
              <div className="flex items-center gap-2 px-2.5 py-2">
                <StatusBadge status={r.status} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[11px] text-ink-3">{r.providerName}</div>
                  <div className="truncate font-mono text-xs" title={r.modelApiId}>
                    {r.modelApiId}
                  </div>
                </div>
                <span className="shrink-0 tabular-nums text-xs text-ink-3">
                  {r.result ? `${r.result.latencyMs} ms` : "—"}
                </span>
                <button
                  type="button"
                  className="btn-ghost shrink-0 !px-2 !py-0.5 text-xs"
                  disabled={!hasLogs && r.status === "pending"}
                  onClick={() =>
                    setExpanded((prev) => ({ ...prev, [r.modelId]: !prev[r.modelId] }))
                  }
                >
                  {isOpen ? "收起日志" : "日志"}
                  {hasLogs ? ` (${r.logs.length})` : ""}
                </button>
              </div>
              {isOpen ? (
                <pre className="max-h-48 overflow-y-auto overflow-x-hidden border-t border-surface-3 bg-surface-0 p-2.5 font-mono text-[11px] leading-relaxed text-ink-2 whitespace-pre-wrap break-words [overflow-wrap:anywhere]">
                  {hasLogs
                    ? r.logs.join("\n")
                    : r.status === "running"
                      ? "等待日志…"
                      : r.status === "skipped"
                        ? "已跳过（未测试）"
                        : "暂无日志"}
                </pre>
              ) : null}
            </div>
          );
        })}
      </div>
    </Modal>
  );
}

function StatusBadge({ status }: { readonly status: MultiRowStatus }) {
  const map: Record<MultiRowStatus, string> = {
    pending: "bg-surface-3 text-ink-3",
    running: "bg-accent/20 text-accent",
    ok: "bg-ok/15 text-ok",
    fail: "bg-danger/20 text-danger",
    skipped: "bg-surface-3 text-ink-3",
  };
  const label: Record<MultiRowStatus, string> = {
    pending: "待测",
    running: "测试中",
    ok: "成功",
    fail: "失败",
    skipped: "跳过",
  };
  return <span className={`badge ${map[status]}`}>{label[status]}</span>;
}
