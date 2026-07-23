import { useCallback, useEffect, useRef, useState } from "react";
import type { AgentBindings, FullState, Model, Provider } from "../types";
import { emptyBindings } from "../types";
import * as api from "../api/tauri";

type Props = {
  readonly state: FullState;
  readonly draft: AgentBindings | null;
  readonly onDraftChange: (draft: AgentBindings | null) => void;
  readonly onToast: (msg: string) => void;
};

export function AgentsPage({ state, draft, onDraftChange, onToast }: Props) {
  const [bindings, setBindings] = useState<AgentBindings>(draft ?? emptyBindings());
  const [loading, setLoading] = useState(!draft);
  const bootstrapped = useRef(draft !== null);

  const loadFromDisk = useCallback(async () => {
    setLoading(true);
    try {
      const live = await api.readLiveBindings();
      setBindings(live);
      onDraftChange(live);
      bootstrapped.current = true;
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
      const empty = emptyBindings();
      setBindings(empty);
      onDraftChange(empty);
      bootstrapped.current = true;
    } finally {
      setLoading(false);
    }
  }, [onDraftChange, onToast]);

  // Initial disk load only once when there is no session draft.
  // Do not re-fetch when switching tabs or when callback identities change.
  useEffect(() => {
    if (draft) {
      setBindings(draft);
      setLoading(false);
      bootstrapped.current = true;
      return;
    }
    if (!bootstrapped.current) {
      void loadFromDisk();
    }
  }, [draft, loadFromDisk]);

  const patch = (next: AgentBindings) => {
    setBindings(next);
    onDraftChange(next);
  };

  const providers = state.store.providers;
  const modelsOf = (providerId: string | null): readonly Model[] =>
    providerId ? state.store.models.filter((m) => m.providerId === providerId) : [];

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-ink-3">
        正在读取各 Agent 磁盘配置…
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="text-sm text-ink-2">
          <p>
            首次进入 / 点「重置」：从<strong className="text-ink-1">磁盘真实配置</strong>加载。
          </p>
          <p className="mt-0.5 text-xs text-ink-3">
            修改会即时同步到会话草稿，并用于「应用同步」；不写 store、不改 Agent
            文件。切换 Tab 不会丢失；关闭应用后草稿清空。
          </p>
        </div>
        <button
          type="button"
          className="btn-secondary"
          disabled={loading}
          onClick={() => void loadFromDisk()}
        >
          重置
        </button>
      </div>

      <div className="grid grid-cols-2 gap-4">
        <AgentCard title="Claude Code" path={state.paths.claudeSettings} exists={state.paths.claudeExists}>
          <ModeToggle
            mode={bindings.claude.mode}
            onChange={(mode) =>
              patch({
                ...bindings,
                claude: { ...bindings.claude, mode },
              })
            }
          />
          {bindings.claude.mode === "third_party" ? (
            <ProviderModelPickers
              providers={providers}
              providerId={bindings.claude.providerId}
              modelId={bindings.claude.modelId}
              models={modelsOf(bindings.claude.providerId)}
              onProvider={(providerId) =>
                patch({
                  ...bindings,
                  claude: {
                    ...bindings.claude,
                    providerId,
                    modelId: firstModelId(state, providerId),
                  },
                })
              }
              onModel={(modelId) =>
                patch({
                  ...bindings,
                  claude: { ...bindings.claude, modelId },
                })
              }
            />
          ) : (
            <p className="text-xs text-ink-3">官方模式：Apply 时清除 BASE_URL/TOKEN 劫持。</p>
          )}
          {!bindings.claude.providerId && bindings.claude.mode === "third_party" ? (
            <p className="text-xs text-warn">
              磁盘上有第三方 baseUrl，但未匹配到 ModelHub 中的提供商（可先导入/新建）。
            </p>
          ) : null}
        </AgentCard>

        <AgentCard title="Codex" path={state.paths.codexConfig} exists={state.paths.codexExists}>
          <ModeToggle
            mode={bindings.codex.mode}
            onChange={(mode) =>
              patch({
                ...bindings,
                codex: { ...bindings.codex, mode },
              })
            }
          />
          {bindings.codex.mode === "third_party" ? (
            <>
              <ProviderModelPickers
                providers={providers}
                providerId={bindings.codex.providerId}
                modelId={bindings.codex.modelId}
                models={modelsOf(bindings.codex.providerId)}
                onProvider={(providerId) =>
                  patch({
                    ...bindings,
                    codex: {
                      ...bindings.codex,
                      providerId,
                      modelId: firstModelId(state, providerId),
                    },
                  })
                }
                onModel={(modelId) =>
                  patch({
                    ...bindings,
                    codex: { ...bindings.codex, modelId },
                  })
                }
              />
              {bindings.codex.providerId
                ? protocolWarn(providers, bindings.codex.providerId)
                : null}
              <p className="text-xs text-ink-3">
                磁盘 model_provider 槽：{bindings.codex.providerKey || "—"}
              </p>
            </>
          ) : (
            <p className="text-xs text-ink-3">官方模式：model_provider=openai。</p>
          )}
        </AgentCard>

        <AgentCard
          title="OpenCode"
          path={state.paths.opencodeConfig}
          exists={state.paths.opencodeExists}
        >
          <ProviderModelPickers
            providers={providers.filter((p) => p.enabled)}
            providerId={bindings.opencode.providerId}
            modelId={bindings.opencode.modelId}
            models={modelsOf(bindings.opencode.providerId)}
            onProvider={(providerId) =>
              patch({
                ...bindings,
                opencode: {
                  ...bindings.opencode,
                  providerId,
                  modelId: firstModelId(state, providerId),
                },
              })
            }
            onModel={(modelId) =>
              patch({
                ...bindings,
                opencode: { ...bindings.opencode, modelId },
              })
            }
          />
          <EnabledPreview providers={providers} />
        </AgentCard>

        <AgentCard title="Pi" path={state.paths.piModels} exists={state.paths.piExists}>
          <ProviderModelPickers
            providers={providers.filter((p) => p.enabled)}
            providerId={bindings.pi.providerId}
            modelId={bindings.pi.modelId}
            models={modelsOf(bindings.pi.providerId)}
            onProvider={(providerId) =>
              patch({
                ...bindings,
                pi: {
                  ...bindings.pi,
                  providerId,
                  modelId: firstModelId(state, providerId),
                },
              })
            }
            onModel={(modelId) =>
              patch({
                ...bindings,
                pi: { ...bindings.pi, modelId },
              })
            }
          />
          <EnabledPreview providers={providers} />
        </AgentCard>
      </div>
    </div>
  );
}

