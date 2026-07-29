/**
 * Shared helpers so list multi-test / detail batch-test / single-test
 * all read the same in-memory outcomes + live multi/batch rows.
 */
import type { Model, TestConnectionResult } from "../types";
import { getLastTestResult } from "./lastTestResults";
import { getBatchTestSession } from "./batchTestSession";
import { getMultiTestSession } from "./multiTestSession";
import { getSingleTestSession } from "./singleTestSession";
import { pickTestDisplaySource } from "./testDisplayPolicy";

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
  const single = getSingleTestSession();
  const multi = getMultiTestSession();
  const multiRow = multi?.rows.find((r) => r.modelId === modelId);
  const batch = getBatchTestSession();
  const batchRow = batch?.rows.find((r) => r.modelId === modelId);
  const last = getLastTestResult(modelId);
  const source = pickTestDisplaySource({
    singleRunning: Boolean(single?.modelId === modelId && single.busy),
    multiRunning: Boolean(multi?.busy && multiRow),
    batchRunning: Boolean(batch?.busy && batchRow),
    hasLast: Boolean(last),
  });

  if (source === "single" && single) {
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

  // A running multi session is authoritative for its rows. Once it finishes,
  // use `lastTestResults`: a newer single/batch run may already have replaced
  // the outcome, and completed session rows must not shadow that shared value.
  if (source === "multi" && multiRow) {
    return {
      modelId,
      status: multiRow.status,
      logs: multiRow.logs,
      result: multiRow.result,
      error: multiRow.error,
      latencyMs: multiRow.result?.latencyMs ?? null,
      source: "multi",
    };
  }

  if (source === "batch" && batchRow) {
    return {
      modelId,
      status: batchRow.status,
      logs: batchRow.logs,
      result: batchRow.result,
      error: batchRow.error,
      latencyMs: batchRow.result?.latencyMs ?? null,
      source: "batch",
    };
  }

  if (source === "last" && last) {
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
