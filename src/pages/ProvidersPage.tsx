import { useEffect, useMemo, useRef, useState } from "react";
import type { FullState, Model, Protocol, Provider, ProviderInput, RemoteModel } from "../types";
import { PROTOCOLS } from "../types";
import * as api from "../api/tauri";
import { ConfirmDialog, Modal } from "../components/Modal";
import { TestConnectionModal } from "../components/TestConnectionModal";
import { ExternalLink } from "../components/ExternalLink";
import { BatchTestConnectionModal } from "../components/BatchTestConnectionModal";
import { MultiProviderTestModal } from "../components/MultiProviderTestModal";
import { getBatchTestSession, subscribeBatchTestSession } from "../lib/batchTestSession";
import { getMultiTestSession, isMultiTestBusy, subscribeMultiTestSession } from "../lib/multiTestSession";
import { getSingleTestSession, subscribeSingleTestSession } from "../lib/singleTestSession";
import { formatTestedAt, getLastTestResult, subscribeLastTestResults } from "../lib/lastTestResults";
import { getModelTestDisplay } from "../lib/testDisplay";

type Props = {
  readonly state: FullState;
  readonly onRefresh: () => Promise<void>;
  readonly onToast: (msg: string) => void;
};

type ConfirmState =
  | { kind: "deleteOne"; id: string; name: string }
  | { kind: "deleteMany"; ids: string[] }
  | { kind: "deleteModel"; id: string; name: string }
  | null;