function firstModelId(state: FullState, providerId: string | null): string | null {
  if (!providerId) return null;
  const models = state.store.models.filter((m) => m.providerId === providerId);
  return models[0]?.id ?? null;
}

function protocolWarn(providers: readonly Provider[], providerId: string) {
  const p = providers.find((x) => x.id === providerId);
  if (!p) return null;
  if (p.protocol === "openai-responses") return null;
  return (
    <div className="rounded-md border border-warn/40 bg-warn/10 px-3 py-2 text-xs text-warn">
      当前协议为 {p.protocol}，Codex 通常需要 openai-responses，可能不可用。
    </div>
  );
}

function EnabledPreview({ providers }: { readonly providers: readonly Provider[] }) {
  const enabled = providers.filter((p) => p.enabled);
  return (
    <div className="rounded-md bg-surface-0 px-3 py-2 text-xs text-ink-3">
      Apply 时将全量同步 {enabled.length} 个 enabled 提供商：
      {enabled.length === 0 ? "（无）" : enabled.map((p) => p.name).join("、")}
    </div>
  );
}

function AgentCard({
  title,
  path,
  exists,
  children,
}: {
  readonly title: string;
  readonly path: string;
  readonly exists: boolean;
  readonly children: React.ReactNode;
}) {
  return (
    <div className="card flex flex-col gap-3 p-4">
      <div>
        <h3 className="font-semibold">{title}</h3>
        <p className="mt-1 font-mono text-[11px] text-ink-3">
          {exists ? "已检测到" : "未找到"} · {path}
        </p>
      </div>
      {children}
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
