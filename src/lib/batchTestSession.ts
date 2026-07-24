import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Model, TestConnectionResult } from "../types";
import * as api from "../api/tauri";
import { getLastTestResult, setLastTestResult } from "./lastTestResults";

export type BatchRowStatus = "pending" | "running" | "ok" | "fail" | "skipped";

export type BatchRowState = {
  readonly modelId: string;
  readonly modelApiId: string;
  readonly displayName: string;
  status: BatchRowStatus;
  result: TestConnectionResult | null;
  error: string | null;
  /** Per-model request log lines (live + final). */
  logs: string[];
};

export type BatchTestSession = {
  readonly id: string;
  readonly providerId: string;
  readonly providerName: string;
  readonly protocol: string;
  readonly prompt: string;
  readonly timeoutSecs: number;
  readonly onlyEnabled: boolean;
  readonly models: readonly Model[];
  /** Extra HTTP headers for every model in this run. */
  readonly extraHeaders: Readonly<Record<string, string>>;
  rows: BatchRowState[];
  busy: boolean;
  cancelled: boolean;
  currentIndex: number;
  queueIds: string[];
  activeRunId: string | null;
  activeModelId: string | null;
  listeners: Set<() => void>;
};

let session: BatchTestSession | null = null;
let unlistenLog: UnlistenFn | null = null;
let listenPromise: Promise<void> | null = null;

function notify() {
  if (!session) return;
  for (const l of [...session.listeners]) {
    try {
      l();
    } catch {
      /* ignore */
    }
  }
}

export function getBatchTestSession(): BatchTestSession | null {
  return session;
}

const globalListeners = new Set<() => void>();

export function subscribeBatchTestSession(listener: () => void): () => void {
  const wrap = () => listener();
  session?.listeners.add(wrap);
  globalListeners.add(wrap);
  return () => {
    globalListeners.delete(wrap);
    session?.listeners.delete(wrap);
  };
}

function attachGlobalListeners(s: BatchTestSession) {
  for (const l of globalListeners) {
    s.listeners.add(l);
  }
}

function newRunId(): string {
  return `batch_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 10)}`;
}

function appendRowLog(modelId: string, line: string) {
  if (!session) return;
  session.rows = session.rows.map((r) =>
    r.modelId === modelId ? { ...r, logs: [...r.logs, line] } : r,
  );
  const row = session.rows.find((r) => r.modelId === modelId);
  if (row) {
    setLastTestResult(modelId, row.result?.ok ?? false, row.result?.latencyMs, {
      logs: row.logs,
      result: row.result,
      memoryOnly: true,
    });
  }
}

function setRowLogs(modelId: string, logs: string[]) {
  if (!session) return;
  session.rows = session.rows.map((r) =>
    r.modelId === modelId ? { ...r, logs } : r,
  );
}

export type StartBatchArgs = {
  providerId: string;
  providerName: string;
  protocol: string;
  models: readonly Model[];
  prompt: string;
  timeoutSecs: number;
  onlyEnabled: boolean;
  extraHeaders?: Readonly<Record<string, string>>;
};

export function createBatchTestSession(args: StartBatchArgs): BatchTestSession {
  if (session?.busy && session.providerId === args.providerId) {
    return session;
  }
  const queue = args.onlyEnabled ? args.models.filter((m) => m.enabled) : [...args.models];
  const queueIds = queue.map((m) => m.id);
  const queueSet = new Set(queueIds);
  session = {
    id: newRunId(),
    providerId: args.providerId,
    providerName: args.providerName,
    protocol: args.protocol,
    prompt: args.prompt,
    timeoutSecs: args.timeoutSecs,
    onlyEnabled: args.onlyEnabled,
    models: args.models,
    extraHeaders: { ...(args.extraHeaders ?? {}) },
    rows: args.models.map((m) => {
      const last = getLastTestResult(m.id);
      return {
        modelId: m.id,
        modelApiId: m.modelId,
        displayName: m.displayName,
        status: queueSet.has(m.id) ? ("pending" as const) : ("skipped" as const),
        result: last?.result ?? null,
        error: null,
        logs: last?.logs?.length ? [...last.logs] : [],
      };
    }),
    busy: false,
    cancelled: false,
    currentIndex: -1,
    queueIds,
    activeRunId: null,
    activeModelId: null,
    listeners: new Set(),
  };
  attachGlobalListeners(session);
  notify();
  return session;
}

