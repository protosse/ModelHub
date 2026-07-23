import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { TestConnectionResult } from "../types";
import * as api from "../api/tauri";
import { getLastTestResult, setLastTestResult } from "./lastTestResults";

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
  busy: boolean;
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

/** Resume existing session for this model, or create a fresh shell. */
export function ensureSingleTestSession(args: EnsureSingleArgs): SingleTestSession {
  if (session && session.modelId === args.modelId) {
    // Refresh from shared cache if we have no live run and cache is richer
    if (!session.busy) {
      const last = getLastTestResult(args.modelId);
      if (last?.logs?.length && session.liveLines.length === 0) {
        session.liveLines = [...last.logs];
        if (last.result) session.result = last.result;
      }
    }
    return session;
  }
  // Switching to another model: leave previous request running if busy.
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
    busy: false,
    runId: null,
    liveLines: last?.logs?.length ? [...last.logs] : [],
    result: last?.result ?? null,
    showLog: Boolean(last?.logs?.length),
    logTab: "timeline",
  };
  // Do not notify here — may run during React render.
  return session;
}

export function patchSingleTestSession(
  patch: Partial<
    Pick<
      SingleTestSession,
      "prompt" | "timeoutSecs" | "selectedPromptId" | "saveName" | "showLog" | "logTab"
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
  session.liveLines = [];
  session.result = null;
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
  s.prompt = text;
  s.timeoutSecs = timeout;
  s.runId = runId;
  s.result = null;
  s.liveLines = [`[ui] preparing run ${runId}`, "[ui] invoking test_model_connection…"];
  s.showLog = true;
  s.logTab = "timeline";
  notify();

  try {
    const res = await api.testModelConnection(s.modelId, text, runId, timeout);
    if (session !== s || s.runId !== runId) return;
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
    if (session !== s || s.runId !== runId) return;
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
    if (session === s && s.runId === runId) {
      s.busy = false;
      notify();
    }
  }
}
