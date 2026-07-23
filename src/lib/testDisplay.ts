/**
 * Shared helpers so list multi-test / detail batch-test / single-test
 * all read the same in-memory outcomes + live multi/batch rows.
 */
import type { Model, TestConnectionResult } from "../types";
import { getLastTestResult } from "./lastTestResults";
import { getBatchTestSession } from "./batchTestSession";
import { getMultiTestSession } from "./multiTestSession";
import { getSingleTestSession } from "./singleTestSession";

export type DisplayTestStatus = "pending" | "running" | "ok" | "fail" | "skipped";

export type ModelTestDisplay = {
  readonly modelId: string;
  readonly status: DisplayTestStatus;
  readonly logs: readonly string[];
  readonly result: TestConnectionResult | null;
  readonly error: string | null;
  readonly latencyMs: number | null;
  readonly source: "multi" | "batch" | "single" | "last" | "none";
};

/** Best-effort live/last view for one model across all test runners. */
export function getModelTestDisplay(modelId: string): ModelTestDisplay {
  // Single-model session: only while busy (or has live lines for this model).
  const single = getSingleTestSession();
  if (single?.modelId === modelId && single.busy) {
    return {
      modelId,
      status: "running",
      logs: single.liveLines,
      result: single.result,
      error: single.result?.error ?? null,
      latencyMs: single.result?.latencyMs ?? null,
      source: "single",
    };
  }

  // Multi session row is authoritative for models in that session
  // (including pending while queue is waiting).
  const multi = getMultiTestSession();
  if (multi) {
    const row = multi.rows.find((r) => r.modelId === modelId);
    if (row) {
      // After a finished multi run, still surface ok/fail/logs from session.
      // For pending: only treat as live pending while busy; otherwise fall through
      // to disk last result (so old finished sessions don't force forever-pending).
      if (multi.busy || row.status !== "pending") {
        return {
          modelId,
          status: row.status,
          logs: row.logs,
          result: row.result,
          error: row.error,
          latencyMs: row.result?.latencyMs ?? null,
          source: "multi",
        };
      }
      // busy=false and pending: session created but not started, or leftover — prefer last
    }
  }

  const batch = getBatchTestSession();
  if (batch) {
    const row = batch.rows.find((r) => r.modelId === modelId);
    if (row) {
      if (batch.busy || row.status !== "pending") {
        return {
          modelId,
          status: row.status,
          logs: row.logs,
          result: row.result,
          error: row.error,
          latencyMs: row.result?.latencyMs ?? null,
          source: "batch",
        };
      }
    }
  }

  const last = getLastTestResult(modelId);
  if (last) {
    return {
      modelId,
      status: last.ok ? "ok" : "fail",
      logs: last.logs,
      result: last.result,
      error: last.result?.error ?? null,
      latencyMs: last.latencyMs ?? last.result?.latencyMs ?? null,
      source: "last",
    };
  }

  return {
    modelId,
    status: "pending",
    logs: [],
    result: null,
    error: null,
    latencyMs: null,
    source: "none",
  };
}

export function buildProviderModelDisplays(
  providerId: string,
  models: readonly Model[],
): Array<{
  modelId: string;
  modelApiId: string;
  displayName: string;
  status: DisplayTestStatus;
  logs: string[];
  result: TestConnectionResult | null;
  error: string | null;
}> {
  return models
    .filter((m) => m.providerId === providerId)
    .map((m) => {
      const d = getModelTestDisplay(m.id);
      return {
        modelId: m.id,
        modelApiId: m.modelId,
        displayName: m.displayName,
        status: d.status,
        logs: [...d.logs],
        result: d.result,
        error: d.error,
      };
    });
}
