/**
 * Multi-provider connectivity test:
 * - Global concurrency = 3
 * - At most one in-flight model test per provider
 * - Session survives modal close/reopen
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Model, Provider, TestConnectionResult } from "../types";
import * as api from "../api/tauri";
import { getLastTestResult, setLastTestResult } from "./lastTestResults";

export type MultiRowStatus = "pending" | "running" | "ok" | "fail" | "skipped";

export type MultiRowState = {
  readonly modelId: string;
  readonly providerId: string;
  readonly providerName: string;
  readonly modelApiId: string;
  readonly displayName: string;
  status: MultiRowStatus;
  result: TestConnectionResult | null;
  error: string | null;
  logs: string[];
};

export type MultiTestSession = {
  readonly id: string;
  readonly providerIds: readonly string[];
  readonly prompt: string;
  readonly timeoutSecs: number;
  readonly onlyEnabled: boolean;
  readonly concurrency: number;
  rows: MultiRowState[];
  busy: boolean;
  cancelled: boolean;
  /** modelId currently receiving streamed logs for a runId */
  activeByRunId: Map<string, string>;
  listeners: Set<() => void>;
};

let session: MultiTestSession | null = null;
let unlistenLog: UnlistenFn | null = null;
let listenPromise: Promise<void> | null = null;
const globalListeners = new Set<() => void>();

const DEFAULT_CONCURRENCY = 3;

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

export function getMultiTestSession(): MultiTestSession | null {
  return session;
}

export function subscribeMultiTestSession(listener: () => void): () => void {
  const wrap = () => listener();
  session?.listeners.add(wrap);
  globalListeners.add(wrap);
  return () => {
    globalListeners.delete(wrap);
    session?.listeners.delete(wrap);
  };
}

function attachGlobalListeners(s: MultiTestSession) {
  for (const l of globalListeners) {
    s.listeners.add(l);
  }
}

