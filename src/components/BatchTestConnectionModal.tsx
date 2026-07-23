import { useEffect, useMemo, useState } from "react";
import type { Model, Provider, TestPrompt } from "../types";
import {
  createBatchTestSession,
  getBatchTestSession,
  requestStopBatchTest,
  startBatchTest,
  subscribeBatchTestSession,
  type BatchRowStatus,
  type BatchRowState,
  type BatchTestSession,
} from "../lib/batchTestSession";
import { getModelTestDisplay } from "../lib/testDisplay";
import { subscribeLastTestResults } from "../lib/lastTestResults";
import { getMultiTestSession, requestStopMultiTest, subscribeMultiTestSession } from "../lib/multiTestSession";
import { Modal } from "./Modal";

type Props = {
  readonly provider: Provider;
  readonly models: readonly Model[];
  readonly prompts: readonly TestPrompt[];
  readonly onClose: () => void;
  readonly onToast: (msg: string) => void;
};

const FALLBACK_PROMPT = "请只回复一个单词：ok";
const DEFAULT_TIMEOUT = 30;

type ListSort = "default" | "latency_asc" | "latency_desc";

function clampTimeout(n: number): number {
  if (!Number.isFinite(n)) return DEFAULT_TIMEOUT;
  return Math.min(300, Math.max(5, Math.round(n)));
}

function snapshot(session: BatchTestSession | null) {
  if (!session) return null;
  return {
    id: session.id,
    busy: session.busy,
    rows: session.rows,
    currentIndex: session.currentIndex,
    queueIds: session.queueIds,
    prompt: session.prompt,
    timeoutSecs: session.timeoutSecs,
    onlyEnabled: session.onlyEnabled,
    providerId: session.providerId,
    cancelled: session.cancelled,
  };
}

