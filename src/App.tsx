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

export default function App() {
  const [page, setPage] = useState<PageId>("providers");
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
    setState(s);
    setError(null);
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
      onNavigate={setPage}
      onApply={() => setPage("apply")}
      toast={toast}
    >
      {page === "providers" ? (
        <ProvidersPage state={state} onRefresh={refresh} onToast={showToast} />
      ) : null}
      {page === "agents" ? (
        <AgentsPage
          state={state}
          draft={draftBindings}
          onDraftChange={setDraftBindings}
          onToast={showToast}
        />
      ) : null}
      {page === "apply" ? (
        <ApplyPage
          state={state}
          draft={draftBindings ?? emptyBindings()}
          onToast={showToast}
        />
      ) : null}
      {page === "import" ? (
        <ImportPage state={state} onRefresh={refresh} onToast={showToast} />
      ) : null}
      {page === "backups" ? <BackupsPage onToast={showToast} /> : null}
      {page === "settings" ? <SettingsPage state={state} /> : null}
    </Layout>
  );
}
