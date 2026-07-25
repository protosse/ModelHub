import { useEffect, useMemo, useState } from "react";
import type { FullState, Model, Provider } from "../types";
import { TestConnectionModal } from "../components/TestConnectionModal";
import { MultiProviderTestModal } from "../components/MultiProviderTestModal";
import {
  formatTestedAt,
  getLastTestResult,
  subscribeLastTestResults,
} from "../lib/lastTestResults";
import { getModelTestDisplay, type DisplayTestStatus } from "../lib/testDisplay";
import { subscribeSingleTestSession } from "../lib/singleTestSession";
import { subscribeBatchTestSession } from "../lib/batchTestSession";
import {
  isMultiTestBusy,
  subscribeMultiTestSession,
} from "../lib/multiTestSession";

type Props = {
  readonly state: FullState;
  readonly active: boolean;
  readonly onRefresh: () => Promise<void>;
  readonly onToast: (msg: string) => void;
  readonly onOpenProvider: (providerId: string) => void;
};

type StatusFilter = "all" | "ok" | "fail" | "untested";
type ListSort =
  | "recommend"
  | "latency_asc"
  | "latency_desc"
  | "tested_desc"
  | "name";

type Row = {
  readonly model: Model;
  readonly provider: Provider;
  readonly status: DisplayTestStatus;
  readonly latencyMs: number | null;
  readonly testedAt: string | null;
  readonly source: string;
};

type ConnLabel = "running" | "pending" | "ok" | "fail" | "untested" | "skipped";

function StatusBadge({ label }: { readonly label: ConnLabel }) {
  if (label === "running") {
    return <span className="badge bg-accent/20 text-accent">测试中</span>;
  }
  if (label === "pending") {
    return <span className="badge bg-surface-3 text-ink-3">待测</span>;
  }
  if (label === "ok") {
    return <span className="badge bg-ok/15 text-ok">可用</span>;
  }
  if (label === "fail") {
    return <span className="badge bg-danger/20 text-danger">失败</span>;
  }
  if (label === "skipped") {
    return <span className="badge bg-surface-3 text-ink-3">跳过</span>;
  }
  return <span className="badge bg-surface-3 text-ink-3">未测</span>;
}

function connLabel(r: {
  status: DisplayTestStatus;
  source: string;
  modelId: string;
}): ConnLabel {
  if (r.status === "running") return "running";
  if (r.status === "ok") return "ok";
  if (r.status === "fail") return "fail";
  if (r.status === "skipped") return "skipped";
  if (r.status === "pending" && (r.source === "multi" || r.source === "batch")) {
    return "pending";
  }
  const last = getLastTestResult(r.modelId);
  if (last) return last.ok ? "ok" : "fail";
  return "untested";
}

