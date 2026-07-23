import { useEffect, useMemo, useState } from "react";
import type {
  FullState,
  ImportAction,
  ImportItemDecision,
  ImportPreview,
  ImportPreviewItem,
} from "../types";
import * as api from "../api/tauri";

type Props = {
  readonly state: FullState;
  readonly onRefresh: () => Promise<void>;
  readonly onToast: (msg: string) => void;
};

type RowState = {
  readonly id: string;
  name: string;
  action: ImportAction;
  selected: boolean;
};

function defaultAction(item: ImportPreviewItem): ImportAction {
  if (item.alreadyExists) return "override";
  return "import";
}

export function ImportPage({ state, onRefresh, onToast }: Props) {
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [rows, setRows] = useState<RowState[]>([]);
  const [busy, setBusy] = useState(false);

  const load = async (preserveSelection = false) => {
    setBusy(true);
    try {
      const p = await api.previewImport();
      setPreview(p);
      setRows((prev) => {
        const prevById = new Map(prev.map((r) => [r.id, r]));
        return p.items.map((item) => {
          const old = prevById.get(item.id);
          if (preserveSelection && old) {
            return {
              id: item.id,
              name: old.name.trim() ? old.name : item.name,
              action: item.alreadyExists
                ? old.action === "import"
                  ? "override"
                  : old.action
                : old.action,
              selected: item.alreadyExists ? false : old.selected,
            };
          }
          return {
            id: item.id,
            name: item.name,
            action: defaultAction(item),
            selected: !item.alreadyExists,
          };
        });
      });
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const itemById = useMemo(() => {
    const m = new Map<string, ImportPreviewItem>();
    for (const it of preview?.items ?? []) m.set(it.id, it);
    return m;
  }, [preview]);

  const selectedCount = rows.filter((r) => r.selected).length;

  const updateRow = (id: string, patch: Partial<RowState>) => {
    setRows((prev) => prev.map((r) => (r.id === id ? { ...r, ...patch } : r)));
  };

  const validateBeforeImport = (): string | null => {
    const selected = rows.filter((r) => r.selected && r.action !== "skip");
    if (selected.length === 0) return "请至少选择一项导入/覆盖";

    const names = selected.map((r) => r.name.trim().toLowerCase());
    if (names.some((n) => !n)) return "名称不能为空";
    const dup = names.find((n, i) => names.indexOf(n) !== i);
    if (dup) return `导入列表中存在重复名称：${dup}`;

    for (const r of selected) {
      const item = itemById.get(r.id);
      if (!item) continue;
      const name = r.name.trim();
      if (r.action === "import") {
        if (item.alreadyExists) {
          return `「${name}」端点已存在，请改为覆盖或取消勾选`;
        }
        const clash = state.store.providers.some(
          (p) => p.name.toLowerCase() === name.toLowerCase(),
        );
        if (clash) {
          return `名称「${name}」已存在，请改名或选择覆盖`;
        }
      }
    }
    return null;
  };

  return (
    <div className="mx-auto max-w-5xl space-y-4">
      <div className="card p-4">
        <h3 className="mb-3 font-semibold">本机 Agent 检测</h3>
        <ul className="space-y-2 text-sm">
          <Det label="Claude" ok={state.paths.claudeExists} path={state.paths.claudeSettings} />
          <Det label="Codex" ok={state.paths.codexExists} path={state.paths.codexConfig} />
          <Det label="OpenCode" ok={state.paths.opencodeExists} path={state.paths.opencodeConfig} />
          <Det label="Pi" ok={state.paths.piExists} path={state.paths.piModels} />
        </ul>
        <div className="mt-4 flex flex-wrap gap-2">
          <button
            type="button"
            className="btn-secondary"
            disabled={busy}
            onClick={() => void load(false)}
          >
            刷新预览
          </button>
          <button
            type="button"
            className="btn-secondary"
            disabled={busy || rows.length === 0}
            onClick={() =>
              setRows((prev) =>
                prev.map((r) => ({
                  ...r,
                  selected: !(itemById.get(r.id)?.alreadyExists ?? false),
                })),
              )
            }
          >
            全选可导入
          </button>
          <button
            type="button"
            className="btn-secondary"
            disabled={busy || selectedCount === 0}
            onClick={() => setRows((prev) => prev.map((r) => ({ ...r, selected: false })))}
          >
            清空选择
          </button>
          <button
            type="button"
            className="btn-primary min-w-[9.5rem]"
            disabled={busy || selectedCount === 0}
            onClick={async () => {
              const err = validateBeforeImport();
              if (err) {
                onToast(err);
                return;
              }
              setBusy(true);
              try {
                const items: ImportItemDecision[] = rows
                  .filter((r) => r.selected)
                  .map((r) => ({
                    id: r.id,
                    name: r.name.trim(),
                    action: r.action,
                  }));
                const res = await api.runImport(items);
                await onRefresh();
                await load(true);
                onToast(
                  `导入完成：新增 ${res.importedProviders}，覆盖 ${res.overridden}，模型 +${res.importedModels}，跳过 ${res.skipped}`,
                );
              } catch (e) {
                onToast(e instanceof Error ? e.message : String(e));
              } finally {
                setBusy(false);
              }
            }}
          >
            {selectedCount > 0 ? `导入所选 (${selectedCount})` : "导入所选"}
          </button>
        </div>
        <p className="mt-3 text-xs text-ink-3">
          同一 baseUrl + 协议 视为同一提供商，会跨 Agent 合并成一行（来源可能是
          opencode+pi+codex）。名称需唯一：与现有同名可改名或选覆盖。
          Codex 从 ~/.codex/config.toml 的 [model_providers.xxx] 读取；若与
          OpenCode/Pi 同地址，不会单独再出一行。
        </p>
      </div>

      <div className="card overflow-x-auto p-4">
        <h3 className="mb-3 font-semibold">
          预览 {preview ? `（${preview.items.length}）` : ""}
        </h3>
        {!preview ? (
          <p className="text-sm text-ink-3">加载中…</p>
        ) : preview.items.length === 0 ? (
          <p className="text-sm text-ink-3">未发现可导入的 Provider</p>
        ) : (
          <table className="w-full min-w-[900px] text-left text-sm">
            <thead className="text-xs text-ink-3">
              <tr>
                <th className="w-10 pb-2" />
                <th className="pb-2">来源</th>
                <th className="pb-2">名称（可改）</th>
                <th className="pb-2">URL</th>
                <th className="pb-2">协议</th>
                <th className="pb-2">模型</th>
                <th className="pb-2">状态</th>
                <th className="pb-2">动作</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const item = itemById.get(row.id);
                if (!item) return null;
                return (
                  <tr key={row.id} className="border-t border-surface-3 align-top">
                    <td className="py-2">
                      <input
                        type="checkbox"
                        checked={row.selected}
                        onChange={(e) => updateRow(row.id, { selected: e.target.checked })}
                      />
                    </td>
                    <td className="py-2">
                      <div className="flex max-w-[140px] flex-wrap gap-1">
                        {item.source.split("+").map((s) => (
                          <span key={s} className="badge bg-surface-3 text-ink-2">
                            {s}
                          </span>
                        ))}
                      </div>
                    </td>
                    <td className="py-2">
                      <input
                        className="input !py-1 text-sm"
                        value={row.name}
                        onChange={(e) => updateRow(row.id, { name: e.target.value })}
                      />
                      {item.nameConflict ? (
                        <div className="mt-1 text-[11px] text-warn">名称可能冲突，建议改名</div>
                      ) : null}
                    </td>
                    <td className="max-w-[180px] truncate py-2 font-mono text-xs">{item.baseUrl}</td>
                    <td className="py-2 font-mono text-xs">{item.protocol}</td>
                    <td className="py-2">{item.modelCount}</td>
                    <td className="py-2 text-xs">
                      {item.alreadyExists ? (
                        <span className="text-warn">端点已存在</span>
                      ) : (
                        <span className="text-ok">新</span>
                      )}
                      {!item.hasApiKey ? <div className="text-ink-3">无 Key</div> : null}
                    </td>
                    <td className="py-2">
                      <select
                        className="input !py-1 text-xs"
                        value={row.action}
                        onChange={(e) =>
                          updateRow(row.id, { action: e.target.value as ImportAction })
                        }
                      >
                        <option value="import">导入（新建）</option>
                        <option value="override">覆盖已有</option>
                        <option value="skip">跳过</option>
                      </select>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

function Det({
  label,
  ok,
  path,
}: {
  readonly label: string;
  readonly ok: boolean;
  readonly path: string;
}) {
  return (
    <li className="flex items-start gap-3">
      <span className={ok ? "text-ok" : "text-ink-3"}>{ok ? "●" : "○"}</span>
      <div>
        <div className="font-medium">{label}</div>
        <div className="font-mono text-[11px] text-ink-3">{path}</div>
      </div>
    </li>
  );
}
