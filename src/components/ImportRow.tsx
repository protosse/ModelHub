import type { ImportAction, ImportPreviewItem, Protocol } from "../types";

/** Merged per-item state: backend preview data + user-editable fields. */
export type ImportItem = ImportPreviewItem & {
  selected: boolean;
  error: string | null;
};

type Props = {
  readonly item: ImportItem;
  readonly action: ImportAction;
  readonly expanded: boolean;
  readonly importing: boolean;
  readonly dimmed: boolean;
  readonly nameConflict: boolean;
  readonly onToggleSelect: (selected: boolean) => void;
  readonly onNameChange: (name: string) => void;
  readonly onToggleExpand: () => void;
  readonly onAutoRename: () => void;
};

function protocolBadge(p: Protocol): string {
  switch (p) {
    case "openai-completions":
      return "completions";
    case "openai-responses":
      return "responses";
    case "anthropic-messages":
      return "anthropic";
    default:
      return p;
  }
}

export function ImportRow({
  item,
  action,
  expanded,
  importing,
  dimmed,
  nameConflict,
  onToggleSelect,
  onNameChange,
  onToggleExpand,
  onAutoRename,
}: Props) {
  const modelIds = item.modelIds ?? [];
  const newModelIds = item.newModelIds ?? (!item.alreadyExists ? modelIds : []);
  const newSet = new Set(newModelIds);

  return (
    <li
      id={`import-row-${item.id}`}
      className={
        item.error
          ? "bg-danger/5 px-3 py-3"
          : dimmed
            ? "px-3 py-3 opacity-55 hover:bg-surface-2/40 hover:opacity-90"
            : "px-3 py-3 hover:bg-surface-2/40"
      }
      onClick={(e) => {
        const t = e.target as HTMLElement;
        if (t.closest("input,button,a,textarea,select,label")) return;
        if (importing) return;
        onToggleSelect(!item.selected);
      }}
    >
      <div className="flex gap-3">
        <input
          type="checkbox"
          className="mt-1"
          checked={item.selected}
          disabled={importing}
          onChange={(e) => onToggleSelect(e.target.checked)}
          onClick={(e) => e.stopPropagation()}
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <input
              className={
                item.error
                  ? "input !border-danger max-w-xs !py-1 text-sm"
                  : "input max-w-xs !py-1 text-sm"
              }
              value={item.name}
              disabled={importing}
              onChange={(e) => onNameChange(e.target.value)}
              onClick={(e) => e.stopPropagation()}
            />
            {item.alreadyExists ? (
              <span className="badge bg-warn/15 text-warn">已存在</span>
            ) : (
              <span className="badge bg-ok/15 text-ok">新</span>
            )}
            {item.alreadyExists && (item.extraModelCount ?? 0) > 0 ? (
              <span className="badge bg-accent/15 text-accent">
                +{item.extraModelCount} 可补模型
              </span>
            ) : null}
            {!item.hasApiKey ? (
              <span className="badge bg-surface-3 text-ink-3">无 Key</span>
            ) : null}
            <span className="badge bg-surface-3 font-mono text-[10px] text-ink-2">
              {protocolBadge(item.protocol)}
            </span>
            <button
              type="button"
              className="text-xs text-ink-3 underline hover:text-ink-2"
              disabled={modelIds.length === 0}
              onClick={(e) => {
                e.stopPropagation();
                onToggleExpand();
              }}
            >
              {item.modelCount} 模型{expanded ? " ▴" : " ▾"}
            </button>
            {item.selected ? (
              <span className="text-xs text-ink-3">
                → {action === "override" ? "覆盖（增量）" : "导入"}
              </span>
            ) : (
              <span className="text-xs text-ink-3">→ 跳过</span>
            )}
          </div>
          <div className="mt-1 truncate font-mono text-[11px] text-ink-3" title={item.baseUrl}>
            {item.baseUrl}
          </div>
          <div className="mt-1.5 flex flex-wrap items-center gap-1">
            {item.source.split("+").map((s) => (
              <span key={s} className="badge bg-surface-3 text-ink-2">
                {s}
              </span>
            ))}
            {item.alreadyExists && item.existingName ? (
              <span className="text-[11px] text-ink-3">库中：{item.existingName}</span>
            ) : null}
            {nameConflict || item.error?.includes("名") ? (
              <button
                type="button"
                className="text-[11px] text-accent underline"
                disabled={importing}
                onClick={(e) => {
                  e.stopPropagation();
                  onAutoRename();
                }}
              >
                自动改名
              </button>
            ) : null}
          </div>
          {item.alreadyExists && (item.extraModelCount ?? 0) > 0 && !item.selected ? (
            <div className="mt-1 text-[11px] text-accent">
              本机多 {item.extraModelCount} 个模型未入库，勾选后覆盖可增量补入
            </div>
          ) : null}
          {expanded && modelIds.length > 0 ? (
            <div className="mt-2 rounded-md bg-surface-0 px-2 py-1.5 font-mono text-[11px]">
              {modelIds.slice(0, 30).map((m) => {
                const isNew = !item.alreadyExists || newSet.has(m);
                return (
                  <div
                    key={m}
                    className={isNew ? "truncate text-ok" : "truncate text-ink-3"}
                    title={isNew ? "将新增" : "已存在，跳过"}
                  >
                    {m}
                  </div>
                );
              })}
              {modelIds.length > 30 ? (
                <div className="text-ink-3">…共 {modelIds.length} 个</div>
              ) : null}
            </div>
          ) : null}
          {item.error ? (
            <div className="mt-1 text-xs text-danger">{item.error}</div>
          ) : nameConflict ? (
            <div className="mt-1 text-[11px] text-warn">名称冲突，请改名或点「自动改名」</div>
          ) : null}
        </div>
      </div>
    </li>
  );
}