export function ModelsPage({
  state,
  active,
  onRefresh,
  onToast,
  onOpenProvider,
}: Props) {
  const [q, setQ] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [providerFilter, setProviderFilter] = useState<string>("all");
  const [protocolFilter, setProtocolFilter] = useState<string>("all");
  const [listSort, setListSort] = useState<ListSort>("recommend");
  const [checkedIds, setCheckedIds] = useState<Set<string>>(new Set());
  const [tick, setTick] = useState(0);
  const [testingModelId, setTestingModelId] = useState<string | null>(null);
  const [multiOpen, setMultiOpen] = useState(false);
  const [multiModels, setMultiModels] = useState<Model[]>([]);

  // Shared test sessions + disk summaries (same store as Providers page).
  useEffect(() => {
    if (!active) return;
    const sub = () => setTick((n) => n + 1);
    const u1 = subscribeLastTestResults(sub);
    const u2 = subscribeSingleTestSession(sub);
    const u3 = subscribeBatchTestSession(sub);
    const u4 = subscribeMultiTestSession(sub);
    setTick((n) => n + 1);
    return () => {
      u1();
      u2();
      u3();
      u4();
    };
  }, [active]);

  const providerById = useMemo(() => {
    const m = new Map<string, Provider>();
    for (const p of state.store.providers) m.set(p.id, p);
    return m;
  }, [state.store.providers]);

  const rows: Row[] = useMemo(() => {
    void tick;
    const out: Row[] = [];
    for (const model of state.store.models) {
      const provider = providerById.get(model.providerId);
      if (!provider) continue;
      const d = getModelTestDisplay(model.id);
      const last = getLastTestResult(model.id);
      const latencyMs = d.latencyMs ?? last?.latencyMs ?? null;
      out.push({
        model,
        provider,
        status: d.status,
        latencyMs: typeof latencyMs === "number" ? latencyMs : null,
        testedAt: last?.testedAt ?? null,
        source: d.source,
      });
    }
    return out;
  }, [state.store.models, providerById, tick]);

  // Drop checks for models that disappeared.
  useEffect(() => {
    const alive = new Set(state.store.models.map((m) => m.id));
    setCheckedIds((prev) => {
      let changed = false;
      const next = new Set<string>();
      for (const id of prev) {
        if (alive.has(id)) next.add(id);
        else changed = true;
      }
      return changed ? next : prev;
    });
  }, [state.store.models]);

  const providersSorted = useMemo(() => {
    const list = [...state.store.providers];
    list.sort((a, b) => a.name.localeCompare(b.name, "zh"));
    return list;
  }, [state.store.providers]);

  const filteredRows = useMemo(() => {
    const needle = q.trim().toLowerCase();
    let list = rows.filter((r) => {
      if (providerFilter !== "all" && r.provider.id !== providerFilter) return false;
      if (protocolFilter !== "all" && r.provider.protocol !== protocolFilter) return false;
      if (needle) {
        const hay = `${r.model.modelId} ${r.model.displayName} ${r.provider.name}`.toLowerCase();
        if (!hay.includes(needle)) return false;
      }
      if (statusFilter !== "all") {
        const label = connLabel({
          status: r.status,
          source: r.source,
          modelId: r.model.id,
        });
        if (statusFilter === "ok" && label !== "ok") return false;
        if (statusFilter === "fail" && label !== "fail") return false;
        if (statusFilter === "untested" && label !== "untested") return false;
      }
      return true;
    });

    const rank = (r: Row): number => {
      if (r.status === "running") return 0;
      if (r.status === "ok") return 1;
      if (r.status === "fail") return 3;
      if (r.status === "pending") return 2;
      const last = getLastTestResult(r.model.id);
      if (last?.ok) return 1;
      if (last && !last.ok) return 3;
      return 4; // untested
    };

    const latency = (r: Row): number | null =>
      typeof r.latencyMs === "number" && Number.isFinite(r.latencyMs) ? r.latencyMs : null;

    list = [...list].sort((a, b) => {
      if (listSort === "name") {
        return (
          a.model.modelId.localeCompare(b.model.modelId) ||
          a.provider.name.localeCompare(b.provider.name, "zh")
        );
      }
      if (listSort === "tested_desc") {
        const ta = a.testedAt ? Date.parse(a.testedAt) : 0;
        const tb = b.testedAt ? Date.parse(b.testedAt) : 0;
        return tb - ta || a.model.modelId.localeCompare(b.model.modelId);
      }
      if (listSort === "latency_asc" || listSort === "latency_desc") {
        const la = latency(a);
        const lb = latency(b);
        if (la == null && lb == null) return a.model.modelId.localeCompare(b.model.modelId);
        if (la == null) return 1;
        if (lb == null) return -1;
        return listSort === "latency_asc" ? la - lb : lb - la;
      }
      // recommend: ok first → latency asc → fail → untested
      const ra = rank(a);
      const rb = rank(b);
      if (ra !== rb) return ra - rb;
      if (ra === 1) {
        const la = latency(a);
        const lb = latency(b);
        if (la != null && lb != null && la !== lb) return la - lb;
        if (la == null) return 1;
        if (lb == null) return -1;
      }
      return a.model.modelId.localeCompare(b.model.modelId);
    });

    return list;
  }, [rows, q, statusFilter, providerFilter, protocolFilter, listSort]);

  /** Counts over the search/provider/protocol-filtered set (before status chip). */
  const statusCounts = useMemo(() => {
    const base = rows.filter((r) => {
      if (providerFilter !== "all" && r.provider.id !== providerFilter) return false;
      if (protocolFilter !== "all" && r.provider.protocol !== protocolFilter) return false;
      const needle = q.trim().toLowerCase();
      if (needle) {
        const hay = `${r.model.modelId} ${r.model.displayName} ${r.provider.name}`.toLowerCase();
        if (!hay.includes(needle)) return false;
      }
      return true;
    });
    let ok = 0;
    let fail = 0;
    let untested = 0;
    let running = 0;
    let pending = 0;
    const latencies: number[] = [];
    for (const r of base) {
      const label = connLabel({
        status: r.status,
        source: r.source,
        modelId: r.model.id,
      });
      if (label === "ok") {
        ok += 1;
        if (r.latencyMs != null) latencies.push(r.latencyMs);
      } else if (label === "fail") fail += 1;
      else if (label === "untested") untested += 1;
      else if (label === "running") running += 1;
      else if (label === "pending") pending += 1;
      // skipped counted nowhere special
    }
    latencies.sort((a, b) => a - b);
    const median =
      latencies.length === 0
        ? null
        : latencies.length % 2 === 1
          ? latencies[(latencies.length - 1) / 2]
          : Math.round(
              (latencies[latencies.length / 2 - 1] + latencies[latencies.length / 2]) / 2,
            );
    return {
      total: base.length,
      ok,
      fail,
      untested,
      running,
      pending,
      median,
    };
  }, [rows, q, providerFilter, protocolFilter]);

  const summary = useMemo(() => {
    // Filtered list stats for the action bar (status chip applied).
    let ok = 0;
    let fail = 0;
    let untested = 0;
    let inFlight = 0;
    const latencies: number[] = [];
    for (const r of filteredRows) {
      const label = connLabel({
        status: r.status,
        source: r.source,
        modelId: r.model.id,
      });
      if (label === "ok") {
        ok += 1;
        if (r.latencyMs != null) latencies.push(r.latencyMs);
      } else if (label === "fail") fail += 1;
      else if (label === "untested") untested += 1;
      else if (label === "running" || label === "pending") inFlight += 1;
    }
    latencies.sort((a, b) => a - b);
    const median =
      latencies.length === 0
        ? null
        : latencies.length % 2 === 1
          ? latencies[(latencies.length - 1) / 2]
          : Math.round(
              (latencies[latencies.length / 2 - 1] + latencies[latencies.length / 2]) / 2,
            );
    return { total: filteredRows.length, ok, fail, untested, inFlight, median };
  }, [filteredRows]);

  const allFilteredChecked =
    filteredRows.length > 0 && filteredRows.every((r) => checkedIds.has(r.model.id));

  const toggleCheckAllFiltered = () => {
    setCheckedIds((prev) => {
      const next = new Set(prev);
      if (allFilteredChecked) {
        for (const r of filteredRows) next.delete(r.model.id);
      } else {
        for (const r of filteredRows) next.add(r.model.id);
      }
      return next;
    });
  };

  const toggleCheck = (id: string) => {
    setCheckedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const openMultiForModels = (models: Model[]) => {
    if (models.length === 0) {
      onToast("没有可测的模型");
      return;
    }
    if (isMultiTestBusy()) {
      onToast("已有批量测试进行中，请先停止或等待完成");
      setMultiModels(models);
      setMultiOpen(true);
      return;
    }
    setMultiModels(models);
    setMultiOpen(true);
  };

  const testingModel = testingModelId
    ? state.store.models.find((m) => m.id === testingModelId) ?? null
    : null;
  const testingProvider = testingModel
    ? providerById.get(testingModel.providerId) ?? null
    : null;

  const multiProviders = useMemo(() => {
    const ids = new Set(multiModels.map((m) => m.providerId));
    return state.store.providers.filter((p) => ids.has(p.id));
  }, [multiModels, state.store.providers]);

  const statusChips: readonly { id: StatusFilter; label: string; count: number }[] = [
    { id: "all", label: "全部", count: statusCounts.total },
    { id: "ok", label: "可用", count: statusCounts.ok },
    { id: "fail", label: "失败", count: statusCounts.fail },
    { id: "untested", label: "未测", count: statusCounts.untested },
  ];

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <div className="flex flex-wrap items-end gap-2">
        <div className="min-w-[12rem] flex-1">
          <label className="label">搜索</label>
          <input
            className="input"
            placeholder="Model ID / 展示名 / 提供商"
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
        </div>
        <div>
          <label className="label">提供商</label>
          <select
            className="input max-w-[12rem]"
            value={providerFilter}
            onChange={(e) => setProviderFilter(e.target.value)}
          >
            <option value="all">全部</option>
            {providersSorted.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="label">协议</label>
          <select
            className="input"
            value={protocolFilter}
            onChange={(e) => setProtocolFilter(e.target.value)}
          >
            <option value="all">全部</option>
            <option value="openai-completions">openai-completions</option>
            <option value="openai-responses">openai-responses</option>
            <option value="anthropic-messages">anthropic-messages</option>
          </select>
        </div>
        <div>
          <label className="label">排序</label>
          <select
            className="input"
            value={listSort}
            onChange={(e) => setListSort(e.target.value as ListSort)}
          >
            <option value="recommend">默认（可用优先 · 快→慢）</option>
            <option value="latency_asc">响应时间 ↑</option>
            <option value="latency_desc">响应时间 ↓</option>
            <option value="tested_desc">最近测试</option>
            <option value="name">Model ID</option>
          </select>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-1.5">
        {statusChips.map((c) => {
          const on = statusFilter === c.id;
          return (
            <button
              key={c.id}
              type="button"
              className={
                on
                  ? "rounded-full border border-accent/40 bg-accent/15 px-2.5 py-1 text-xs font-medium text-accent"
                  : "rounded-full border border-surface-3 bg-surface-1 px-2.5 py-1 text-xs text-ink-2 hover:border-surface-3 hover:bg-surface-2"
              }
              onClick={() => setStatusFilter(c.id)}
            >
              {c.label} {c.count}
            </button>
          );
        })}
        {statusCounts.running + statusCounts.pending > 0 ? (
          <span className="ml-1 text-xs text-accent">
            测试中 {statusCounts.running}
            {statusCounts.pending > 0 ? ` · 排队 ${statusCounts.pending}` : ""}
          </span>
        ) : null}
        {statusCounts.median != null ? (
          <span className="text-xs text-ink-3">
            · 可用中位 {statusCounts.median} ms
          </span>
        ) : null}
      </div>

      <div className="flex flex-wrap items-center gap-2 text-sm">
        <span className="text-ink-3">列表 {summary.total}</span>
        <span className="text-ink-3">|</span>
        <label className="flex items-center gap-2 text-ink-2">
          <input
            type="checkbox"
            checked={allFilteredChecked}
            onChange={toggleCheckAllFiltered}
            disabled={filteredRows.length === 0}
          />
          全选当前
        </label>
        <span className="text-ink-3">已选 {checkedIds.size}</span>
        <button
          type="button"
          className="btn-secondary !py-1 text-xs"
          disabled={filteredRows.length === 0}
          title="对当前筛选结果批量连通性测试（全局并发 3，同提供商串行）"
          onClick={() => openMultiForModels(filteredRows.map((r) => r.model))}
        >
          测当前列表
        </button>
        <button
          type="button"
          className="btn-secondary !py-1 text-xs"
          disabled={checkedIds.size === 0}
          title="对勾选模型批量测试"
          onClick={() => {
            const models = state.store.models.filter((m) => checkedIds.has(m.id));
            openMultiForModels(models);
          }}
        >
          测所选
        </button>
      </div>

      <div className="card min-h-0 flex-1 overflow-auto">
        {state.store.models.length === 0 ? (
          <div className="p-8 text-center text-sm text-ink-3">
            库中暂无模型。请到「提供商」添加，或从「导入」写入。
          </div>
        ) : filteredRows.length === 0 ? (
          <div className="p-8 text-center text-sm text-ink-3">无匹配模型</div>
        ) : (
          <table className="w-full table-fixed text-left text-sm">
            <colgroup>
              <col className="w-[3%]" />
              <col className="w-[9%]" />
              <col className="w-[9%]" />
              <col className="w-[14%]" />
              <col className="w-[18%]" />
              <col className="w-[12%]" />
              <col className="w-[12%]" />
              <col className="w-[13%]" />
              <col className="w-[10%]" />
            </colgroup>
            <thead className="sticky top-0 z-10 bg-surface-1 text-xs text-ink-3">
              <tr>
                <th className="px-2 py-2 font-medium" />
                <th className="px-2 py-2 font-medium">状态</th>
                <th className="px-2 py-2 font-medium">响应</th>
                <th className="px-2 py-2 font-medium">最近测试</th>
                <th className="px-2 py-2 font-medium">Model ID</th>
                <th className="px-2 py-2 font-medium">展示名</th>
                <th className="px-2 py-2 font-medium">提供商</th>
                <th className="px-2 py-2 font-medium">协议</th>
                <th className="px-2 py-2 font-medium" />
              </tr>
            </thead>
            <tbody>
              {filteredRows.map((r) => {
                const tip = r.testedAt
                  ? `测试时间：${formatTestedAt(r.testedAt)}${
                      r.latencyMs != null ? ` · ${r.latencyMs} ms` : ""
                    }`
                  : r.latencyMs != null
                    ? `${r.latencyMs} ms`
                    : undefined;
                return (
                  <tr key={r.model.id} className="border-t border-surface-3">
                    <td className="px-2 py-2">
                      <input
                        type="checkbox"
                        checked={checkedIds.has(r.model.id)}
                        onChange={() => toggleCheck(r.model.id)}
                        aria-label={`选择 ${r.model.modelId}`}
                      />
                    </td>
                    <td className="px-2 py-2">
                      <span title={tip}>
                        <StatusBadge
                          label={connLabel({
                            status: r.status,
                            source: r.source,
                            modelId: r.model.id,
                          })}
                        />
                      </span>
                    </td>
                    <td className="px-2 py-2 tabular-nums text-xs text-ink-2">
                      {r.latencyMs != null ? `${r.latencyMs} ms` : "—"}
                    </td>
                    <td className="truncate px-2 py-2 text-xs text-ink-3" title={r.testedAt ?? undefined}>
                      {r.testedAt ? formatTestedAt(r.testedAt) : "—"}
                    </td>
                    <td className="truncate px-2 py-2 font-mono text-xs" title={r.model.modelId}>
                      {r.model.modelId}
                    </td>
                    <td className="truncate px-2 py-2 text-xs" title={r.model.displayName}>
                      {r.model.displayName || "—"}
                    </td>
                    <td className="truncate px-2 py-2">
                      <button
                        type="button"
                        className="max-w-full truncate text-left text-xs text-accent hover:underline"
                        title={`在提供商页打开「${r.provider.name}」`}
                        onClick={() => onOpenProvider(r.provider.id)}
                      >
                        {r.provider.name}
                      </button>
                    </td>
                    <td className="truncate px-2 py-2 font-mono text-[11px] text-ink-3" title={r.provider.protocol}>
                      {r.provider.protocol}
                    </td>
                    <td className="px-2 py-2 text-right">
                      <button
                        type="button"
                        className="btn-secondary !px-2 !py-1 text-xs"
                        onClick={() => setTestingModelId(r.model.id)}
                      >
                        {r.status === "running" ? "测试中" : "测试"}
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {testingModel && testingProvider ? (
        <TestConnectionModal
          provider={testingProvider}
          model={testingModel}
          prompts={state.store.testPrompts ?? []}
          onClose={() => setTestingModelId(null)}
          onPromptsChanged={onRefresh}
          onToast={onToast}
        />
      ) : null}

      {multiOpen ? (
        <MultiProviderTestModal
          providers={multiProviders}
          models={multiModels}
          prompts={state.store.testPrompts ?? []}
          onClose={() => setMultiOpen(false)}
          onPromptsChanged={onRefresh}
          onToast={onToast}
        />
      ) : null}
    </div>
  );
}