export function BatchTestConnectionModal({
  provider,
  models,
  prompts,
  onClose,
  onToast,
}: Props) {
  const existing = getBatchTestSession();
  const multiExisting = getMultiTestSession();
  const batchResume =
    Boolean(existing) &&
    existing!.providerId === provider.id &&
    (existing!.busy ||
      existing!.rows.some((r) => r.status === "ok" || r.status === "fail"));
  const multiResume = Boolean(
    multiExisting?.rows.some(
      (r) =>
        r.providerId === provider.id &&
        (multiExisting.busy ||
          r.status === "ok" ||
          r.status === "fail" ||
          r.status === "running" ||
          r.logs.length > 0),
    ),
  );
  // Prefer batch session settings; fall back to multi when list test is the source.
  const resumePrompt = batchResume
    ? existing!.prompt
    : multiResume
      ? multiExisting!.prompt
      : null;
  const resumeTimeout = batchResume
    ? existing!.timeoutSecs
    : multiResume
      ? multiExisting!.timeoutSecs
      : null;
  const resumeOnlyEnabled = batchResume
    ? existing!.onlyEnabled
    : multiResume
      ? multiExisting!.onlyEnabled
      : null;

  const defaultPrompt = useMemo(() => {
    if (resumePrompt != null) return resumePrompt;
    const seeded = prompts.find((p) => p.isDefault) ?? prompts[0];
    return seeded?.content ?? FALLBACK_PROMPT;
  }, [prompts, resumePrompt]);

  const [selectedPromptId, setSelectedPromptId] = useState(() => {
    const seeded = prompts.find((p) => p.isDefault) ?? prompts[0];
    return seeded?.id ?? "";
  });
  const [prompt, setPrompt] = useState(defaultPrompt);
  const [timeoutSecs, setTimeoutSecs] = useState(
    resumeTimeout ?? DEFAULT_TIMEOUT,
  );
  const [onlyEnabled, setOnlyEnabled] = useState(
    resumeOnlyEnabled ?? false,
  );
  const [tick, setTick] = useState(0);
  /** modelId -> expanded */
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [listSort, setListSort] = useState<ListSort>("default");

  useEffect(() => {
    const un1 = subscribeBatchTestSession(() => setTick((n) => n + 1));
    const un2 = subscribeMultiTestSession(() => setTick((n) => n + 1));
    const un3 = subscribeLastTestResults(() => setTick((n) => n + 1));
    return () => {
      un1();
      un2();
      un3();
    };
  }, []);

  const sessionSnap = snapshot(
    getBatchTestSession()?.providerId === provider.id ? getBatchTestSession() : null,
  );
  void tick;
  const multiBusy = Boolean(
    getMultiTestSession()?.busy &&
      getMultiTestSession()!.rows.some((r) => r.providerId === provider.id),
  );
  const busy = (sessionSnap?.busy ?? false) || multiBusy;
  const rows = sessionSnap?.rows;
  const currentIndex = sessionSnap?.currentIndex ?? -1;
  const queueLen = sessionSnap?.queueIds.length ?? 0;


  const applyPrompt = (id: string) => {
    setSelectedPromptId(id);
    const p = prompts.find((x) => x.id === id);
    if (p) setPrompt(p.content);
  };

  const displayRows: BatchRowState[] = useMemo(() => {
    // Prefer this provider's dedicated batch session when present.
    if (rows && rows.length && sessionSnap) {
      return rows;
    }
    // Otherwise merge multi-session rows + shared last results for this provider.
    const multi = getMultiTestSession();
    const multiRows =
      multi?.rows.filter((r) => r.providerId === provider.id) ?? [];
    const multiById = new Map(multiRows.map((r) => [r.modelId, r]));

    return models.map((m) => {
      const mr = multiById.get(m.id);
      if (mr && (multi?.busy || mr.status !== "pending" || mr.logs.length > 0 || mr.result)) {
        return {
          modelId: m.id,
          modelApiId: m.modelId,
          displayName: m.displayName,
          status: mr.status as BatchRowStatus,
          result: mr.result,
          error: mr.error,
          logs: [...mr.logs],
        };
      }
      const d = getModelTestDisplay(m.id);
      return {
        modelId: m.id,
        modelApiId: m.modelId,
        displayName: m.displayName,
        status: d.status as BatchRowStatus,
        result: d.result,
        error: d.error,
        logs: [...d.logs],
      };
    });
  }, [rows, models, sessionSnap, provider.id, tick]);

  const sortedRows = useMemo(() => {
    if (listSort === "default") return displayRows;
    const latency = (r: BatchRowState): number | null => {
      const ms = r.result?.latencyMs;
      return typeof ms === "number" && Number.isFinite(ms) ? ms : null;
    };
    return [...displayRows].sort((a, b) => {
      const la = latency(a);
      const lb = latency(b);
      if (la == null && lb == null) return 0;
      if (la == null) return 1;
      if (lb == null) return -1;
      return listSort === "latency_asc" ? la - lb : lb - la;
    });
  }, [displayRows, listSort]);

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

  const queuePreview = useMemo(() => {
    if (onlyEnabled) return models.filter((m) => m.enabled).length;
    return models.length;
  }, [models, onlyEnabled]);

  const runAll = async () => {
    if (sessionSnap?.busy) return;
    const multi = getMultiTestSession();
    if (multi?.busy && multi.rows.some((r) => r.providerId === provider.id)) {
      onToast("该提供商已在列表「测试所选」中测试，请等待完成或在列表弹窗中停止");
      return;
    }
    const text = prompt.trim();
    if (!text) {
      onToast("提示词不能为空");
      return;
    }
    const timeout = clampTimeout(timeoutSecs);
    setTimeoutSecs(timeout);
    if (queuePreview === 0) {
      onToast(onlyEnabled ? "没有已启用的模型可测" : "没有模型可测");
      return;
    }

    createBatchTestSession({
      providerId: provider.id,
      providerName: provider.name,
      protocol: provider.protocol,
      models,
      prompt: text,
      timeoutSecs: timeout,
      onlyEnabled,
    });
    setExpanded({});
    setTick((n) => n + 1);
    await startBatchTest();
  };

  const stop = () => {
    if (sessionSnap?.busy) {
      requestStopBatchTest();
    }
    const multi = getMultiTestSession();
    if (multi?.busy && multi.rows.some((r) => r.providerId === provider.id)) {
      requestStopMultiTest();
    }
    setTick((n) => n + 1);
  };

  const close = () => {
    onClose();
  };

  const toggleExpand = (modelId: string) => {
    setExpanded((prev) => ({ ...prev, [modelId]: !prev[modelId] }));
  };

  return (
    <Modal onClose={close} xwide>
      <h3 className="mb-1 text-base font-semibold">批量连通性测试</h3>
      <p className="mb-4 text-xs text-ink-3">
        串行测试该 Provider 下的模型（并发 1）。关闭弹窗不会中断进行中的测试，再次打开可继续查看进度。
      </p>
      {multiBusy || (multiResume && !batchResume) ? (
        <div className="mb-3 rounded-md border border-accent/30 bg-accent/10 px-3 py-2 text-xs text-ink-2">
          {multiBusy
            ? "该提供商正在列表「测试所选」中运行。下方显示共享进度与日志；可停止，或等待结束后再在本页开新一轮。"
            : "下方结果来自列表「测试所选」的共享缓存。"}
        </div>
      ) : null}

      <dl className="mb-4 grid grid-cols-3 gap-2 text-sm">
        <div className="text-ink-3">提供商</div>
        <div className="col-span-2">{provider.name}</div>
        <div className="text-ink-3">协议</div>
        <div className="col-span-2 font-mono text-xs">{provider.protocol}</div>
        <div className="text-ink-3">模型数</div>
        <div className="col-span-2 text-xs">
          共 {models.length} 个
          {onlyEnabled ? ` · 将测 ${queuePreview} 个（仅已启用）` : ""}
          {busy ? " · 测试进行中" : ""}
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
        value={
          sessionSnap?.busy
            ? sessionSnap.prompt
            : multiBusy && multiExisting
              ? multiExisting.prompt
              : prompt
        }
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
            value={
              sessionSnap?.busy
                ? sessionSnap.timeoutSecs
                : multiBusy && multiExisting
                  ? multiExisting.timeoutSecs
                  : timeoutSecs
            }
            disabled={busy}
            onChange={(e) => setTimeoutSecs(Number(e.target.value))}
            title="单次请求超时，范围 5–300 秒"
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
            <button type="button" className="btn-secondary" onClick={close}>
              关闭
            </button>
          )}
          <button
            type="button"
            className="btn-primary min-w-[7rem]"
            disabled={(sessionSnap?.busy ?? false) || multiBusy || queuePreview === 0}
            onClick={() => void runAll()}
          >
            {sessionSnap?.busy
              ? `测试中… ${Math.max(currentIndex, 0) + 1}/${queueLen || queuePreview}`
              : multiBusy
                ? "测试中…"
                : `开始测试（${queuePreview}）`}
          </button>
        </div>
      </div>

      <div className="mb-3 flex flex-wrap items-center gap-1.5">
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
                  ? "原始顺序"
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

      <div className="max-h-[28rem] space-y-2 overflow-auto">
        {sortedRows.map((r) => {
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
                  <div className="truncate font-mono text-xs" title={r.modelApiId}>
                    {r.modelApiId}
                  </div>
                  {r.displayName && r.displayName !== r.modelApiId ? (
                    <div className="truncate text-[11px] text-ink-3">{r.displayName}</div>
                  ) : null}
                </div>
                <span className="shrink-0 tabular-nums text-xs text-ink-3">
                  {r.result ? `${r.result.latencyMs} ms` : "—"}
                </span>
                <button
                  type="button"
                  className="btn-ghost shrink-0 !px-2 !py-0.5 text-xs"
                  disabled={!hasLogs && r.status === "pending"}
                  onClick={() => toggleExpand(r.modelId)}
                  title={hasLogs ? (isOpen ? "收起日志" : "展开日志") : "暂无日志"}
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

function StatusBadge({ status }: { readonly status: BatchRowStatus }) {
  const map: Record<BatchRowStatus, string> = {
    pending: "bg-surface-3 text-ink-3",
    running: "bg-accent/20 text-accent",
    ok: "bg-ok/15 text-ok",
    fail: "bg-danger/20 text-danger",
    skipped: "bg-surface-3 text-ink-3",
  };
  const label: Record<BatchRowStatus, string> = {
    pending: "待测",
    running: "测试中",
    ok: "成功",
    fail: "失败",
    skipped: "跳过",
  };
  return <span className={`badge ${map[status]}`}>{label[status]}</span>;
}
