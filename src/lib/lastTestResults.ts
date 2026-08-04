/** Last connectivity test outcome per model row id (memory + store.json).
 *  Also keeps last logs/result in memory so single/batch/multi UIs share them.
 */

import * as api from "../api/tauri";
import type { ModelTestResult, TestConnectionResult } from "../types";
import { shouldKeepInMemoryOutcome } from "./testResultPolicy";

export type LastTestOutcome = {
  readonly ok: boolean;
  /** ISO-8601 when the test finished (preferred for display). */
  readonly testedAt: string;
  readonly latencyMs?: number;
  /** In-memory only: last request log lines (not persisted to disk). */
  readonly logs: readonly string[];
  /** In-memory only: full result when available. */
  readonly result: TestConnectionResult | null;
};

const results = new Map<string, LastTestOutcome>();
const listeners = new Set<() => void>();

function notify() {
  for (const l of [...listeners]) {
    try {
      l();
    } catch {
      /* ignore */
    }
  }
}

function toIso(ms: number): string {
  try {
    return new Date(ms).toISOString();
  } catch {
    return new Date().toISOString();
  }
}

export function getLastTestResult(modelId: string): LastTestOutcome | null {
  return results.get(modelId) ?? null;
}

/** Restore the exact finished outcome captured before a cancelled run. */
export function restoreLastTestResult(
  modelId: string,
  outcome: LastTestOutcome | null,
): void {
  if (outcome) {
    results.set(modelId, outcome);
  } else {
    results.delete(modelId);
  }
  notify();
}

/** Seed / refresh cache from get_state (store.modelTestResults). Keeps in-memory logs if present. */
export function hydrateLastTestResults(
  map: Readonly<Record<string, ModelTestResult>> | null | undefined,
): void {
  const previous = new Map(results);
  results.clear();
  if (map) {
    for (const [id, r] of Object.entries(map)) {
      if (!r || typeof r.ok !== "boolean") continue;
      const inMemory = previous.get(id);
      const diskTestedAt = r.testedAt || new Date().toISOString();
      const keepMemoryOutcome = shouldKeepInMemoryOutcome(
        inMemory?.testedAt,
        diskTestedAt,
      );
      const memorySummary = keepMemoryOutcome ? inMemory : undefined;
      results.set(id, {
        ok: memorySummary?.ok ?? r.ok,
        testedAt: memorySummary?.testedAt ?? diskTestedAt,
        latencyMs: memorySummary
          ? memorySummary.latencyMs
          : (r.latencyMs ?? undefined),
        logs: inMemory?.logs ?? [],
        result: inMemory?.result ?? null,
      });
    }
  }
  // Preserve in-memory entries not yet on disk, including a just-finished
  // outcome whose fire-and-forget persistence command is still queued.
  for (const [id, inMemory] of previous) {
    if (!results.has(id) && (inMemory.logs.length || inMemory.result)) {
      results.set(id, inMemory);
    }
  }
  notify();
}

export type SetLastTestArgs = {
  modelId: string;
  ok: boolean;
  latencyMs?: number;
  logs?: readonly string[];
  result?: TestConnectionResult | null;
  testedAt?: string;
  /** When true, only update memory logs/result without disk write (e.g. live progress). */
  memoryOnly?: boolean;
};

/**
 * Update local cache immediately; persist ok/testedAt/latency to store.json unless memoryOnly.
 */
export function setLastTestResult(
  modelId: string,
  ok: boolean,
  latencyMs?: number,
  extra?: {
    logs?: readonly string[];
    result?: TestConnectionResult | null;
    testedAt?: string;
    memoryOnly?: boolean;
  },
): void {
  const prev = results.get(modelId);
  const testedAt = extra?.testedAt ?? new Date().toISOString();
  const logs =
    extra?.logs !== undefined ? [...extra.logs] : prev?.logs ? [...prev.logs] : [];
  const result = extra?.result !== undefined ? extra.result : (prev?.result ?? null);
  results.set(modelId, {
    ok,
    testedAt,
    latencyMs: latencyMs ?? prev?.latencyMs,
    logs,
    result,
  });
  notify();
  if (extra?.memoryOnly) return;
  void api
    .recordModelTestResult(modelId, ok, latencyMs ?? null, testedAt)
    .catch(() => {
      /* ignore persist errors in UI path; cache still shows latest */
    });
}

/** Append a live log line for a model (in-memory only). */
export function appendLastTestLog(modelId: string, line: string): void {
  const prev = results.get(modelId);
  const logs = [...(prev?.logs ?? []), line];
  results.set(modelId, {
    ok: prev?.ok ?? false,
    testedAt: prev?.testedAt ?? new Date().toISOString(),
    latencyMs: prev?.latencyMs,
    logs,
    result: prev?.result ?? null,
  });
  notify();
}

/** Replace live logs while a run is in progress (in-memory). */
export function setLastTestLogs(modelId: string, logs: readonly string[]): void {
  const prev = results.get(modelId);
  results.set(modelId, {
    ok: prev?.ok ?? false,
    testedAt: prev?.testedAt ?? new Date().toISOString(),
    latencyMs: prev?.latencyMs,
    logs: [...logs],
    result: prev?.result ?? null,
  });
  notify();
}

export function clearLastTestLogs(modelId: string): void {
  const prev = results.get(modelId);
  if (!prev) return;
  if (!prev.logs.length && !prev.result) return;
  results.set(modelId, {
    ok: prev.ok,
    testedAt: prev.testedAt,
    latencyMs: prev.latencyMs,
    logs: [],
    result: null,
  });
  notify();
}

export function removeLastTestResult(modelId: string): void {
  if (!results.has(modelId)) return;
  results.delete(modelId);
  notify();
}

export function subscribeLastTestResults(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Format testedAt for tooltip (local time). */
export function formatTestedAt(testedAt: string): string {
  const d = new Date(testedAt);
  if (Number.isNaN(d.getTime())) return testedAt;
  try {
    return d.toLocaleString(undefined, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    });
  } catch {
    return d.toISOString();
  }
}

export { toIso };