export function ProvidersPage({ state, onRefresh, onToast }: Props) {
  const [q, setQ] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [checkedIds, setCheckedIds] = useState<Set<string>>(new Set());
  const [showCreate, setShowCreate] = useState(false);
  const [remoteByProvider, setRemoteByProvider] = useState<Record<string, RemoteModel[]>>({});
  const [fetchingId, setFetchingId] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<ConfirmState>(null);
  const [confirmBusy, setConfirmBusy] = useState(false);
  const [multiTesting, setMultiTesting] = useState(false);
  const [multiSessionTick, setMultiSessionTick] = useState(0);
  useEffect(() => {
    return subscribeMultiTestSession(() => setMultiSessionTick((n) => n + 1));
  }, []);

  const providers = useMemo(() => {
    const list = [...state.store.providers];
    list.sort((a, b) => a.name.localeCompare(b.name, "zh"));
    if (!q.trim()) return list;
    const needle = q.trim().toLowerCase();
    return list.filter(
      (p) =>
        p.name.toLowerCase().includes(needle) ||
        p.baseUrl.toLowerCase().includes(needle) ||
        p.protocol.includes(needle),
    );
  }, [state.store.providers, q]);

  const selected = providers.find((p) => p.id === selectedId) ?? null;
  const models = selected
    ? state.store.models.filter((m) => m.providerId === selected.id)
    : [];

  const allChecked =
    providers.length > 0 && providers.every((p) => checkedIds.has(p.id));

  const toggleCheck = (id: string) => {
    setCheckedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleCheckAll = () => {
    if (allChecked) setCheckedIds(new Set());
    else setCheckedIds(new Set(providers.map((p) => p.id)));
  };

  const fetchModelsFor = async (providerId: string) => {
    if (fetchingId) return;
    setFetchingId(providerId);
    try {
      const list = await api.fetchProviderModels(providerId);
      setRemoteByProvider((prev) => ({ ...prev, [providerId]: [...list] }));
      onToast(`已获取 ${list.length} 个远程模型`);
    } catch (e) {
      onToast(`获取模型失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setFetchingId(null);
    }
  };

  const runConfirm = async () => {
    if (!confirm) return;
    setConfirmBusy(true);
    try {
      if (confirm.kind === "deleteOne") {
        await api.deleteProvider(confirm.id);
        setCheckedIds((prev) => {
          const next = new Set(prev);
          next.delete(confirm.id);
          return next;
        });
        if (selectedId === confirm.id) setSelectedId(null);
        await onRefresh();
        onToast("已删除提供商");
      } else if (confirm.kind === "deleteMany") {
        const n = await api.deleteProviders(confirm.ids);
        setCheckedIds(new Set());
        if (selectedId && confirm.ids.includes(selectedId)) setSelectedId(null);
        await onRefresh();
        onToast(`已删除 ${n} 个提供商`);
      } else if (confirm.kind === "deleteModel") {
        await api.deleteModel(confirm.id);
        await onRefresh();
        onToast("已删除模型");
      }
      setConfirm(null);
    } catch (e) {
      onToast(`删除失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setConfirmBusy(false);
    }
  };

  return (
    <div className="grid h-full grid-cols-12 gap-4">
      <section className="col-span-5 flex min-h-0 flex-col gap-3">
        <div className="flex items-center gap-2">
          <input
            className="input"
            placeholder="搜索名称 / URL / 协议"
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
          <button type="button" className="btn-primary shrink-0" onClick={() => setShowCreate(true)}>
            新建
          </button>
        </div>

        <div className="flex items-center gap-2 text-sm">
          <label className="flex items-center gap-2 text-ink-2">
            <input type="checkbox" checked={allChecked} onChange={toggleCheckAll} />
            全选
          </label>
          <span className="text-ink-3">已选 {checkedIds.size}</span>
          <button
            type="button"
            className="btn-danger !py-1 text-xs"
            disabled={checkedIds.size === 0}
            onClick={() => setConfirm({ kind: "deleteMany", ids: [...checkedIds] })}
          >
            删除所选
          </button>
          <button
            type="button"
            className="btn-secondary !py-1 text-xs"
            disabled={checkedIds.size === 0}
            title="对勾选的提供商下模型做连通性测试（全局并发 3，同提供商串行）"
            onClick={() => setMultiTesting(true)}
          >
            {(() => {
              void multiSessionTick;
              if (isMultiTestBusy()) {
                const s = getMultiTestSession();
                if (s && [...checkedIds].some((id) => s.providerIds.includes(id))) {
                  return "测试中…";
                }
              }
              return "测试所选";
            })()}
          </button>
        </div>

        <div className="card min-h-0 flex-1 overflow-auto">
          {providers.length === 0 ? (
            <div className="p-8 text-center text-sm text-ink-3">
              暂无提供商。可「新建」或到「导入」从本机配置导入。
            </div>
          ) : (
            <ul className="divide-y divide-surface-3">
              {providers.map((p) => {
                const count = state.store.models.filter((m) => m.providerId === p.id).length;
                const active = p.id === selectedId;
                return (
                  <li key={p.id} className="flex items-stretch">
                    <label className="flex items-center px-3" onClick={(e) => e.stopPropagation()}>
                      <input
                        type="checkbox"
                        checked={checkedIds.has(p.id)}
                        onChange={() => toggleCheck(p.id)}
                      />
                    </label>
                    <button
                      type="button"
                      className={
                        active
                          ? "min-w-0 flex-1 px-2 py-3 text-left bg-accent/10"
                          : "min-w-0 flex-1 px-2 py-3 text-left hover:bg-surface-2"
                      }
                      onClick={() => setSelectedId(p.id)}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className="font-medium">{p.name}</span>
                        <span
                          className={
                            p.enabled
                              ? "badge bg-ok/15 text-ok"
                              : "badge bg-surface-3 text-ink-3"
                          }
                        >
                          {p.enabled ? "同步中" : "未同步"}
                        </span>
                      </div>
                      <ExternalLink
                        href={p.baseUrl}
                        className="mt-1 inline-block max-w-full cursor-pointer truncate align-bottom font-mono text-xs text-accent hover:underline"
                        title={`在浏览器打开 ${p.baseUrl}`}
                      >
                        {p.baseUrl}
                      </ExternalLink>
                      <div className="mt-1 flex items-center gap-2 text-xs text-ink-3">
                        <span className="badge bg-surface-3 text-ink-2">{p.protocol}</span>
                        <span>{count} 模型</span>
                        <span>{state.secretMasks[p.secretRef] ?? "••••"}</span>
                      </div>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </section>

      <section className="col-span-7 min-h-0">
        {selected ? (
          <ProviderDetail
            key={selected.id}
            provider={selected}
            models={models}
            testPrompts={state.store.testPrompts ?? []}
            mask={state.secretMasks[selected.secretRef] ?? "••••"}
            remoteModels={remoteByProvider[selected.id] ?? []}
            fetching={fetchingId === selected.id}
            fetchDisabled={fetchingId !== null}
            onFetch={() => void fetchModelsFor(selected.id)}
            onRefresh={onRefresh}
            onToast={onToast}
            onRequestDelete={() =>
              setConfirm({ kind: "deleteOne", id: selected.id, name: selected.name })
            }
            onRequestDeleteModel={(id, name) =>
              setConfirm({ kind: "deleteModel", id, name })
            }
          />
        ) : (
          <div className="card flex h-full items-center justify-center p-8 text-sm text-ink-3">
            选择左侧提供商查看详情
          </div>
        )}
      </section>

      {multiTesting ? (
        <MultiProviderTestModal
          providers={state.store.providers.filter((p) => checkedIds.has(p.id))}
          models={state.store.models}
          prompts={state.store.testPrompts ?? []}
          onClose={() => setMultiTesting(false)}
          onToast={onToast}
        />
      ) : null}

      {showCreate ? (
        <ProviderFormModal
          title="新建提供商"
          onClose={() => setShowCreate(false)}
          onSubmit={async (input) => {
            await api.addProvider(input);
            await onRefresh();
            onToast("已创建提供商");
            setShowCreate(false);
          }}
        />
      ) : null}

      {confirm ? (
        <ConfirmDialog
          title={confirm.kind === "deleteModel" ? "删除模型" : "删除提供商"}
          message={
            confirm.kind === "deleteMany"
              ? `确定删除选中的 ${confirm.ids.length} 个提供商？\n此操作不可撤销。`
              : confirm.kind === "deleteOne"
                ? `确定删除提供商「${confirm.name}」？\n此操作不可撤销。`
                : `确定删除模型「${confirm.name}」？`
          }
          confirmLabel="删除"
          danger
          busy={confirmBusy}
          onCancel={() => !confirmBusy && setConfirm(null)}
          onConfirm={() => void runConfirm()}
        />
      ) : null}
    </div>
  );
}

function ProviderDetail({
  provider,
  models,
  testPrompts,
  mask,
  remoteModels,
  fetching,
  fetchDisabled,
  onFetch,
  onRefresh,
  onToast,
  onRequestDelete,
  onRequestDeleteModel,
}: {
  readonly provider: Provider;
  readonly models: readonly Model[];
  readonly testPrompts: FullState["store"]["testPrompts"];
  readonly mask: string;
  readonly remoteModels: readonly RemoteModel[];
  readonly fetching: boolean;
  readonly fetchDisabled: boolean;
  readonly onFetch: () => void;
  readonly onRefresh: () => Promise<void>;
  readonly onToast: (msg: string) => void;
  readonly onRequestDelete: () => void;
  readonly onRequestDeleteModel: (id: string, name: string) => void;
}) {
  const [tab, setTab] = useState<"connect" | "models">("models");
  const [editing, setEditing] = useState(false);
  const [addingModel, setAddingModel] = useState(false);
  const [cloneOpen, setCloneOpen] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const [plainKey, setPlainKey] = useState<string | null>(null);
  const [editingModelId, setEditingModelId] = useState<string | null>(null);
  const [testingModelId, setTestingModelId] = useState<string | null>(null);
  const [batchTesting, setBatchTesting] = useState(false);
  const [batchSessionTick, setBatchSessionTick] = useState(0);

  useEffect(() => {
    return subscribeBatchTestSession(() => setBatchSessionTick((n) => n + 1));
  }, []);
  const [singleSessionTick, setSingleSessionTick] = useState(0);
  useEffect(() => {
    return subscribeSingleTestSession(() => setSingleSessionTick((n) => n + 1));
  }, []);
  const [lastTestTick, setLastTestTick] = useState(0);
  useEffect(() => {
    return subscribeLastTestResults(() => setLastTestTick((n) => n + 1));
  }, []);
  const [multiSessionTick, setMultiSessionTick] = useState(0);
  useEffect(() => {
    return subscribeMultiTestSession(() => setMultiSessionTick((n) => n + 1));
  }, []);

  const testingModel = testingModelId
    ? models.find((m) => m.id === testingModelId) ?? null
    : null;

  const toggleKey = async () => {
    if (showKey) {
      setShowKey(false);
      return;
    }
    try {
      const key = await api.revealApiKey(provider.secretRef);
      setPlainKey(key);
      setShowKey(true);
    } catch (e) {
      onToast(`读取密钥失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  return (
    <div className="card flex h-full min-h-0 flex-col">
      <div className="flex items-start justify-between gap-3 border-b border-surface-3 px-4 py-3">
        <div>
          <h2 className="text-base font-semibold">{provider.name}</h2>
          <ExternalLink
            href={provider.baseUrl}
            className="mt-1 inline-block max-w-full cursor-pointer truncate align-bottom font-mono text-xs text-accent hover:underline"
            title={`在浏览器打开 ${provider.baseUrl}`}
            onError={onToast}
          >
            {provider.baseUrl}
          </ExternalLink>
        </div>
        <div className="flex flex-wrap gap-2">
          <button type="button" className="btn-secondary" onClick={() => setEditing(true)}>
            编辑
          </button>
          <button type="button" className="btn-secondary" onClick={() => setCloneOpen(true)}>
            克隆
          </button>
          <button
            type="button"
            className="btn-secondary"
            onClick={async () => {
              try {
                await api.setProviderEnabled(provider.id, !provider.enabled);
                await onRefresh();
                onToast(provider.enabled ? "已关闭 OC/Pi 同步" : "已开启 OC/Pi 同步");
              } catch (e) {
                onToast(`操作失败：${e instanceof Error ? e.message : String(e)}`);
              }
            }}
          >
            {provider.enabled ? "禁用同步" : "启用同步"}
          </button>
          <button type="button" className="btn-danger" onClick={onRequestDelete}>
            删除
          </button>
        </div>
      </div>

      <div className="flex gap-1 border-b border-surface-3 px-3 pt-2">
        {(
          [
            ["models", "模型"],
            ["connect", "连接"],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            className={
              tab === id
                ? "rounded-t-md bg-surface-2 px-3 py-1.5 text-sm text-ink-1"
                : "px-3 py-1.5 text-sm text-ink-3 hover:text-ink-1"
            }
            onClick={() => setTab(id)}
          >
            {label}
          </button>
        ))}
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-4">
        {tab === "connect" ? (
          <dl className="grid grid-cols-3 gap-3 text-sm">
            <div className="col-span-1 text-ink-3">协议</div>
            <div className="col-span-2 font-mono">{provider.protocol}</div>
            <div className="col-span-1 text-ink-3">密钥</div>
            <div className="col-span-2 flex flex-wrap items-center gap-2 font-mono">
              <span className="break-all">{showKey && plainKey ? plainKey : mask}</span>
              <button type="button" className="btn-ghost !px-2 !py-0.5 text-xs" onClick={() => void toggleKey()}>
                {showKey ? "隐藏" : "显示"}
              </button>
              <button
                type="button"
                className="btn-ghost !px-2 !py-0.5 text-xs"
                onClick={async () => {
                  try {
                    const key = plainKey ?? (await api.revealApiKey(provider.secretRef));
                    setPlainKey(key);
                    await navigator.clipboard.writeText(key);
                    onToast("已复制完整密钥");
                  } catch (e) {
                    onToast(`复制失败：${e instanceof Error ? e.message : String(e)}`);
                  }
                }}
              >
                复制
              </button>
            </div>
            <div className="col-span-1 text-ink-3">同步到 OC/Pi</div>
            <div className="col-span-2">{provider.enabled ? "是" : "否"}</div>
            <div className="col-span-1 text-ink-3">备注</div>
            <div className="col-span-2 whitespace-pre-wrap">{provider.notes || "—"}</div>
          </dl>
        ) : (
          <div>
            <div className="mb-3 flex items-center justify-end gap-2">
              {remoteModels.length > 0 ? (
                <span className="text-xs text-ink-3">已缓存 {remoteModels.length} 个</span>
              ) : null}
              <button
                type="button"
                className="btn-secondary"
                disabled={fetchDisabled}
                onClick={onFetch}
              >
                {fetching ? "获取中…" : "获取模型"}
              </button>
              <button
                type="button"
                className="btn-secondary"
                disabled={models.length === 0}
                title="串行测试全部模型（并发 1）；关闭弹窗后测试仍继续"
                onClick={() => setBatchTesting(true)}
              >
                {(() => {
                  void batchSessionTick;
                  const s = getBatchTestSession();
                  if (s?.providerId === provider.id && s.busy) return "测试中…";
                  return "测试全部";
                })()}
              </button>
              <button type="button" className="btn-primary" onClick={() => setAddingModel(true)}>
                添加模型
              </button>
            </div>
            {models.length === 0 ? (
              <div className="text-sm text-ink-3">暂无模型。可「获取模型」后添加，或手动填写。</div>
            ) : (
              <table className="w-full table-fixed text-left text-sm">
                <colgroup>
                  <col className="w-[28%]" />
                  <col className="w-[20%]" />
                  <col className="w-[12%]" />
                  <col className="w-[12%]" />
                  <col className="w-[28%]" />
                </colgroup>
                <thead className="text-xs text-ink-3">
                  <tr>
                    <th className="pb-2 pr-2 font-medium">Model ID</th>
                    <th className="pb-2 pr-2 font-medium">展示名</th>
                    <th className="pb-2 font-medium">启用</th>
                    <th className="pb-2 font-medium">最近测试</th>
                    <th className="pb-2 font-medium" />
                  </tr>
                </thead>
                <tbody>
                  {models.map((m) =>
                    editingModelId === m.id ? (
                      <ModelEditRow
                        key={m.id}
                        model={m}
                        remoteModels={remoteModels}
                        onCancel={() => setEditingModelId(null)}
                        onSaved={async () => {
                          setEditingModelId(null);
                          await onRefresh();
                          onToast("模型已更新");
                        }}
                        onError={(msg) => onToast(msg)}
                      />
                    ) : (
                      <tr key={m.id} className="border-t border-surface-3">
                        <td className="truncate py-2 pr-2 font-mono text-xs" title={m.modelId}>
                          {m.modelId}
                        </td>
                        <td className="truncate py-2 pr-2" title={m.displayName}>
                          {m.displayName}
                        </td>
                        <td className="py-2">
                          <button
                            type="button"
                            className={
                              m.enabled
                                ? "rounded-md border border-ok/40 bg-ok/15 px-2 py-1 text-xs text-ok"
                                : "rounded-md border border-surface-3 bg-surface-0 px-2 py-1 text-xs text-ink-3"
                            }
                            onClick={async () => {
                              try {
                                await api.updateModel(m.id, {
                                  providerId: m.providerId,
                                  modelId: m.modelId,
                                  displayName: m.displayName,
                                  enabled: !m.enabled,
                                  capabilities: m.capabilities,
                                });
                                await onRefresh();
                              } catch (e) {
                                onToast(`更新失败：${e instanceof Error ? e.message : String(e)}`);
                              }
                            }}
                          >
                            {m.enabled ? "已启用" : "已禁用"}
                          </button>
                        </td>
                        <td className="py-2">
                          {(() => {
                            void lastTestTick;
                            void singleSessionTick;
                            void batchSessionTick;
                            void multiSessionTick;
                            // Prefer live session row (pending/running/ok/fail) over stale last success.
                            const d = getModelTestDisplay(m.id);
                            if (d.status === "running") {
                              return (
                                <span className="badge bg-accent/20 text-accent">测试中</span>
                              );
                            }
                            if (d.status === "pending" && (d.source === "multi" || d.source === "batch")) {
                              return (
                                <span className="badge bg-surface-3 text-ink-3">待测</span>
                              );
                            }
                            if (d.status === "ok") {
                              const last = getLastTestResult(m.id);
                              const tip = last
                                ? `测试时间：${formatTestedAt(last.testedAt)}${
                                    last.latencyMs != null ? ` · ${last.latencyMs} ms` : ""
                                  }`
                                : d.latencyMs != null
                                  ? `${d.latencyMs} ms`
                                  : undefined;
                              return (
                                <span className="badge bg-ok/15 text-ok" title={tip}>
                                  成功
                                </span>
                              );
                            }
                            if (d.status === "fail") {
                              const last = getLastTestResult(m.id);
                              const tip = last
                                ? `测试时间：${formatTestedAt(last.testedAt)}${
                                    last.latencyMs != null ? ` · ${last.latencyMs} ms` : ""
                                  }`
                                : undefined;
                              return (
                                <span className="badge bg-danger/20 text-danger" title={tip}>
                                  失败
                                </span>
                              );
                            }
                            if (d.status === "skipped" && (d.source === "multi" || d.source === "batch")) {
                              return (
                                <span className="badge bg-surface-3 text-ink-3">跳过</span>
                              );
                            }
                            // Fallback: disk last result when no active session status
                            const last = getLastTestResult(m.id);
                            if (!last) {
                              return <span className="text-xs text-ink-3">—</span>;
                            }
                            const tip = `测试时间：${formatTestedAt(last.testedAt)}${
                              last.latencyMs != null ? ` · ${last.latencyMs} ms` : ""
                            }`;
                            return last.ok ? (
                              <span className="badge bg-ok/15 text-ok" title={tip}>
                                成功
                              </span>
                            ) : (
                              <span className="badge bg-danger/20 text-danger" title={tip}>
                                失败
                              </span>
                            );
                          })()}
                        </td>
                        <td className="py-2 text-right">
                          <div className="flex justify-end gap-1">
                            <button
                              type="button"
                              className="btn-secondary !px-2 !py-1 text-xs"
                              onClick={() => setTestingModelId(m.id)}
                            >
                              {(() => {
                                void singleSessionTick;
                                const s = getSingleTestSession();
                                if (s?.modelId === m.id && s.busy) return "测试中…";
                                return "测试";
                              })()}
                            </button>
                            <button
                              type="button"
                              className="btn-secondary !px-2 !py-1 text-xs"
                              onClick={() => setEditingModelId(m.id)}
                            >
                              编辑
                            </button>
                            <button
                              type="button"
                              className="btn-danger !px-2 !py-1 text-xs"
                              onClick={() => onRequestDeleteModel(m.id, m.displayName || m.modelId)}
                            >
                              删除
                            </button>
                          </div>
                        </td>
                      </tr>
                    ),
                  )}
                </tbody>
              </table>
            )}
          </div>
        )}
      </div>

      {editing ? (
        <ProviderFormModal
          title="编辑提供商"
          initial={provider}
          onClose={() => setEditing(false)}
          onSubmit={async (input) => {
            await api.updateProvider(provider.id, input);
            await onRefresh();
            onToast("已保存");
            setEditing(false);
          }}
        />
      ) : null}

      {addingModel ? (
        <ModelFormModal
          providerId={provider.id}
          existingIds={new Set(models.map((m) => m.modelId))}
          remoteModels={remoteModels}
          onClose={() => setAddingModel(false)}
          onSubmit={async (items) => {
            for (const item of items) {
              await api.addModel({
                providerId: provider.id,
                modelId: item.modelId,
                displayName: item.displayName,
                enabled: item.enabled,
                capabilities: { reasoning: false, vision: false },
              });
            }
            await onRefresh();
            onToast(`已添加 ${items.length} 个模型`);
            setAddingModel(false);
          }}
        />
      ) : null}

      {cloneOpen ? (
        <CloneModal
          sourceName={provider.name}
          onClose={() => setCloneOpen(false)}
          onSubmit={async (name, key) => {
            await api.cloneProvider(provider.id, name, key);
            await onRefresh();
            onToast("已克隆为新 Provider 实例");
            setCloneOpen(false);
          }}
        />
      ) : null}

      {batchTesting ? (
        <BatchTestConnectionModal
          provider={provider}
          models={models}
          prompts={testPrompts}
          onClose={() => setBatchTesting(false)}
          onToast={onToast}
        />
      ) : null}

      {testingModel ? (
        <TestConnectionModal
          provider={provider}
          model={testingModel}
          prompts={testPrompts}
          onClose={() => setTestingModelId(null)}
          onPromptsChanged={onRefresh}
          onToast={onToast}
        />
      ) : null}
    </div>
  );
}

function ModelEditRow({
  model,
  remoteModels,
  onCancel,
  onSaved,
  onError,
}: {
  readonly model: Model;
  readonly remoteModels: readonly RemoteModel[];
  readonly onCancel: () => void;
  readonly onSaved: () => Promise<void>;
  readonly onError: (msg: string) => void;
}) {
  const [modelId, setModelId] = useState(model.modelId);
  const [displayName, setDisplayName] = useState(model.displayName);
  const [enabled, setEnabled] = useState(model.enabled);
  const [busy, setBusy] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const pickerRef = useRef<HTMLDivElement | null>(null);
  const hasRemote = remoteModels.length > 0;

  useEffect(() => {
    if (!pickerOpen) return;
    const onDoc = (e: MouseEvent) => {
      if (!pickerRef.current) return;
      if (!pickerRef.current.contains(e.target as Node)) setPickerOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setPickerOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [pickerOpen]);

  const pickRemote = (id: string) => {
    setModelId(id);
    const meta = remoteModels.find((x) => x.id === id);
    if (meta) setDisplayName(meta.name || id);
    setPickerOpen(false);
  };

  // Compact controls that stay within fixed table columns (no column reflow).
  const compactInput =
    "box-border w-full min-w-0 rounded-md border border-surface-3 bg-surface-0 px-2 py-1 text-xs text-ink-1 outline-none focus:border-accent";

  return (
    <tr className="border-t border-surface-3 bg-surface-0/40">
      <td className="py-2 pr-2 align-middle">
        <div className="relative w-full min-w-0" ref={pickerRef}>
          <input
            className={`${compactInput} font-mono ${hasRemote ? "pr-7" : ""}`}
            value={modelId}
            onChange={(e) => setModelId(e.target.value)}
            placeholder="Model ID"
            title="可手动输入 Model ID"
          />
          {hasRemote ? (
            <button
              type="button"
              className="absolute right-0.5 top-1/2 flex h-6 w-6 -translate-y-1/2 items-center justify-center rounded text-ink-3 hover:bg-surface-3 hover:text-ink-1"
              title="从已获取列表选择"
              aria-label="从已获取列表选择"
              aria-expanded={pickerOpen}
              onClick={() => setPickerOpen((v) => !v)}
            >
              ▾
            </button>
          ) : null}
          {hasRemote && pickerOpen ? (
            <ul className="absolute left-0 top-full z-20 mt-1 max-h-44 min-w-full overflow-auto rounded-md border border-surface-3 bg-surface-1 py-1 shadow-lg">
              {remoteModels.map((x) => (
                <li key={x.id}>
                  <button
                    type="button"
                    className={`flex w-full flex-col items-start px-2 py-1 text-left hover:bg-surface-2 ${
                      x.id === modelId ? "bg-accent/10 text-accent" : "text-ink-1"
                    }`}
                    onClick={() => pickRemote(x.id)}
                  >
                    <span className="font-mono text-xs leading-tight">{x.id}</span>
                    {x.name && x.name !== x.id ? (
                      <span className="text-[10px] leading-tight text-ink-3">{x.name}</span>
                    ) : null}
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      </td>
      <td className="py-2 pr-2 align-middle">
        <input
          className={compactInput}
          value={displayName}
          onChange={(e) => setDisplayName(e.target.value)}
        />
      </td>
      <td className="py-2 align-middle">
        <button
          type="button"
          className={
            enabled
              ? "rounded-md border border-ok/40 bg-ok/15 px-2 py-1 text-xs text-ok"
              : "rounded-md border border-surface-3 bg-surface-0 px-2 py-1 text-xs text-ink-3"
          }
          onClick={() => setEnabled((v) => !v)}
        >
          {enabled ? "已启用" : "已禁用"}
        </button>
      </td>
      <td className="py-2 align-middle">
        <span className="text-xs text-ink-3">—</span>
      </td>
      <td className="py-2 text-right align-middle">
        <div className="flex justify-end gap-1">
          <button type="button" className="btn-secondary !px-2 !py-1 text-xs" disabled={busy} onClick={onCancel}>
            取消
          </button>
          <button
            type="button"
            className="btn-primary !px-2 !py-1 text-xs"
            disabled={busy || !modelId.trim()}
            onClick={async () => {
              setBusy(true);
              try {
                await api.updateModel(model.id, {
                  providerId: model.providerId,
                  modelId: modelId.trim(),
                  displayName: displayName.trim() || modelId.trim(),
                  enabled,
                  capabilities: model.capabilities,
                });
                await onSaved();
              } catch (e) {
                onError(`保存失败：${e instanceof Error ? e.message : String(e)}`);
                setBusy(false);
              }
            }}
          >
            保存
          </button>
        </div>
      </td>
    </tr>
  );
}

function ProviderFormModal({
  title,
  initial,
  onClose,
  onSubmit,
}: {
  readonly title: string;
  readonly initial?: Provider;
  readonly onClose: () => void;
  readonly onSubmit: (input: ProviderInput) => Promise<void>;
}) {
  const [name, setName] = useState(initial?.name ?? "");
  const [baseUrl, setBaseUrl] = useState(initial?.baseUrl ?? "");
  const [protocol, setProtocol] = useState<Protocol>(
    initial?.protocol ?? "openai-completions",
  );
  const [apiKey, setApiKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [notes, setNotes] = useState(initial?.notes ?? "");
  const [enabled, setEnabled] = useState(initial?.enabled ?? true);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  return (
    <Modal onClose={onClose} wide>
      <h3 className="mb-4 text-base font-semibold">{title}</h3>
      <div className="space-y-3">
        <div>
          <label className="label">名称</label>
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
        </div>
        <div>
          <label className="label">Base URL</label>
          <input className="input font-mono" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} />
        </div>
        <div>
          <label className="label">协议</label>
          <select
            className="input"
            value={protocol}
            onChange={(e) => setProtocol(e.target.value as Protocol)}
          >
            {PROTOCOLS.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="label">
            API Key {initial ? "（留空则不修改）" : ""}
          </label>
          <div className="flex gap-2">
            <input
              className="input font-mono"
              type={showKey ? "text" : "password"}
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={initial ? "•••• 不修改" : ""}
            />
            <button type="button" className="btn-secondary shrink-0" onClick={() => setShowKey((v) => !v)}>
              {showKey ? "隐藏" : "显示"}
            </button>
          </div>
        </div>
        <div>
          <label className="label">备注</label>
          <textarea className="input min-h-[72px]" value={notes} onChange={(e) => setNotes(e.target.value)} />
        </div>
        <label className="flex items-center gap-2 text-sm text-ink-2">
          <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
          启用同步到 OpenCode / Pi
        </label>
        {err ? <div className="text-sm text-danger">{err}</div> : null}
      </div>
      <div className="mt-5 flex justify-end gap-2">
        <button type="button" className="btn-secondary" onClick={onClose} disabled={busy}>
          取消
        </button>
        <button
          type="button"
          className="btn-primary"
          disabled={busy}
          onClick={async () => {
            setErr(null);
            if (!name.trim() || !baseUrl.trim()) {
              setErr("名称与 Base URL 必填");
              return;
            }
            if (!initial && !apiKey.trim()) {
              setErr("新建时 API Key 必填");
              return;
            }
            setBusy(true);
            try {
              await onSubmit({
                name: name.trim(),
                baseUrl: baseUrl.trim(),
                protocol,
                apiKey: apiKey.trim(),
                enabled,
                notes,
                headers: initial?.headers ?? {},
                compat: initial?.compat ?? {},
              });
            } catch (e) {
              setErr(e instanceof Error ? e.message : String(e));
              setBusy(false);
            }
          }}
        >
          保存
        </button>
      </div>
    </Modal>
  );
}

function ModelFormModal({
  providerId,
  existingIds,
  remoteModels,
  onClose,
  onSubmit,
}: {
  readonly providerId: string;
  readonly existingIds: ReadonlySet<string>;
  readonly remoteModels: readonly RemoteModel[];
  readonly onClose: () => void;
  readonly onSubmit: (
    items: readonly { modelId: string; displayName: string; enabled: boolean }[],
  ) => Promise<void>;
}) {
  const available = remoteModels.filter((m) => !existingIds.has(m.id));
  const [mode, setMode] = useState<"pick" | "manual">(available.length > 0 ? "pick" : "manual");
  const [picked, setPicked] = useState<string[]>(available.slice(0, 1).map((m) => m.id));
  const [modelId, setModelId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [busy, setBusy] = useState(false);
  void providerId;

  const togglePick = (id: string) => {
    setPicked((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  };

  return (
    <Modal onClose={onClose} wide>
      <h3 className="mb-4 text-base font-semibold">添加模型</h3>
      <div className="mb-3 flex gap-2">
        <button
          type="button"
          className={mode === "pick" ? "btn-primary" : "btn-secondary"}
          onClick={() => setMode("pick")}
          disabled={available.length === 0}
        >
          从已获取列表选择
        </button>
        <button
          type="button"
          className={mode === "manual" ? "btn-primary" : "btn-secondary"}
          onClick={() => setMode("manual")}
        >
          手动填写
        </button>
      </div>

      {mode === "pick" ? (
        <div className="space-y-2">
          {available.length === 0 ? (
            <p className="text-sm text-ink-3">请先点「获取模型」，或改用手动填写。</p>
          ) : (
            <ul className="max-h-64 space-y-1 overflow-auto rounded-md border border-surface-3 p-2">
              {available.map((m) => (
                <li key={m.id}>
                  <label className="flex cursor-pointer items-start gap-2 rounded px-2 py-1.5 text-sm hover:bg-surface-2">
                    <input
                      type="checkbox"
                      className="mt-0.5"
                      checked={picked.includes(m.id)}
                      onChange={() => togglePick(m.id)}
                    />
                    <span>
                      <span className="font-mono text-xs">{m.id}</span>
                      {m.name !== m.id ? (
                        <span className="ml-2 text-ink-3">{m.name}</span>
                      ) : null}
                    </span>
                  </label>
                </li>
              ))}
            </ul>
          )}
          <label className="flex items-center gap-2 text-sm text-ink-2">
            <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
            添加后启用（参与 OC/Pi 同步）
          </label>
        </div>
      ) : (
        <div className="space-y-3">
          <div>
            <label className="label">上游 Model ID</label>
            <input className="input font-mono" value={modelId} onChange={(e) => setModelId(e.target.value)} />
          </div>
          <div>
            <label className="label">展示名</label>
            <input
              className="input"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder="默认同 Model ID"
            />
          </div>
          <label className="flex items-center gap-2 text-sm text-ink-2">
            <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
            启用
          </label>
        </div>
      )}

      <div className="mt-5 flex justify-end gap-2">
        <button type="button" className="btn-secondary" onClick={onClose}>
          取消
        </button>
        <button
          type="button"
          className="btn-primary"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            try {
              if (mode === "pick") {
                if (picked.length === 0) return;
                const items = picked.map((id) => {
                  const meta = available.find((m) => m.id === id);
                  return {
                    modelId: id,
                    displayName: meta?.name || id,
                    enabled,
                  };
                });
                await onSubmit(items);
              } else {
                if (!modelId.trim()) return;
                await onSubmit([
                  {
                    modelId: modelId.trim(),
                    displayName: displayName.trim() || modelId.trim(),
                    enabled,
                  },
                ]);
              }
            } finally {
              setBusy(false);
            }
          }}
        >
          添加
        </button>
      </div>
    </Modal>
  );
}

function CloneModal({
  sourceName,
  onClose,
  onSubmit,
}: {
  readonly sourceName: string;
  readonly onClose: () => void;
  readonly onSubmit: (name: string, key: string) => Promise<void>;
}) {
  const [name, setName] = useState(`${sourceName}-copy`);
  const [key, setKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [busy, setBusy] = useState(false);

  return (
    <Modal onClose={onClose}>
      <h3 className="mb-2 text-base font-semibold">克隆为新实例（换 Key）</h3>
      <p className="mb-4 text-xs text-ink-3">
        不同 Key 视为不同 Provider。会复制 URL/协议/模型列表。
      </p>
      <div className="space-y-3">
        <div>
          <label className="label">新名称</label>
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
        </div>
        <div>
          <label className="label">新 API Key</label>
          <div className="flex gap-2">
            <input
              className="input font-mono"
              type={showKey ? "text" : "password"}
              value={key}
              onChange={(e) => setKey(e.target.value)}
            />
            <button type="button" className="btn-secondary shrink-0" onClick={() => setShowKey((v) => !v)}>
              {showKey ? "隐藏" : "显示"}
            </button>
          </div>
        </div>
      </div>
      <div className="mt-5 flex justify-end gap-2">
        <button type="button" className="btn-secondary" onClick={onClose}>
          取消
        </button>
        <button
          type="button"
          className="btn-primary"
          disabled={busy || !name.trim() || !key.trim()}
          onClick={async () => {
            setBusy(true);
            await onSubmit(name.trim(), key.trim());
          }}
        >
          克隆
        </button>
      </div>
    </Modal>
  );
}
