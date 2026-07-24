import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { TestConnectionResult } from "../types";
import * as api from "../api/tauri";
import {
  clearLastTestLogs,
  getLastTestResult,
  setLastTestResult,
} from "./lastTestResults";

export type SingleTestSession = {
  readonly modelId: string;
  readonly providerId: string;
  readonly modelApiId: string;
  readonly providerName: string;
  readonly protocol: string;
  prompt: string;
  timeoutSecs: number;
  selectedPromptId: string;
  saveName: string;
  /** Extra HTTP headers for this run (merged after provider.headers). */
  extraHeaders: Record<string, string>;
  busy: boolean;
  /** When true, in-flight invoke result will be discarded when it returns. */
  cancelled: boolean;
  runId: string | null;
  liveLines: string[];
  result: TestConnectionResult | null;
  showLog: boolean;
  logTab: "timeline" | "request" | "response";
};

let session: SingleTestSession | null = null;
const globalListeners = new Set<() => void>();
let unlistenLog: UnlistenFn | null = null;
let listenPromise: Promise<void> | null = null;

function notify() {
  for (const l of [...globalListeners]) {
    try {
      l();
    } catch {
      /* ignore */
    }
  }
}

export function getSingleTestSession(): SingleTestSession | null {
  return session;
}

export function subscribeSingleTestSession(listener: () => void): () => void {
  globalListeners.add(listener);
  return () => {
    globalListeners.delete(listener);
  };
}

function newRunId(): string {
  return `run_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 10)}`;
}

async function ensureLogListener(): Promise<void> {
  if (unlistenLog) return;
  if (listenPromise) return listenPromise;
  listenPromise = listen<api.TestConnectionLogEvent>(api.TEST_CONNECTION_LOG_EVENT, (event) => {
    const payload = event.payload;
    if (!payload?.line || !payload.runId) return;
    if (!session || session.runId !== payload.runId) return;
    session.liveLines = [...session.liveLines, payload.line];
    setLastTestResult(session.modelId, session.result?.ok ?? false, session.result?.latencyMs, {
      logs: session.liveLines,
      result: session.result,
      memoryOnly: true,
    });
    notify();
  }).then((fn) => {
    unlistenLog = fn;
    listenPromise = null;
  });
  await listenPromise;
}

export type EnsureSingleArgs = {
  modelId: string;
  providerId: string;
  modelApiId: string;
  providerName: string;
  protocol: string;
  defaultPrompt: string;
  defaultPromptId: string;
};

export function ensureSingleTestSession(args: EnsureSingleArgs): SingleTestSession {
  if (session && session.modelId === args.modelId) {
    if (!session.busy) {
      const last = getLastTestResult(args.modelId);
      if (last?.logs?.length && session.liveLines.length === 0 && session.result == null) {
        session.liveLines = [...last.logs];
        if (last.result) session.result = last.result;
        // Keep showLog as-is (default collapsed); user expands manually.
      }
    }
    return session;
  }
  const last = getLastTestResult(args.modelId);
  session = {
    modelId: args.modelId,
    providerId: args.providerId,
    modelApiId: args.modelApiId,
    providerName: args.providerName,
    protocol: args.protocol,
    prompt: args.defaultPrompt,
    timeoutSecs: 30,
    selectedPromptId: args.defaultPromptId,
    saveName: "",
    extraHeaders: {},
    busy: false,
    cancelled: false,
    runId: null,
    liveLines: last?.logs?.length ? [...last.logs] : [],
    result: last?.result ?? null,
    // Logs panel stays collapsed until the user expands it.
    showLog: false,
    logTab: "timeline",
  };
  return session;
}

/** Soft-stop: free UI immediately; discard in-flight result when the request finishes. */
export function requestStopSingleTest(): void {
  if (!session || !session.busy) return;
  const s = session;
  s.cancelled = true;
  s.liveLines = [
    ...s.liveLines,
    "[ui] stop requested — discarding in-flight result when it returns",
  ];
  // Invalidate run so the late invoke response is ignored.
  s.runId = null;
  s.busy = false;
  notify();
}

export function patchSingleTestSession(
  patch: Partial<
    Pick<
      SingleTestSession,
      | "prompt"
      | "timeoutSecs"
      | "selectedPromptId"
      | "saveName"
      | "extraHeaders"
      | "showLog"
      | "logTab"
    >
  >,
): void {
  if (!session || session.busy) {
    // allow showLog/logTab even when busy
    if (!session) return;
    if (patch.showLog !== undefined) session.showLog = patch.showLog;
    if (patch.logTab !== undefined) session.logTab = patch.logTab;
    notify();
    return;
  }
  Object.assign(session, patch);
  notify();
}

export function clearSingleTestLogs(): void {
  if (!session || session.busy) return;
  const modelId = session.modelId;
  session.liveLines = [];
  session.result = null;
  session.showLog = false;
  session.logTab = "timeline";
  // Also clear shared cache so re-open / re-render does not rehydrate batch logs.
  clearLastTestLogs(modelId);
  notify();
}

export async function runSingleTest(prompt: string, timeoutSecs: number): Promise<void> {
  if (!session || session.busy) return;
  const text = prompt.trim();
  if (!text) throw new Error("提示词不能为空");
  const timeout = Math.min(300, Math.max(5, Math.round(timeoutSecs) || 30));
  const s = session;
  await ensureLogListener();

  const runId = newRunId();
  s.busy = true;
  s.cancelled = false;
  s.prompt = text;
  s.timeoutSecs = timeout;
  s.runId = runId;
  s.result = null;
  s.liveLines = [`[ui] preparing run ${runId}`, "[ui] invoking test_model_connection…"];
  // Do not auto-expand the log panel; respect current showLog / default collapsed.
  s.logTab = "timeline";
  notify();

  try {
    const res = await api.testModelConnection(
      s.modelId,
      text,
      runId,
      timeout,
      s.extraHeaders,
    );
    // Stopped or a newer run replaced this session.
    if (session !== s || s.runId !== runId || s.cancelled) return;
    s.result = res;
    if (res.logs?.length) {
      s.liveLines = [...res.logs];
    } else {
      s.liveLines = [...s.liveLines, "[ui] finished (no backend log array)"];
    }
    setLastTestResult(s.modelId, res.ok, res.latencyMs, {
      logs: s.liveLines,
      result: res,
    });
    if (!res.ok) s.logTab = "response";
  } catch (e) {
    if (session !== s || s.runId !== runId || s.cancelled) return;
    const msg = e instanceof Error ? e.message : String(e);
    s.liveLines = [...s.liveLines, `[ui] invoke error: ${msg}`];
    s.result = {
      ok: false,
      latencyMs: 0,
      httpStatus: null,
      protocol: s.protocol as TestConnectionResult["protocol"],
      requestUrl: "",
      responseText: null,
      error: msg,
      logs: [`invoke error: ${msg}`],
      requestMethod: "POST",
      requestHeaders: [],
      requestBody: null,
      responseHeaders: [],
      responseBody: null,
    };
    setLastTestResult(s.modelId, false, undefined, {
      logs: s.liveLines,
      result: s.result,
    });
    s.logTab = "timeline";
  } finally {
    // Only clear busy for this run if it was not already soft-stopped.
    if (session === s && s.runId === runId) {
      s.busy = false;
      s.cancelled = false;
      notify();
    }
  }
}
