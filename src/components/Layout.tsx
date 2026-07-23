import type { PageId } from "../types";
import { Toast } from "./Toast";

const NAV: readonly { id: PageId; label: string }[] = [
  { id: "providers", label: "提供商" },
  { id: "agents", label: "Agent 绑定" },
  { id: "apply", label: "应用同步" },
  { id: "import", label: "导入" },
  { id: "backups", label: "备份" },
  { id: "settings", label: "设置" },
] as const;

type Props = {
  readonly page: PageId;
  readonly onNavigate: (page: PageId) => void;
  readonly onApply: () => void;
  readonly toast: string | null;
  readonly children: React.ReactNode;
};

export function Layout({ page, onNavigate, onApply, toast, children }: Props) {
  return (
    <div className="relative flex h-full bg-surface-0 text-ink-1">
      <aside className="flex w-52 shrink-0 flex-col border-r border-surface-3 bg-surface-1">
        <div className="border-b border-surface-3 px-4 py-4">
          <div className="text-lg font-semibold tracking-tight">ModelHub</div>
          <div className="mt-0.5 text-xs text-ink-3">Provider-first 模型中枢</div>
        </div>
        <nav className="flex flex-1 flex-col gap-0.5 p-2">
          {NAV.map((item) => {
            const active = page === item.id;
            return (
              <button
                key={item.id}
                type="button"
                className={
                  active
                    ? "rounded-md bg-accent/15 px-3 py-2 text-left text-sm font-medium text-accent"
                    : "rounded-md px-3 py-2 text-left text-sm text-ink-2 hover:bg-surface-3 hover:text-ink-1"
                }
                onClick={() => onNavigate(item.id)}
              >
                {item.label}
              </button>
            );
          })}
        </nav>
        <div className="border-t border-surface-3 p-3 text-[11px] text-ink-3">
          只管理模型配置
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center justify-between border-b border-surface-3 bg-surface-1 px-5">
          <div className="text-sm text-ink-2">
            {NAV.find((n) => n.id === page)?.label ?? ""}
          </div>
          <button type="button" className="btn-primary" onClick={onApply}>
            应用更改
          </button>
        </header>
        <main className="min-h-0 flex-1 overflow-auto p-5">{children}</main>
      </div>

      <Toast message={toast} />
    </div>
  );
}