export function requestStopBatchTest(): void {
  if (!session || !session.busy) return;
  session.cancelled = true;
  if (session.activeModelId) {
    appendRowLog(session.activeModelId, "[batch] stop requested — finishing current, skip rest…");
  }
  // Immediately mark not-yet-started rows so the UI reacts (pending would look stuck).
  session.rows = session.rows.map((r) =>
    r.status === "pending"
      ? {
          ...r,
          status: "skipped" as const,
          error: null,
          logs: [...r.logs, "[batch] skipped (stopped)"],
        }
      : r,
  );
  // No in-flight model (race / empty queue): free the session now.
  if (!session.rows.some((r) => r.status === "running")) {
    session.activeRunId = null;
    session.activeModelId = null;
    session.currentIndex = -1;
    session.busy = false;
  }
  notify();
}

async function ensureLogListener(): Promise<void> {
  if (unlistenLog) return;
  if (listenPromise) return listenPromise;
  listenPromise = listen<api.TestConnectionLogEvent>(api.TEST_CONNECTION_LOG_EVENT, (event) => {
    const payload = event.payload;
    if (!payload?.line || !payload.runId) return;
    if (!session || session.activeRunId !== payload.runId || !session.activeModelId) return;
    appendRowLog(session.activeModelId, payload.line);
    notify();
  }).then((fn) => {
    unlistenLog = fn;
    listenPromise = null;
  });
  await listenPromise;
}

export async function startBatchTest(): Promise<void> {
  if (!session || session.busy) return;
  const s = session;
  await ensureLogListener();
  s.busy = true;
  s.cancelled = false;
  s.currentIndex = -1;
  s.activeModelId = null;
  s.activeRunId = null;
  notify();

  let okCount = 0;
  let failCount = 0;
  const queue = s.queueIds
    .map((id) => s.models.find((m) => m.id === id))
    .filter((m): m is Model => Boolean(m));

  for (let i = 0; i < queue.length; i += 1) {
    if (!session || session.id !== s.id) break;
    if (s.cancelled) {
      notify();
      break;
    }

    const model = queue[i]!;
    s.currentIndex = i;
    const runId = newRunId();
    s.activeRunId = runId;
    s.activeModelId = model.id;
    s.rows = s.rows.map((r) =>
      r.modelId === model.id
        ? {
            ...r,
            status: "running",
            result: null,
            error: null,
            logs: [
              `[batch] (${i + 1}/${queue.length}) ${model.modelId}`,
              `run=${runId} timeout=${s.timeoutSecs}s`,
            ],
          }
        : r,
    );
    notify();

    try {
      const res = await api.testModelConnection(
        model.id,
        s.prompt,
        runId,
        s.timeoutSecs,
        s.extraHeaders,
      );
      if (session?.id !== s.id) break;

      // Prefer full backend logs when available.
      if (res.logs?.length) {
        setRowLogs(model.id, [
          `[batch] (${i + 1}/${queue.length}) ${model.modelId}`,
          ...res.logs,
        ]);
      }

      const ok = res.ok;
      if (ok) okCount += 1;
      else failCount += 1;
      const finalLogs = res.logs?.length
        ? [`[batch] (${i + 1}/${queue.length}) ${model.modelId}`, ...res.logs]
        : undefined;
      s.rows = s.rows.map((r) =>
        r.modelId === model.id
          ? {
              ...r,
              status: ok ? "ok" : "fail",
              result: res,
              error: ok ? null : res.error ?? `HTTP ${res.httpStatus ?? "?"}`,
              logs: finalLogs
                ? finalLogs
                : [
                    ...r.logs,
                    ok
                      ? `[batch] ok ${res.latencyMs}ms`
                      : `[batch] fail: ${res.error ?? "unknown"}`,
                  ],
            }
          : r,
      );
      {
        const row = s.rows.find((r) => r.modelId === model.id);
        setLastTestResult(model.id, ok, res.latencyMs, {
          logs: row?.logs ?? finalLogs ?? res.logs ?? [],
          result: res,
        });
      }
      notify();
    } catch (e) {
      if (session?.id !== s.id) break;
      const msg = e instanceof Error ? e.message : String(e);
      failCount += 1;
      s.rows = s.rows.map((r) =>
        r.modelId === model.id
          ? {
              ...r,
              status: "fail",
              result: null,
              error: msg,
              logs: [...r.logs, `[batch] invoke error: ${msg}`],
            }
          : r,
      );
      {
        const row = s.rows.find((r) => r.modelId === model.id);
        setLastTestResult(model.id, false, undefined, {
          logs: row?.logs ?? [],
          result: null,
        });
      }
      notify();
    }
  }

  if (session?.id === s.id) {
    // Safety: any leftover pending after cancel/break → skipped.
    if (s.cancelled) {
      s.rows = s.rows.map((r) =>
        r.status === "pending"
          ? {
              ...r,
              status: "skipped" as const,
              logs: [...r.logs, "[batch] skipped (stopped)"],
            }
          : r,
      );
    }
    s.activeRunId = null;
    s.activeModelId = null;
    s.currentIndex = -1;
    s.busy = false;
    notify();
  }

  void okCount;
  void failCount;
}
