import { useMemo, useState } from "react";
import * as api from "../api/tauri";
import type {
  AgentBindings,
  Protocol,
  QuickAddAgent,
  QuickAddResult,
  RemoteModel,
} from "../types";
import { PROTOCOLS } from "../types";
import { Modal } from "./Modal";

type Props = {
  readonly bindings: AgentBindings | null;
  readonly onClose: () => void;
  readonly onCommitted: (result: QuickAddResult) => Promise<void>;
};

const AGENTS: readonly { id: QuickAddAgent; label: string }[] = [
  { id: "claude", label: "Claude Code" },
  { id: "codex", label: "Codex" },
  { id: "opencode", label: "OpenCode" },
  { id: "pi", label: "Pi" },
];

const SELECTED_AGENTS_KEY = "modelhub.quickAdd.agents";
const VALID_AGENT_IDS = new Set<QuickAddAgent>(AGENTS.map((item) => item.id));

function readSelectedAgents(): ReadonlySet<QuickAddAgent> {
  try {
    const raw = localStorage.getItem(SELECTED_AGENTS_KEY);
    if (!raw) return new Set();
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return new Set();
    return new Set(
      parsed.filter((item): item is QuickAddAgent => VALID_AGENT_IDS.has(item as QuickAddAgent)),
    );
  } catch {
    return new Set();
  }
}

