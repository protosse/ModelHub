import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  AgentBindings,
  ApplyAgentResult,
  CatalogEntry,
  FullState,
  Model,
  Provider,
} from "../types";
import { emptyBindings } from "../types";
import * as api from "../api/tauri";
import type { ApplyPreview } from "../api/tauri";

type AgentId = "claude" | "codex" | "opencode" | "pi";

type Props = {
  readonly state: FullState;
  readonly draft: AgentBindings | null;
  readonly onDraftChange: (draft: AgentBindings | null) => void;
  readonly active: boolean;
  readonly onRefresh: () => Promise<void>;
  readonly onToast: (msg: string) => void;
};

const AGENTS: readonly {
  readonly id: AgentId;
  readonly label: string;
  readonly kind: "active-only" | "catalog";
}[] = [
  { id: "claude", label: "Claude Code", kind: "active-only" },
  { id: "codex", label: "Codex", kind: "active-only" },
  { id: "opencode", label: "OpenCode", kind: "catalog" },
  { id: "pi", label: "Pi", kind: "catalog" },
] as const;

function agentPath(state: FullState, id: AgentId): { path: string; exists: boolean } {
  switch (id) {
    case "claude":
      return { path: state.paths.claudeSettings, exists: state.paths.claudeExists };
    case "codex":
      return { path: state.paths.codexConfig, exists: state.paths.codexExists };
    case "opencode":
      return { path: state.paths.opencodeConfig, exists: state.paths.opencodeExists };
    case "pi":
      return { path: state.paths.piModels, exists: state.paths.piExists };
  }
}

/** An agent is "changed" if its diff has any non-`same` line. */
function agentChanged(preview: ApplyPreview | null, id: AgentId): boolean {
  const d = preview?.agents.find((a) => a.agent === id);
  if (!d) return false;
  return d.lines.some((l) => l.kind !== "same");
}

