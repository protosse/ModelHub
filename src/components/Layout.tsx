import { useCallback, useState } from "react";
import type { PageId } from "../types";
import { Toast } from "./Toast";

const SIDEBAR_COLLAPSED_KEY = "modelhub.sidebarCollapsed";

type NavItem = {
  readonly id: PageId;
  readonly label: string;
  readonly icon: React.ReactNode;
};

function IconPlus({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M12 5v14M5 12h14"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
      />
    </svg>
  );
}

function IconProviders({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M4 7.5A2.5 2.5 0 0 1 6.5 5h11A2.5 2.5 0 0 1 20 7.5v0A2.5 2.5 0 0 1 17.5 10h-11A2.5 2.5 0 0 1 4 7.5v0ZM4 16.5A2.5 2.5 0 0 1 6.5 14h11a2.5 2.5 0 0 1 2.5 2.5v0a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 16.5v0Z"
        stroke="currentColor"
        strokeWidth="1.75"
      />
      <circle cx="8" cy="7.5" r="1" fill="currentColor" />
      <circle cx="8" cy="16.5" r="1" fill="currentColor" />
    </svg>
  );
}

function IconModels({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M12 3.5 20 8v8l-8 4.5L4 16V8l8-4.5Z"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinejoin="round"
      />
      <path d="M12 12 20 8M12 12v8.5M12 12 4 8" stroke="currentColor" strokeWidth="1.75" />
    </svg>
  );
}

function IconAgents({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <rect x="5" y="7" width="14" height="11" rx="3" stroke="currentColor" strokeWidth="1.75" />
      <path
        d="M12 3.5v3.5M9 18v2.5M15 18v2.5"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
      />
      <circle cx="9.5" cy="12.5" r="1.1" fill="currentColor" />
      <circle cx="14.5" cy="12.5" r="1.1" fill="currentColor" />
    </svg>
  );
}

function IconImport({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M12 4v10m0 0 3.5-3.5M12 14 8.5 10.5"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M5 16.5V18a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-1.5"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
      />
    </svg>
  );
}

function IconBackups({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M6.5 8.5h11a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-11a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2Z"
        stroke="currentColor"
        strokeWidth="1.75"
      />
      <path
        d="M8 8.5V7a4 4 0 0 1 8 0v1.5"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
      />
      <path d="M12 12.5v3" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" />
    </svg>
  );
}

function IconSettings({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <circle cx="12" cy="12" r="3" stroke="currentColor" strokeWidth="1.75" />
      <path
        d="M12 3.5v2.2M12 18.3v2.2M4.9 6.5l1.6 1.6M17.5 15.9l1.6 1.6M3.5 12h2.2M18.3 12h2.2M4.9 17.5l1.6-1.6M17.5 8.1l1.6-1.6"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
      />
    </svg>
  );
}

