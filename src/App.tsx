import { useCallback, useEffect, useState } from "react";
import { Layout } from "./components/Layout";
import { AgentsPage } from "./pages/AgentsPage";
import { ApplyPage } from "./pages/ApplyPage";
import { BackupsPage } from "./pages/BackupsPage";
import { ImportPage } from "./pages/ImportPage";
import { ProvidersPage } from "./pages/ProvidersPage";
import { SettingsPage } from "./pages/SettingsPage";
import * as api from "./api/tauri";
import type { AgentBindings, FullState, PageId } from "./types";
import { emptyBindings } from "./types";
import { hydrateLastTestResults } from "./lib/lastTestResults";

/** Stable empty draft so ApplyPage doesn't treat every parent render as a change. */
const EMPTY_DRAFT = emptyBindings();

function pagePaneClass(active: boolean): string {
  // Keep mounted + preserve each page's own scroll position.
  return active ? "h-full overflow-auto" : "hidden";
}

export default function App() {
  const [page, setPage] = useState<PageId>("providers");
  const [visited, setVisited] = useState<ReadonlySet<PageId>>(
    () => new Set<PageId>(["providers"]),
  );
  const [state, setState] = useState<FullState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [draftBindings, setDraftBindings] = useState<AgentBindings | null>(null);

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast(null), 4000);
  }, []);

  const refresh = useCallback(async () => {
    const s = await api.getState();
    hydrateLastTestResults(s.store.modelTestResults ?? {});
    setState(s);
    setError(null);
  }, []);

  const navigate = useCallback((next: PageId) => {
    setPage(next);
    setVisited((prev) => {
      if (prev.has(next)) return prev;
      const copy = new Set(prev);
      copy.add(next);
      return copy;
    });
  }, []);

  useEffect(() => {
    void refresh().catch((e: unknown) => {
      setError(e instanceof Error ? e.message : String(e));
    });
  }, [refresh]);

  if (error) {
    return (
      <div className="flex h-full items-center justify-center bg-surface-0 p-8 text-ink-1">
        <div className="card max-w-lg p-6">
          <h1 className="text-lg font-semibold text-danger">无法加载 ModelHub</h1>
          <p className="mt-2 text-sm text-ink-2">{error}</p>
          <button type="button" className="btn-primary mt-4" onClick={() => void refresh()}>
            重试
          </button>
        </div>
      </div>
    );
  }

  if (!state) {
    return (
      <div className="flex h-full items-center justify-center bg-surface-0 text-ink-2">
        加载中…
      </div>
    );
  }

  return (
    <Layout
      page={page}
      onNavigate={navigate}
      onApply={() => navigate("apply")}
      toast={toast}
    >
      {/* Mount once on first visit, then keep alive across tab switches. */}
      {visited.has("providers") ? (
        <div className={pagePaneClass(page === "providers")}>
          <ProvidersPage state={state} onRefresh={refresh} onToast={showToast} />
        </div>
      ) : null}
      {visited.has("agents") ? (
        <div className={pagePaneClass(page === "agents")}>
          <AgentsPage
            state={state}
            draft={draftBindings}
            onDraftChange={setDraftBindings}
            onToast={showToast}
          />
        </div>
      ) : null}
      {visited.has("apply") ? (
        <div className={pagePaneClass(page === "apply")}>
          <ApplyPage
            state={state}
            draft={draftBindings ?? EMPTY_DRAFT}
            onToast={showToast}
          />
        </div>
      ) : null}
      {visited.has("import") ? (
        <div className={pagePaneClass(page === "import")}>
          <ImportPage state={state} onRefresh={refresh} onToast={showToast} />
        </div>
      ) : null}
      {visited.has("backups") ? (
        <div className={pagePaneClass(page === "backups")}>
          <BackupsPage onToast={showToast} />
        </div>
      ) : null}
      {visited.has("settings") ? (
        <div className={pagePaneClass(page === "settings")}>
          <SettingsPage state={state} />
        </div>
      ) : null}
    </Layout>
  );
}