export function AgentWorkbenchPage({
  state,
  draft,
  onDraftChange,
  active,
  onRefresh,
  onToast,
}: Props) {
  const [selected, setSelected] = useState<AgentId>("claude");
  const [bindings, setBindings] = useState<AgentBindings>(draft ?? emptyBindings());
  const [loading, setLoading] = useState(!draft);
  const bootstrapped = useRef(draft !== null);

  const [preview, setPreview] = useState<ApplyPreview | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const [busy, setBusy] = useState<null | "one" | "all">(null);
  const [results, setResults] = useState<Readonly<Record<string, ApplyAgentResult>>>({});

  const onToastRef = useRef(onToast);
  onToastRef.current = onToast;

  const loadFromDisk = useCallback(async () => {
    setLoading(true);
    try {
      const live = await api.readLiveBindings();
      setBindings(live);
      onDraftChange(live);
      bootstrapped.current = true;
    } catch (e) {
      setPreview(null);
      onToastRef.current(e instanceof Error ? e.message : String(e));
      const empty = emptyBindings();
      setBindings(empty);
      onDraftChange(empty);
      bootstrapped.current = true;
    } finally {
      setLoading(false);
    }
  }, [onDraftChange]);

  // Initial disk load only once when there is no session draft.
  useEffect(() => {
    if (draft) {
      setBindings(draft);
      setLoading(false);
      bootstrapped.current = true;
      return;
    }
    if (!bootstrapped.current) void loadFromDisk();
  }, [draft, loadFromDisk]);

  const patch = (next: AgentBindings) => {
    setBindings(next);
    onDraftChange(next);
  };

  const providers = state.store.providers;
  const modelsOf = useCallback(
    (providerId: string | null): readonly Model[] =>
      providerId
        ? state.store.models.filter((m) => m.providerId === providerId)
        : [],
    [state.store.models],
  );

  // Preview all four agents in one call; drives both status dots and detail diff.
  const loadPreview = useCallback(async (b: AgentBindings) => {
    setPreviewBusy(true);
    try {
      const p = await api.previewApply([], b);
      setPreview(p);
    } catch (e) {
      setPreview(null);
      onToastRef.current(e instanceof Error ? e.message : String(e));
    } finally {
      setPreviewBusy(false);
    }
  }, []);

  // Re-preview when draft or store data (providers/models/catalogs) change,
  // but only while this page is visible (avoid churn behind other tabs).
  useEffect(() => {
    if (loading || !active) return;
    void loadPreview(bindings);
  }, [
    loading,
    active,
    bindings,
    state.store.providers,
    state.store.models,
    state.store.agentCatalogs,
    loadPreview,
  ]);

  const applyAgents = useCallback(
    async (agents: readonly AgentId[], scope: "one" | "all") => {
      if (agents.length === 0) return;
      setBusy(scope);
      try {
        const res = await api.applyConfig(agents, bindings);
        setResults((prev) => {
          const next = { ...prev };
          for (const r of res.results) next[r.agent] = r;
          return next;
        });
        const failed = res.results.filter((r) => !r.ok).length;
        onToast(failed ? `应用完成，${failed} 个失败` : "应用成功");
        await loadPreview(bindings);
      } catch (e) {
        onToast(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(null);
      }
    },
    [bindings, loadPreview, onToast],
  );

  const changedAgents = useMemo(
    () => AGENTS.filter((a) => agentChanged(preview, a.id)).map((a) => a.id),
    [preview],
  );

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-ink-3">
        正在读取各 Agent 磁盘配置…
      </div>
    );
  }

  const selectedAgent = AGENTS.find((a) => a.id === selected)!;

  return (
    <div className="flex h-full flex-col">
      <div className="flex min-h-0 flex-1 gap-4">
        {/* Left: agent list with status */}
        <aside className="flex w-56 shrink-0 flex-col gap-1">
          {AGENTS.map((a) => {
            const { exists } = agentPath(state, a.id);
            const changed = agentChanged(preview, a.id);
            const isActive = a.id === selected;
            return (
              <button
                key={a.id}
                type="button"
                onClick={() => setSelected(a.id)}
                className={
                  isActive
                    ? "flex items-center justify-between rounded-lg border border-accent/40 bg-accent/10 px-3 py-2.5 text-left"
                    : "flex items-center justify-between rounded-lg border border-surface-3 bg-surface-1 px-3 py-2.5 text-left hover:bg-surface-2"
                }
              >
                <span className="min-w-0">
                  <span className="block text-sm font-medium">{a.label}</span>
                  <span className="block text-[11px] text-ink-3">
                    {exists ? "已检测" : "未找到"}
                    {a.kind === "catalog" ? " · 目录同步" : ""}
                  </span>
                </span>
                <StatusDot changed={changed} exists={exists} />
              </button>
            );
          })}
          <div className="mt-2 rounded-md bg-surface-1 px-3 py-2 text-[11px] text-ink-3">
            首次进入 / 重置从磁盘加载。默认模型与模式为会话草稿（不落盘）；同步目录持久保存。
          </div>
          <button
            type="button"
            className="btn-secondary mt-1 !py-1.5 text-xs"
            onClick={() => void loadFromDisk()}
          >
            从磁盘重置草稿
          </button>
        </aside>

        {/* Right: selected agent detail */}
        <section className="min-w-0 flex-1 overflow-auto">
          <div className="card p-4">
            <AgentDetailHeader state={state} id={selected} label={selectedAgent.label} />

            {selected === "claude" || selected === "codex" ? (
              <ActiveOnlyEditor
                id={selected}
                bindings={bindings}
                providers={providers}
                modelsOf={modelsOf}
                onPatch={patch}
              />
            ) : (
              <CatalogEditor
                id={selected}
                state={state}
                bindings={bindings}
                modelsOf={modelsOf}
                onPatch={patch}
                onToast={onToast}
                onRefresh={onRefresh}
              />
            )}

            <AgentDiffView
              preview={preview}
              busy={previewBusy}
              agent={selected}
            />

            <div className="mt-4 flex items-center gap-2">
              <button
                type="button"
                className="btn-primary"
                disabled={busy !== null}
                onClick={() => void applyAgents([selected], "one")}
              >
                {busy === "one" ? "应用中…" : "应用此 Agent"}
              </button>
              {results[selected] ? (
                <ResultBadge result={results[selected]} />
              ) : null}
            </div>
          </div>
        </section>
      </div>

      {/* Bottom bar: apply all changed */}
      <div className="mt-3 flex shrink-0 items-center justify-between rounded-lg border border-surface-3 bg-surface-1 px-4 py-3">
        <span className="text-sm text-ink-2">
          {changedAgents.length > 0
            ? `${changedAgents.length} 个 Agent 有待应用更改：${changedAgents
                .map((id) => AGENTS.find((a) => a.id === id)?.label)
                .join("、")}`
            : "所有 Agent 与磁盘一致"}
        </span>
        <button
          type="button"
          className="btn-primary"
          disabled={busy !== null || changedAgents.length === 0}
          onClick={() => void applyAgents(changedAgents, "all")}
        >
          {busy === "all"
            ? "应用中…"
            : `应用全部更改 (${changedAgents.length})`}
        </button>
      </div>
    </div>
  );
}