function IconCollapse({ className, collapsed }: { className?: string; collapsed: boolean }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
      style={{ transform: collapsed ? "scaleX(-1)" : undefined }}
    >
      <path
        d="M14.5 6.5 9 12l5.5 5.5"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

const iconClass = "h-[18px] w-[18px]";

const NAV: readonly NavItem[] = [
  { id: "providers", label: "提供商", icon: <IconProviders className={iconClass} /> },
  { id: "models", label: "模型一览", icon: <IconModels className={iconClass} /> },
  { id: "agents", label: "Agent 应用", icon: <IconAgents className={iconClass} /> },
  { id: "import", label: "导入", icon: <IconImport className={iconClass} /> },
  { id: "backups", label: "备份", icon: <IconBackups className={iconClass} /> },
  { id: "settings", label: "设置", icon: <IconSettings className={iconClass} /> },
] as const;

function readCollapsedPreference(): boolean {
  try {
    return window.localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

type Props = {
  readonly page: PageId;
  readonly onNavigate: (page: PageId) => void;
  readonly onQuickAdd: () => void;
  readonly toast: string | null;
  readonly children: React.ReactNode;
};

export function Layout({ page, onNavigate, onQuickAdd, toast, children }: Props) {
  const [collapsed, setCollapsed] = useState(readCollapsedPreference);

  const toggleCollapsed = useCallback(() => {
    setCollapsed((prev) => {
      const next = !prev;
      try {
        window.localStorage.setItem(SIDEBAR_COLLAPSED_KEY, next ? "1" : "0");
      } catch {
        // ignore quota / private mode
      }
      return next;
    });
  }, []);

  const asideWidth = collapsed ? "w-14" : "w-52";
  const navPad = collapsed ? "p-1.5" : "p-2";

  return (
    <div className="relative flex h-full bg-surface-0 text-ink-1">
      <aside
        className={"flex shrink-0 flex-col border-r border-surface-3 bg-surface-1 transition-[width] duration-200 ease-out " + asideWidth}
      >
        <div
          className={
            collapsed
              ? "flex h-[4.25rem] items-center justify-center border-b border-surface-3 px-1"
              : "border-b border-surface-3 px-4 py-4"
          }
        >
          {collapsed ? (
            <div
              className="flex h-9 w-9 items-center justify-center rounded-lg bg-accent/15 text-sm font-semibold text-accent"
              title="ModelHub"
            >
              M
            </div>
          ) : (
            <>
              <div className="text-lg font-semibold tracking-tight">ModelHub</div>
              <div className="mt-0.5 text-xs text-ink-3">Provider-first 模型中枢</div>
            </>
          )}
        </div>

        <nav className={"flex flex-1 flex-col gap-0.5 " + navPad}>
          <button
            type="button"
            title="快速添加提供商"
            aria-label="快速添加提供商"
            className={
              collapsed
                ? "mb-1 flex items-center justify-center rounded-md bg-accent/15 px-0 py-2.5 text-accent hover:bg-accent/25"
                : "mb-1 flex items-center gap-2.5 rounded-md bg-accent/15 px-3 py-2 text-left text-sm font-medium text-accent hover:bg-accent/25"
            }
            onClick={onQuickAdd}
          >
            <IconPlus className={iconClass} />
            {collapsed ? null : <span className="truncate">快速添加</span>}
          </button>
          <div className="mb-1 border-t border-surface-3" />
          {NAV.map((item) => {
            const active = page === item.id;
            const base = collapsed
              ? "flex items-center justify-center rounded-md px-0 py-2.5"
              : "flex items-center gap-2.5 rounded-md px-3 py-2 text-left text-sm";
            const tone = active
              ? " bg-accent/15 font-medium text-accent"
              : " text-ink-2 hover:bg-surface-3 hover:text-ink-1";
            return (
              <button
                key={item.id}
                type="button"
                title={item.label}
                aria-label={item.label}
                aria-current={active ? "page" : undefined}
                className={base + tone}
                onClick={() => onNavigate(item.id)}
              >
                <span className="shrink-0">{item.icon}</span>
                {collapsed ? null : <span className="truncate">{item.label}</span>}
              </button>
            );
          })}
        </nav>

        <div className={"border-t border-surface-3 " + navPad}>
          {!collapsed ? (
            <div className="mb-2 px-1 text-[11px] text-ink-3">只管理模型配置</div>
          ) : null}
          <button
            type="button"
            className={
              collapsed
                ? "flex w-full items-center justify-center rounded-md px-0 py-2.5 text-ink-2 hover:bg-surface-3 hover:text-ink-1"
                : "flex w-full items-center gap-2.5 rounded-md px-3 py-2 text-left text-sm text-ink-2 hover:bg-surface-3 hover:text-ink-1"
            }
            title={collapsed ? "展开侧栏" : "收起侧栏"}
            aria-label={collapsed ? "展开侧栏" : "收起侧栏"}
            aria-expanded={!collapsed}
            onClick={toggleCollapsed}
          >
            <IconCollapse className="h-[18px] w-[18px] shrink-0" collapsed={collapsed} />
            {collapsed ? null : <span>收起侧栏</span>}
          </button>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center border-b border-surface-3 bg-surface-1 px-5">
          <div className="text-sm text-ink-2">
            {NAV.find((n) => n.id === page)?.label ?? ""}
          </div>
        </header>
        <main className="min-h-0 flex-1 overflow-hidden p-5">{children}</main>
      </div>

      <Toast message={toast} />
    </div>
  );
}