export function QuickAddProviderModal({ bindings, onClose, onCommitted }: Props) {
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [protocol, setProtocol] = useState<Protocol>("openai-completions");
  const [apiKey, setApiKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [notes, setNotes] = useState("");
  const [remoteModels, setRemoteModels] = useState<readonly RemoteModel[]>([]);
  const [manualModels, setManualModels] = useState<readonly RemoteModel[]>([]);
  const [fetched, setFetched] = useState(false);
  const [fetching, setFetching] = useState(false);
  const [query, setQuery] = useState("");
  const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(() => new Set());
  const [defaultModelId, setDefaultModelId] = useState("");
  const [manualId, setManualId] = useState("");
  const [agents, setAgents] = useState<ReadonlySet<QuickAddAgent>>(readSelectedAgents);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [result, setResult] = useState<QuickAddResult | null>(null);

  const candidates = useMemo(() => {
    const byId = new Map<string, RemoteModel>();
    for (const model of remoteModels) byId.set(model.id, model);
    for (const model of manualModels) byId.set(model.id, model);
    return [...byId.values()].sort((a, b) => a.id.localeCompare(b.id));
  }, [manualModels, remoteModels]);

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return candidates;
    return candidates.filter(
      (model) =>
        model.id.toLowerCase().includes(needle) ||
        model.name.toLowerCase().includes(needle),
    );
  }, [candidates, query]);

  const selectedModels = candidates.filter((model) => selectedIds.has(model.id));
  const allFilteredSelected =
    filtered.length > 0 && filtered.every((model) => selectedIds.has(model.id));

  const updateSelected = (next: Set<string>) => {
    setSelectedIds(next);
    if (!next.has(defaultModelId)) setDefaultModelId(next.values().next().value ?? "");
  };

  const toggleModel = (id: string) => {
    const next = new Set(selectedIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    updateSelected(next);
  };

  const toggleFiltered = () => {
    const next = new Set(selectedIds);
    if (allFilteredSelected) {
      for (const model of filtered) next.delete(model.id);
    } else {
      for (const model of filtered) next.add(model.id);
    }
    updateSelected(next);
  };

  const clearFetchedModels = () => {
    if (!fetched) return;
    setRemoteModels([]);
    setFetched(false);
    const manualIds = new Set(manualModels.map((model) => model.id));
    updateSelected(new Set([...selectedIds].filter((id) => manualIds.has(id))));
  };

  const fetchModels = async () => {
    if (!baseUrl.trim() || fetching) return;
    setFetching(true);
    setErr(null);
    try {
      const models = await api.fetchModelsFromProviderInput({
        baseUrl: baseUrl.trim(),
        protocol,
        apiKey: apiKey.trim(),
      });
      setRemoteModels(models);
      setFetched(true);
      const known = new Set([...models, ...manualModels].map((model) => model.id));
      const next = new Set([...selectedIds].filter((id) => known.has(id)));
      updateSelected(next);
    } catch (e) {
      setErr(`获取模型失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setFetching(false);
    }
  };

  const addManualModel = () => {
    const id = manualId.trim();
    if (!id) return;
    if (!candidates.some((model) => model.id === id)) {
      setManualModels((prev) => [...prev, { id, name: id }]);
    }
    const next = new Set(selectedIds);
    next.add(id);
    updateSelected(next);
    setManualId("");
  };

  const toggleAgent = (id: QuickAddAgent) => {
    setAgents((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      try {
        localStorage.setItem(SELECTED_AGENTS_KEY, JSON.stringify([...next]));
      } catch {}
      return next;
    });
  };

  const submit = async () => {
    setErr(null);
    if (!name.trim() || !baseUrl.trim() || !apiKey.trim()) {
      setErr("名称、Base URL 与 API Key 必填");
      return;
    }
    if (selectedModels.length === 0) {
      setErr("请至少添加一个模型");
      return;
    }
    if (!defaultModelId || !selectedIds.has(defaultModelId)) {
      setErr("请选择默认模型");
      return;
    }
    if (agents.size === 0) {
      setErr("请至少选择一个 Agent");
      return;
    }

    setBusy(true);
    try {
      const response = await api.quickAddAndApply({
        provider: {
          name: name.trim(),
          baseUrl: baseUrl.trim(),
          protocol,
          apiKey: apiKey.trim(),
          enabled: true,
          notes,
        },
        models: selectedModels.map((model) => ({
          modelId: model.id,
          displayName: model.name || model.id,
        })),
        defaultModelId,
        agents: [...agents],
        bindings,
      });
      setResult(response);
      await onCommitted(response);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  if (result) {
    const failed = result.apply.results.filter((item) => !item.ok);
    return (
      <Modal onClose={onClose} xwide>
        <h3 className="text-base font-semibold">快速添加结果</h3>
        <p className="mt-1 text-sm text-ink-2">
          已保存 {result.provider.name} 和 {result.models.length} 个模型。
        </p>
        <div className="mt-4 divide-y divide-surface-3 rounded-md border border-surface-3">
          {result.apply.results.map((item) => (
            <div key={item.agent} className="flex gap-3 px-3 py-2 text-sm">
              <span className={item.ok ? "text-ok" : "text-danger"}>
                {item.ok ? "成功" : "失败"}
              </span>
              <div className="min-w-0">
                <div className="font-medium">{item.agent}</div>
                <div className="break-words text-xs text-ink-3">{item.message}</div>
              </div>
            </div>
          ))}
        </div>
        {failed.length > 0 ? (
          <p className="mt-3 text-sm text-danger">
            {failed.length} 个 Agent 应用失败；Provider 和模型已保存在 ModelHub，可前往 Agent 应用页重试。
          </p>
        ) : null}
        <div className="mt-5 flex justify-end">
          <button type="button" className="btn-primary" onClick={onClose}>完成</button>
        </div>
      </Modal>
    );
  }

  return (
    <Modal onClose={() => !busy && onClose()} xwide>
      <h3 className="text-base font-semibold">快速添加提供商</h3>
      <div className="mt-3 grid grid-cols-2 gap-3">
        <div>
          <label className="label">名称</label>
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
        </div>
        <div>
          <label className="label">协议</label>
          <select
            className="input"
            value={protocol}
            onChange={(e) => {
              clearFetchedModels();
              setProtocol(e.target.value as Protocol);
            }}
          >
            {PROTOCOLS.map((item) => <option key={item} value={item}>{item}</option>)}
          </select>
        </div>
        <div className="col-span-2">
          <label className="label">Base URL</label>
          <input
            className="input min-w-0 font-mono"
            value={baseUrl}
            onChange={(e) => {
              clearFetchedModels();
              setBaseUrl(e.target.value);
            }}
            placeholder="https://api.example.com"
          />
        </div>
        <div className="col-span-2">
          <label className="label">API Key</label>
          <div className="flex gap-2">
            <input
              className="input min-w-0 font-mono"
              type={showKey ? "text" : "password"}
              value={apiKey}
              onChange={(e) => {
                clearFetchedModels();
                setApiKey(e.target.value);
              }}
            />
            <button type="button" className="btn-secondary shrink-0" onClick={() => setShowKey((v) => !v)}>
              {showKey ? "隐藏" : "显示"}
            </button>
            <button
              type="button"
              className="btn-secondary shrink-0"
              disabled={!baseUrl.trim() || fetching}
              onClick={() => void fetchModels()}
            >
              {fetching ? "获取中…" : "获取模型"}
            </button>
          </div>
        </div>
        <div className="col-span-2">
          <label className="label">备注</label>
          <textarea className="input min-h-10" value={notes} onChange={(e) => setNotes(e.target.value)} />
        </div>
      </div>

      <div className="mt-4 border-t border-surface-3 pt-3">
        <div className="flex items-center justify-between gap-3">
          <h4 className="text-sm font-medium">添加模型</h4>
          <span className="text-xs text-ink-3">已选 {selectedIds.size}</span>
        </div>
        <div className="mt-2 flex gap-2">
          <input
            className="input min-w-0 font-mono"
            value={manualId}
            onChange={(e) => setManualId(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                addManualModel();
              }
            }}
            placeholder="手动输入 Model ID"
          />
          <button type="button" className="btn-secondary shrink-0" disabled={!manualId.trim()} onClick={addManualModel}>
            添加
          </button>
        </div>
        {candidates.length > 0 ? (
          <>
            <div className="mt-2 flex items-center gap-2">
              <input
                className="input !py-1.5 text-sm"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="搜索 Model ID / 名称"
              />
              <button type="button" className="btn-secondary shrink-0 !py-1.5 text-xs" onClick={toggleFiltered}>
                {allFilteredSelected ? "取消当前" : "全选当前"}
              </button>
            </div>
            <div className="mt-2 max-h-36 overflow-auto rounded-md border border-surface-3">
              {filtered.map((model) => (
                <label key={model.id} className="flex items-center gap-2 border-b border-surface-3 px-3 py-2 text-sm last:border-b-0 hover:bg-surface-2">
                  <input type="checkbox" checked={selectedIds.has(model.id)} onChange={() => toggleModel(model.id)} />
                  <span className="min-w-0 flex-1 truncate font-mono text-xs">{model.id}</span>
                  {model.name !== model.id ? <span className="max-w-48 truncate text-xs text-ink-3">{model.name}</span> : null}
                </label>
              ))}
            </div>
          </>
        ) : fetched ? (
          <p className="mt-2 text-sm text-ink-3">接口未返回模型，可手动添加 Model ID。</p>
        ) : (
          <p className="mt-2 text-sm text-ink-3">填写 Base URL 后可获取模型，也可手动添加。</p>
        )}
        {selectedModels.length > 0 ? (
          <div className="mt-3">
            <label className="label">默认模型</label>
            <select className="input font-mono" value={defaultModelId} onChange={(e) => setDefaultModelId(e.target.value)}>
              {selectedModels.map((model) => <option key={model.id} value={model.id}>{model.id}</option>)}
            </select>
          </div>
        ) : null}
      </div>

      <div className="mt-4 border-t border-surface-3 pt-3">
        <h4 className="text-sm font-medium">应用到 Agent</h4>
        <div className="mt-2 grid grid-cols-2 gap-2">
          {AGENTS.map((agent) => (
            <label key={agent.id} className="flex items-center gap-2 rounded-md border border-surface-3 px-3 py-1.5 text-sm hover:bg-surface-2">
              <input type="checkbox" checked={agents.has(agent.id)} onChange={() => toggleAgent(agent.id)} />
              <span>{agent.label}</span>
              {agent.id === "codex" && protocol !== "openai-responses" ? (
                <span className="ml-auto text-[11px] text-warn">建议 responses</span>
              ) : null}
            </label>
          ))}
        </div>
      </div>

      {err ? <div className="mt-3 text-sm text-danger">{err}</div> : null}
      <div className="sticky bottom-0 -mx-5 -mb-5 mt-4 flex justify-end gap-2 border-t border-surface-3 bg-surface-1 px-5 py-3">
        <button type="button" className="btn-secondary" disabled={busy} onClick={onClose}>取消</button>
        <button type="button" className="btn-primary" disabled={busy} onClick={() => void submit()}>
          {busy ? "保存并应用中…" : "保存并应用"}
        </button>
      </div>
    </Modal>
  );
}