function StatusDot({ changed, exists }: { readonly changed: boolean; readonly exists: boolean }) {
  const cls = changed
    ? "bg-warn"
    : exists
      ? "bg-ok"
      : "bg-surface-3";
  const label = changed ? "有更改" : exists ? "一致" : "无配置";
  return (
    <span className="flex shrink-0 items-center gap-1.5">
      <span className="text-[10px] text-ink-3">{label}</span>
      <span className={`h-2.5 w-2.5 rounded-full ${cls}`} />
    </span>
  );
}

function AgentDetailHeader({
  state,
  id,
  label,
}: {
  readonly state: FullState;
  readonly id: AgentId;
  readonly label: string;
}) {
  const { path, exists } = agentPath(state, id);
  return (
    <div className="mb-3 border-b border-surface-3 pb-3">
      <h3 className="font-semibold">{label}</h3>
      <p className="mt-1 font-mono text-[11px] text-ink-3">
        {exists ? "已检测到" : "未找到"} · {path}
      </p>
    </div>
  );
}

function ResultBadge({ result }: { readonly result: ApplyAgentResult }) {
  return (
    <span
      className={
        result.ok
          ? "text-xs text-ok"
          : "text-xs text-danger"
      }
      title={result.message}
    >
      {result.ok ? "已应用" : "失败"}
      {result.restartRequired && result.ok ? " · 建议重启" : ""}
    </span>
  );
}

// ---- active-only (Claude / Codex) ----

function ActiveOnlyEditor({
  id,
  bindings,
  providers,
  modelsOf,
  onPatch,
}: {
  readonly id: "claude" | "codex";
  readonly bindings: AgentBindings;
  readonly providers: readonly Provider[];
  readonly modelsOf: (providerId: string | null) => readonly Model[];
  readonly onPatch: (next: AgentBindings) => void;
}) {
  const b = bindings[id];
  const firstModelId = (providerId: string | null): string | null =>
    modelsOf(providerId)[0]?.id ?? null;

  const setMode = (mode: "official" | "third_party") => {
    if (id === "claude") {
      onPatch({ ...bindings, claude: { ...bindings.claude, mode } });
    } else {
      onPatch({ ...bindings, codex: { ...bindings.codex, mode } });
    }
  };
  const setProvider = (providerId: string | null) => {
    const modelId = firstModelId(providerId);
    if (id === "claude") {
      onPatch({ ...bindings, claude: { ...bindings.claude, providerId, modelId } });
    } else {
      onPatch({ ...bindings, codex: { ...bindings.codex, providerId, modelId } });
    }
  };
  const setModel = (modelId: string | null) => {
    if (id === "claude") {
      onPatch({ ...bindings, claude: { ...bindings.claude, modelId } });
    } else {
      onPatch({ ...bindings, codex: { ...bindings.codex, modelId } });
    }
  };

  return (
    <div className="space-y-3">
      <ModeToggle mode={b.mode} onChange={setMode} />
      {b.mode === "third_party" ? (
        <>
          <ProviderModelPickers
            providers={providers}
            providerId={b.providerId}
            modelId={b.modelId}
            models={modelsOf(b.providerId)}
            onProvider={setProvider}
            onModel={setModel}
          />
          {id === "codex" && b.providerId
            ? protocolWarn(providers, b.providerId)
            : null}
          {id === "claude" && !b.providerId ? (
            <p className="text-xs text-warn">
              磁盘上有第三方 baseUrl，但未匹配到 ModelHub 中的提供商（可先导入/新建）。
            </p>
          ) : null}
          {id === "codex" ? (
            <p className="text-xs text-ink-3">
              磁盘 model_provider 槽：{bindings.codex.providerKey || "—"}
            </p>
          ) : null}
        </>
      ) : (
        <p className="text-xs text-ink-3">
          {id === "claude"
            ? "官方模式：Apply 时清除 BASE_URL/TOKEN 劫持。"
            : "官方模式：model_provider=openai。"}
        </p>
      )}
    </div>
  );
}