function newId(prefix: string): string {
  return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 10)}`;
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

export type StartMultiArgs = {
  providers: readonly Provider[];
  modelsByProvider: ReadonlyMap<string, readonly Model[]>;
  prompt: string;
  timeoutSecs: number;
  onlyEnabled: boolean;
  concurrency?: number;
};

export function createMultiTestSession(args: StartMultiArgs): MultiTestSession {
  // Never replace an in-flight run (avoids orphaned network requests).
  if (session?.busy) {
    return session;
  }

  const providerIds = args.providers.map((p) => p.id);
  const rows: MultiRowState[] = [];
  for (const p of args.providers) {
    const models = args.modelsByProvider.get(p.id) ?? [];
    for (const m of models) {
      const inQueue = args.onlyEnabled ? m.enabled : true;
      const last = getLastTestResult(m.id);
      rows.push({
        modelId: m.id,
        providerId: p.id,
        providerName: p.name,
        modelApiId: m.modelId,
        displayName: m.displayName,
        status: inQueue ? "pending" : "skipped",
        result: last?.result ?? null,
        error: null,
        logs: last?.logs?.length ? [...last.logs] : [],
      });
    }
  }

  session = {
    id: newId("multi"),
    providerIds,
    prompt: args.prompt,
    timeoutSecs: args.timeoutSecs,
    onlyEnabled: args.onlyEnabled,
    concurrency: Math.max(1, Math.min(8, args.concurrency ?? DEFAULT_CONCURRENCY)),
    rows,
    busy: false,
    cancelled: false,
    activeByRunId: new Map(),
    listeners: new Set(),
  };
  attachGlobalListeners(session);
  notify();
  return session;
}

export function requestStopMultiTest(): void {
  if (!session || !session.busy) return;
  session.cancelled = true;
  for (const r of session.rows) {
    if (r.status === "running") {
      appendRowLog(r.modelId, "[multi] stop requested…");
    }
  }
  notify();
}

async function ensureLogListener(): Promise<void> {
  if (unlistenLog) return;
  if (listenPromise) return listenPromise;
  listenPromise = listen<api.TestConnectionLogEvent>(api.TEST_CONNECTION_LOG_EVENT, (event) => {
    const payload = event.payload;
    if (!payload?.line || !payload.runId) return;
    if (!session) return;
    const modelId = session.activeByRunId.get(payload.runId);
    if (!modelId) return;
    appendRowLog(modelId, payload.line);
    notify();
  }).then((fn) => {
    unlistenLog = fn;
    listenPromise = null;
  });
  await listenPromise;
}

async function runOneModel(
  s: MultiTestSession,
  modelId: string,
  indexLabel: string,
): Promise<void> {
  const row = s.rows.find((r) => r.modelId === modelId);
  if (!row) return;

  const runId = newId("mrun");
  s.activeByRunId.set(runId, modelId);
  const startLogs = [
    `[multi] ${indexLabel} ${row.providerName} / ${row.modelApiId}`,
    `run=${runId} timeout=${s.timeoutSecs}s concurrency=${s.concurrency}`,
  ];
  s.rows = s.rows.map((r) =>
    r.modelId === modelId
      ? {
          ...r,
          status: "running",
          result: null,
          error: null,
          logs: startLogs,
        }
      : r,
  );
  setLastTestResult(modelId, false, undefined, {
    logs: startLogs,
    result: null,
    memoryOnly: true,
  });
  notify();

  try {
    const res = await api.testModelConnection(modelId, s.prompt, runId, s.timeoutSecs);
    if (session?.id !== s.id) return;

    const ok = res.ok;
    s.rows = s.rows.map((r) =>
      r.modelId === modelId
        ? {
            ...r,
            status: ok ? "ok" : "fail",
            result: res,
            error: ok ? null : res.error ?? `HTTP ${res.httpStatus ?? "?"}`,
            logs: res.logs?.length
              ? [`[multi] ${indexLabel} ${r.providerName} / ${r.modelApiId}`, ...res.logs]
              : [
                  ...r.logs,
                  ok
                    ? `[multi] ok ${res.latencyMs}ms`
                    : `[multi] fail: ${res.error ?? "unknown"}`,
                ],
          }
        : r,
    );
    {
      const row = s.rows.find((r) => r.modelId === modelId);
      setLastTestResult(modelId, ok, res.latencyMs, {
        logs: row?.logs ?? res.logs ?? [],
        result: res,
      });
    }
    notify();
  } catch (e) {
    if (session?.id !== s.id) return;
    const msg = e instanceof Error ? e.message : String(e);
    s.rows = s.rows.map((r) =>
      r.modelId === modelId
        ? {
            ...r,
            status: "fail",
            result: null,
            error: msg,
            logs: [...r.logs, `[multi] invoke error: ${msg}`],
          }
        : r,
    );
    {
      const row = s.rows.find((r) => r.modelId === modelId);
      setLastTestResult(modelId, false, undefined, {
        logs: row?.logs ?? [],
        result: null,
      });
    }
    notify();
  } finally {
    s.activeByRunId.delete(runId);
  }
}

/**
 * Schedule pending models with:
 * - max `concurrency` in-flight requests
 * - at most one running model per providerId
 */
export async function startMultiTest(): Promise<void> {
  if (!session || session.busy) return;
  const s = session;
  await ensureLogListener();

  const pendingIds = s.rows.filter((r) => r.status === "pending").map((r) => r.modelId);
  if (pendingIds.length === 0) {
    notify();
    return;
  }

  s.busy = true;
  s.cancelled = false;
  notify();

  const waiting = new Set(pendingIds);
  const inFlightProviders = new Set<string>();
  let active = 0;
  let finished = 0;
  const total = pendingIds.length;

  await new Promise<void>((resolve) => {
    const finishIfDone = () => {
      if (session?.id !== s.id) {
        resolve();
        return true;
      }
      if (s.cancelled && active === 0) {
        s.busy = false;
        notify();
        resolve();
        return true;
      }
      if (!s.cancelled && finished >= total && active === 0 && waiting.size === 0) {
        s.busy = false;
        notify();
        resolve();
        return true;
      }
      return false;
    };

    const pump = () => {
      if (finishIfDone()) return;
      if (s.cancelled) return;

      let scheduled = true;
      while (scheduled && active < s.concurrency && waiting.size > 0) {
        scheduled = false;
        for (const modelId of [...waiting]) {
          if (active >= s.concurrency) break;
          const row = s.rows.find((r) => r.modelId === modelId);
          if (!row || row.status !== "pending") {
            waiting.delete(modelId);
            continue;
          }
          if (inFlightProviders.has(row.providerId)) continue;

          waiting.delete(modelId);
          inFlightProviders.add(row.providerId);
          active += 1;
          scheduled = true;
          const label = `(${finished + active}/${total})`;

          void runOneModel(s, modelId, label).finally(() => {
            inFlightProviders.delete(row.providerId);
            active -= 1;
            finished += 1;
            pump();
          });
        }
      }

      // All remaining waiters blocked only by in-flight providers, or empty.
      if (active === 0 && waiting.size === 0) {
        finishIfDone();
      }
    };

    pump();
  });
}


export function multiSessionBusyForProviders(providerIds: readonly string[]): boolean {
  if (!session?.busy) return false;
  return providerIds.some((id) => session!.providerIds.includes(id));
}

export function isMultiTestBusy(): boolean {
  return Boolean(session?.busy);
}