// ---- catalog (OpenCode / Pi) ----

function CatalogEditor({
  id,
  state,
  bindings,
  modelsOf,
  onPatch,
  onToast,
  onRefresh,
}: {
  readonly id: "opencode" | "pi";
  readonly state: FullState;
  readonly bindings: AgentBindings;
  readonly modelsOf: (providerId: string | null) => readonly Model[];
  readonly onPatch: (next: AgentBindings) => void;
  readonly onToast: (msg: string) => void;
  readonly onRefresh: () => Promise<void>;
}) {
  const providers = state.store.providers;
  const catalogRaw = state.store.agentCatalogs[id];
  const catalog = useMemo(() => catalogRaw ?? [], [catalogRaw]);
  // providerId -> entry (presence = in catalog; empty modelIds = all models).
  const byProvider = useMemo(() => {
    const m = new Map<string, CatalogEntry>();
    for (const e of catalog) m.set(e.providerId, e);
    return m;
  }, [catalog]);
  const [search, setSearch] = useState("");
  const [saving, setSaving] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});

  const visible = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return providers;
    return providers.filter((p) => p.name.toLowerCase().includes(q));
  }, [providers, search]);

  const saveCatalog = useCallback(
    async (entries: readonly CatalogEntry[]) => {
      setSaving(true);
      try {
        await api.setAgentCatalog(id, entries);
        await onRefresh();
      } catch (e) {
        onToast(e instanceof Error ? e.message : String(e));
      } finally {
        setSaving(false);
      }
    },
    [id, onRefresh, onToast],
  );

  // Toggle a provider's membership. Adding starts with empty modelIds = all.
  const toggleProvider = (providerId: string) => {
    const next = byProvider.has(providerId)
      ? catalog.filter((e) => e.providerId !== providerId)
      : [...catalog, { providerId, modelIds: [] as string[] }];
    void saveCatalog(next);
  };

  // Toggle a single model within a provider's subset. UncMecking the last one
  // removes the provider from the catalog (avoids the empty=all ambiguity).
  const toggleModel = (providerId: string, modelRowId: string) => {
    const models = modelsOf(providerId);
    const entry = byProvider.get(providerId);
    // Current effective selection: empty modelIds means "all".
    const current = new Set(
      entry && entry.modelIds.length > 0
        ? entry.modelIds
        : models.map((m) => m.id),
    );
    if (current.has(modelRowId)) current.delete(modelRowId);
    else current.add(modelRowId);

    if (current.size === 0) {
      void saveCatalog(catalog.filter((e) => e.providerId !== providerId));
      return;
    }
    // If all models are selected, store empty (dynamic all).
    const allSelected = models.length > 0 && current.size === models.length;
    const modelIds = allSelected ? [] : [...current];
    const nextEntry: CatalogEntry = { providerId, modelIds };
    const next = byProvider.has(providerId)
      ? catalog.map((e) => (e.providerId === providerId ? nextEntry : e))
      : [...catalog, nextEntry];
    void saveCatalog(next);
  };

  // Select-all providers acts on the current (searched) list; others kept.
  const visibleIds = visible.map((p) => p.id);
  const allVisibleSelected =
    visibleIds.length > 0 && visibleIds.every((vid) => byProvider.has(vid));
  const toggleAllVisible = () => {
    if (allVisibleSelected) {
      void saveCatalog(catalog.filter((e) => !visibleIds.includes(e.providerId)));
    } else {
      const merged = [...catalog];
      for (const vid of visibleIds) {
        if (!byProvider.has(vid)) merged.push({ providerId: vid, modelIds: [] });
      }
      void saveCatalog(merged);
    }
  };

  // Default model provider is restricted to the catalog.
  const catalogProviders = providers.filter((p) => byProvider.has(p.id));
  const b = bindings[id];
  const firstModelId = (providerId: string | null): string | null =>
    modelsOf(providerId)[0]?.id ?? null;
  const setProvider = (providerId: string | null) => {
    const modelId = firstModelId(providerId);
    onPatch({ ...bindings, [id]: { ...bindings[id], providerId, modelId } } as AgentBindings);
  };
  const setModel = (modelId: string | null) => {
    onPatch({ ...bindings, [id]: { ...bindings[id], modelId } } as AgentBindings);
  };

  return (
    <div className="space-y-4">
      <div>
        <div className="mb-2 flex items-center justify-between gap-2">
          <label className="label !mb-0">
            同步目录（Apply 时写出这些 Provider 及所选模型）
          </label>
          <span className="text-[11px] text-ink-3">已选 {catalog.length} 个 Provider</span>
        </div>
        <div className="mb-2 flex gap-2">
          <input
            className="input flex-1"
            placeholder="搜索提供商名称…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <button
            type="button"
            className="btn-secondary !py-1 text-xs"
            disabled={saving || visibleIds.length === 0}
            onClick={toggleAllVisible}
          >
            {allVisibleSelected ? "取消全选" : "全选"}
          </button>
        </div>
        <div className="max-h-72 overflow-auto rounded-md border border-surface-3">
          {visible.length === 0 ? (
            <p className="px-3 py-4 text-center text-xs text-ink-3">
              {providers.length === 0 ? "暂无提供商" : "无匹配结果"}
            </p>
          ) : (
            visible.map((p) => {
              const entry = byProvider.get(p.id);
              const inCatalog = Boolean(entry);
              const models = modelsOf(p.id);
              const subsetAll = !entry || entry.modelIds.length === 0;
              const selectedCount = subsetAll ? models.length : entry!.modelIds.length;
              const isOpen = expanded[p.id] ?? false;
              return (
                <div key={p.id} className="border-b border-surface-3 last:border-b-0">
                  <div className="flex items-center gap-2 px-3 py-2 text-sm hover:bg-surface-1">
                    <input
                      type="checkbox"
                      checked={inCatalog}
                      disabled={saving}
                      onChange={() => toggleProvider(p.id)}
                    />
                    <span className="min-w-0 flex-1 truncate">{p.name}</span>
                    <span className="font-mono text-[10px] text-ink-3">{p.protocol}</span>
                    {inCatalog ? (
                      <button
                        type="button"
                        className="text-[11px] text-ink-3 hover:text-ink-1"
                        onClick={() =>
                          setExpanded((prev) => ({ ...prev, [p.id]: !isOpen }))
                        }
                      >
                        {subsetAll
                          ? `全部模型 (${models.length})`
                          : `${selectedCount}/${models.length} 模型`}
                        <span className="ml-1">{isOpen ? "▾" : "▸"}</span>
                      </button>
                    ) : null}
                  </div>
                  {inCatalog && isOpen ? (
                    <div className="bg-surface-0/60 px-3 pb-2 pl-9">
                      {models.length === 0 ? (
                        <p className="py-1 text-[11px] text-ink-3">该 Provider 暂无模型</p>
                      ) : (
                        models.map((m) => {
                          const checked = subsetAll || entry!.modelIds.includes(m.id);
                          return (
                            <label
                              key={m.id}
                              className="flex cursor-pointer items-center gap-2 py-1 text-xs"
                            >
                              <input
                                type="checkbox"
                                checked={checked}
                                disabled={saving}
                                onChange={() => toggleModel(p.id, m.id)}
                              />
                              <span className="min-w-0 flex-1 truncate">
                                {m.displayName}
                              </span>
                              <span className="font-mono text-[10px] text-ink-3">
                                {m.modelId}
                              </span>
                            </label>
                          );
                        })
                      )}
                    </div>
                  ) : null}
                </div>
              );
            })
          )}
        </div>
        <p className="mt-1 text-[11px] text-ink-3">
          勾选 Provider 后默认同步其全部模型；展开可只挑选部分。取消全部模型等于移出目录。
        </p>
      </div>

      <div>
        <label className="label">默认模型（从同步目录中选）</label>
        {catalogProviders.length === 0 ? (
          <p className="text-xs text-ink-3">先在上方勾选至少一个 Provider。</p>
        ) : (
          <>
            <ProviderModelPickers
              providers={catalogProviders}
              providerId={b.providerId}
              modelId={b.modelId}
              models={modelsOf(b.providerId)}
              onProvider={setProvider}
              onModel={setModel}
            />
            {id === "opencode" ? (
              <SmallModelPicker
                bindings={bindings}
                modelsOf={modelsOf}
                onPatch={onPatch}
              />
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}

function SmallModelPicker({
  bindings,
  modelsOf,
  onPatch,
}: {
  readonly bindings: AgentBindings;
  readonly modelsOf: (providerId: string | null) => readonly Model[];
  readonly onPatch: (next: AgentBindings) => void;
}) {
  // small_model reuses the default provider's models.
  const providerId = bindings.opencode.providerId;
  const models = modelsOf(providerId);
  return (
    <div className="mt-2">
      <label className="label">Small Model（可选）</label>
      <select
        className="input"
        value={bindings.opencode.smallModelId ?? ""}
        disabled={!providerId}
        onChange={(e) =>
          onPatch({
            ...bindings,
            opencode: { ...bindings.opencode, smallModelId: e.target.value || null },
          })
        }
      >
        <option value="">未设置</option>
        {models.map((m) => (
          <option key={m.id} value={m.id}>
            {m.displayName} ({m.modelId})
          </option>
        ))}
      </select>
    </div>
  );
}

// ---- shared bits ----

function AgentDiffView({
  preview,
  busy,
  agent,
}: {
  readonly preview: ApplyPreview | null;
  readonly busy: boolean;
  readonly agent: AgentId;
}) {
  const diff = preview?.agents.find((a) => a.agent === agent) ?? null;
  return (
    <div className="mt-4 border-t border-surface-3 pt-3">
      <div className="mb-2 flex items-center justify-between">
        <h4 className="text-sm font-medium">更改对比（磁盘现状 → Apply 后）</h4>
        {busy ? <span className="text-[11px] text-ink-3">对比中…</span> : null}
      </div>
      {!diff ? (
        <p className="text-xs text-ink-3">加载对比…</p>
      ) : (
        <>
          {diff.note ? <p className="mb-2 text-xs text-ink-3">{diff.note}</p> : null}
          <ul className="space-y-1 font-mono text-xs">
            {diff.lines.map((line, i) => (
              <li
                key={`${agent}-${i}`}
                className={
                  line.kind === "add"
                    ? "text-ok"
                    : line.kind === "remove"
                      ? "text-danger"
                      : line.kind === "change"
                        ? "text-warn"
                        : "text-ink-3"
                }
              >
                {line.text}
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}

function protocolWarn(providers: readonly Provider[], providerId: string) {
  const p = providers.find((x) => x.id === providerId);
  if (!p || p.protocol === "openai-responses") return null;
  return (
    <div className="rounded-md border border-warn/40 bg-warn/10 px-3 py-2 text-xs text-warn">
      当前协议为 {p.protocol}，Codex 通常需要 openai-responses，可能不可用。
    </div>
  );
}

function ModeToggle({
  mode,
  onChange,
}: {
  readonly mode: "official" | "third_party";
  readonly onChange: (mode: "official" | "third_party") => void;
}) {
  return (
    <div className="flex gap-2">
      {(
        [
          ["official", "官方订阅"],
          ["third_party", "第三方"],
        ] as const
      ).map(([id, label]) => (
        <button
          key={id}
          type="button"
          className={mode === id ? "btn-primary" : "btn-secondary"}
          onClick={() => onChange(id)}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

function ProviderModelPickers({
  providers,
  providerId,
  modelId,
  models,
  onProvider,
  onModel,
}: {
  readonly providers: readonly Provider[];
  readonly providerId: string | null;
  readonly modelId: string | null;
  readonly models: readonly Model[];
  readonly onProvider: (id: string | null) => void;
  readonly onModel: (id: string | null) => void;
}) {
  return (
    <div className="grid grid-cols-2 gap-2">
      <div>
        <label className="label">Provider</label>
        <select
          className="input"
          value={providerId ?? ""}
          onChange={(e) => onProvider(e.target.value || null)}
        >
          <option value="">未匹配 / 未选择</option>
          {providers.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      </div>
      <div>
        <label className="label">Model</label>
        <select
          className="input"
          value={modelId ?? ""}
          onChange={(e) => onModel(e.target.value || null)}
          disabled={!providerId}
        >
          <option value="">未匹配 / 未选择</option>
          {models.map((m) => (
            <option key={m.id} value={m.id}>
              {m.displayName} ({m.modelId})
            </option>
          ))}
        </select>
      </div>
    </div>
  );
}
