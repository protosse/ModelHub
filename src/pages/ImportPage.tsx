import { useEffect, useMemo, useRef, useState } from "react";
import type {
  FullState,
  ImportAction,
  ImportItemDecision,
  ImportPreview,
  ImportPreviewItem,
  Provider,
} from "../types";
import * as api from "../api/tauri";
import { ConfirmDialog } from "../components/Modal";
import { ImportRow, type ImportItem } from "../components/ImportRow";

type Props = {
  readonly state: FullState;
  /** When false (keep-alive hidden), skip auto-rescan; re-activate may rescan. */
  readonly active?: boolean;
  readonly onRefresh: () => Promise<void>;
  readonly onToast: (msg: string) => void;
  readonly onGoProviders?: () => void;
};

type StatusFilter = "all" | "new" | "exists" | "extra";
type SourceFilter = "all" | "opencode" | "pi" | "claude" | "codex";

/** Build an ImportItem from a fresh backend preview item, preserving user edits. */
function mergeItem(
  fresh: ImportPreviewItem,
  prev: ImportItem | undefined,
  mode: "defaults" | "keep" | "clear",
): ImportItem {
  if (prev && (mode === "keep" || mode === "clear")) {
    return {
      ...fresh,
      name: prev.name.trim() ? prev.name : fresh.name,
      selected: mode === "keep" ? prev.selected : false,
      error: null,
    };
  }
  return {
    ...fresh,
    selected: !fresh.alreadyExists && fresh.hasApiKey,
    error: null,
  };
}

function effectiveAction(item: ImportItem): ImportAction {
  if (!item.selected) return "skip";
  return item.alreadyExists ? "override" : "import";
}

function suggestName(base: string, existing: ReadonlySet<string>, used: ReadonlySet<string>): string {
  const root = base.trim() || "provider";
  let n = 2;
  let candidate = `${root}-${n}`;
  while (existing.has(candidate.toLowerCase()) || used.has(candidate.toLowerCase())) {
    n += 1;
    candidate = `${root}-${n}`;
  }
  return candidate;
}

function rankItem(item: ImportPreviewItem): number {
  if (!item.alreadyExists && item.hasApiKey) return 0;
  if (!item.alreadyExists) return 1;
  if ((item.extraModelCount ?? 0) > 0) return 2;
  return 3;
}

/** Shared name-clash check for live row hint and submit validation (O2). */
function nameClashReason(
  item: Pick<ImportItem, "id" | "name" | "selected" | "alreadyExists" | "existingProviderId">,
  items: readonly ImportItem[],
  providers: readonly Provider[],
  opts?: { readonly requireSelected?: boolean },
): string | null {
  const requireSelected = opts?.requireSelected ?? true;
  if (requireSelected && !item.selected) return null;
  const name = item.name.trim();
  if (!name) return "名称不能为空";
  const lower = name.toLowerCase();
  const batchDup = items.some(
    (it) =>
      it.id !== item.id &&
      it.selected &&
      it.name.trim().toLowerCase() === lower,
  );
  if (batchDup) return "与本批其它勾选项重名";
  if (item.alreadyExists) {
    const clash = providers.some(
      (p) => p.id !== item.existingProviderId && p.name.toLowerCase() === lower,
    );
    if (clash) return "名称已被其它提供商占用，请改名";
  } else {
    const clash = providers.some((p) => p.name.toLowerCase() === lower);
    if (clash) return "名称已存在，请改名";
  }
  return null;
}

export function ImportPage({
  state,
  active = true,
  onRefresh,
  onToast,
  onGoProviders,
}: Props) {
  const [items, setItems] = useState<ImportItem[]>([]);
  const [scanNotes, setScanNotes] = useState<string[]>([]);
  const [scanned, setScanned] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [importing, setImporting] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [sourceFilter, setSourceFilter] = useState<SourceFilter>("all");
  const [query, setQuery] = useState("");
  const [helpOpen, setHelpOpen] = useState(false);
  const [pathsOpen, setPathsOpen] = useState(false);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(() => new Set());
  // O4: keep id→name snapshot at import time so banner survives rescan drops.
  const [postImportNoKey, setPostImportNoKey] = useState<
    readonly { id: string; name: string }[]
  >([]);

  const busy = scanning || importing;
  const storeRev = useMemo(() => {
    // Cheap fingerprint of library identity so keep-alive can detect external edits.
    const p = state.store.providers
      .map((x) => `${x.id}:${x.updatedAt}`)
      .join("|");
    const m = state.store.models.map((x) => `${x.id}:${x.modelId}`).join("|");
    return `${p}#${m}`;
  }, [state.store.providers, state.store.models]);
  const storeRevRef = useRef(storeRev);
  storeRevRef.current = storeRev;
  const lastScannedRev = useRef<string | null>(null);
  const loadRef = useRef<(mode?: "defaults" | "keep" | "clear") => Promise<void>>(async () => {});

  /**
   * Rescan agent configs into the import table.
   * - `defaults`: first open / manual refresh — check new+hasKey only
   * - `keep`: after a failed import — keep exact previous checkboxes + renames
   * - `clear`: after a successful import — keep renames, uncheck everything
   */
  const load = async (mode: "defaults" | "keep" | "clear" = "defaults") => {
    setScanning(true);
    try {
      const p: ImportPreview = await api.previewImport();
      setScanNotes([...(p.scanNotes ?? [])]);
      setItems((prev) => {
        const prevById = new Map(prev.map((it) => [it.id, it]));
        return p.items.map((fresh) => mergeItem(fresh, prevById.get(fresh.id), mode));
      });
      setScanned(true);
      // Use ref so async load after onRefresh still stamps the latest library rev.
      lastScannedRev.current = storeRevRef.current;
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
    } finally {
      setScanning(false);
    }
  };
  loadRef.current = load;

  // Initial scan on mount.
  useEffect(() => {
    void loadRef.current("defaults");
  }, []);

  // B4: when returning to Import after library changed, quiet rescan keeping selections.
  useEffect(() => {
    if (!active) return;
    if (!scanned) return;
    if (importing || scanning) return;
    if (lastScannedRev.current === storeRev) return;
    void loadRef.current("keep");
  }, [active, storeRev, scanned, importing, scanning]);

  const summary = useMemo(() => {
    let neu = 0;
    let exists = 0;
    let noKey = 0;
    let extra = 0;
    for (const it of items) {
      if (it.alreadyExists) exists += 1;
      else neu += 1;
      if (!it.hasApiKey) noKey += 1;
      if ((it.extraModelCount ?? 0) > 0) extra += 1;
    }
    return { total: items.length, neu, exists, noKey, extra };
  }, [items]);

  const agentMissing =
    !state.paths.claudeExists &&
    !state.paths.codexExists &&
    !state.paths.opencodeExists &&
    !state.paths.piExists;

  const filteredRows = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = items.filter((it) => {
      if (statusFilter === "new" && it.alreadyExists) return false;
      if (statusFilter === "exists" && !it.alreadyExists) return false;
      if (statusFilter === "extra" && (it.extraModelCount ?? 0) === 0) return false;
      if (sourceFilter !== "all") {
        const sources = it.source.split("+").map((s) => s.trim().toLowerCase());
        if (!sources.includes(sourceFilter)) return false;
      }
      if (q) {
        const hay = `${it.name} ${it.baseUrl} ${it.source}`.toLowerCase();
        if (!hay.includes(q)) return false;
      }
      return true;
    });
    return [...list].sort((a, b) => rankItem(a) - rankItem(b) || a.name.localeCompare(b.name, "zh"));
  }, [items, statusFilter, sourceFilter, query]);

  const selectedCount = items.filter((it) => it.selected).length;
  const filteredSelectedCount = filteredRows.filter((it) => it.selected).length;
  const filterActive =
    statusFilter !== "all" || sourceFilter !== "all" || query.trim().length > 0;
  const selectedOutsideFilter = selectedCount - filteredSelectedCount;

  const selectedPlan = useMemo(() => {
    let toImport = 0;
    let toOverride = 0;
    let noKey = 0;
    let modelsToAdd = 0;
    const noKeyIds: string[] = [];
    for (const it of items) {
      if (!it.selected) continue;
      if (it.alreadyExists) {
        toOverride += 1;
        modelsToAdd += it.extraModelCount ?? 0;
      } else {
        toImport += 1;
        modelsToAdd += it.modelCount;
      }
      if (!it.hasApiKey) {
        noKey += 1;
        noKeyIds.push(it.id);
      }
    }
    return { toImport, toOverride, noKey, noKeyIds, modelsToAdd };
  }, [items]);

  // O4: prefer live rename from current items; fall back to snapshot name.
  const postImportNoKeyNames = useMemo(() => {
    if (postImportNoKey.length === 0) return [];
    const byId = new Map(items.map((it) => [it.id, it]));
    return postImportNoKey.map((snap) => byId.get(snap.id)?.name ?? snap.name);
  }, [postImportNoKey, items]);

  /**
   * After a user edit, recompute submit errors against the *new* selection/names.
   * - If no row currently has a submit error, only clear the touched rows (soft
   *   live hints still come from liveNameConflict).
   * - If any row has a submit error (post-validate), revalidate *all selected*
   *   rows so fixed rows clear, still-broken peers stay red, and batch-dup
   *   partners drop when one is unchecked/renamed.
   */
  const applyItemsEdit = (
    prev: ImportItem[],
    mapRow: (it: ImportItem) => ImportItem,
  ): ImportItem[] => {
    const draft = prev.map(mapRow);
    const hadSubmitErrors = prev.some((it) => it.error);
    if (!hadSubmitErrors) return draft;
    return draft.map((it) => {
      if (!it.selected) return it.error ? { ...it, error: null } : it;
      const reason = nameClashReason(it, draft, state.store.providers);
      return it.error === reason ? it : { ...it, error: reason };
    });
  };

  /** Batch-select visible (filtered) rows using a predicate. */
  const batchSelect = (predicate: (it: ImportItem) => boolean) => {
    const visible = new Set(filteredRows.map((it) => it.id));
    setItems((prev) =>
      applyItemsEdit(prev, (it) =>
        visible.has(it.id) ? { ...it, selected: predicate(it) } : it,
      ),
    );
  };

  /** Update a single item's editable fields. */
  const updateItem = (
    id: string,
    patch: Partial<Pick<ImportItem, "name" | "selected">>,
  ) => {
    setItems((prev) =>
      applyItemsEdit(prev, (it) =>
        it.id === id ? { ...it, ...patch, error: null } : it,
      ),
    );
  };

  const liveNameConflict = (item: ImportItem): boolean => {
    const reason = nameClashReason(item, items, state.store.providers);
    // Empty-name is shown via validate only; live hint is for clashes.
    return Boolean(reason && reason !== "名称不能为空");
  };

  const autoRename = (id: string) => {
    const item = items.find((it) => it.id === id);
    if (!item) return;
    const existing = new Set(state.store.providers.map((p) => p.name.toLowerCase()));
    const used = new Set(
      items
        .filter((it) => it.id !== id)
        .map((it) => it.name.trim().toLowerCase())
        .filter(Boolean),
    );
    const root = item.name.trim() || "provider";
    updateItem(id, { name: suggestName(root, existing, used) });
  };

  const toggleExpand = (id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const validateRows = (): { ok: boolean; firstErrorId: string | null } => {
    const selected = items.filter((it) => it.selected);
    if (selected.length === 0) {
      onToast("请至少勾选一项要导入或覆盖的提供商");
      return { ok: false, firstErrorId: null };
    }

    let ok = true;
    let firstErrorId: string | null = null;
    const next = items.map((it) => {
      if (!it.selected) return { ...it, error: null };
      const reason = nameClashReason(it, items, state.store.providers);
      if (reason) {
        ok = false;
        if (!firstErrorId) firstErrorId = it.id;
        return { ...it, error: reason };
      }
      return { ...it, error: null };
    });
    setItems(next);
    if (!ok) {
      setStatusFilter("all");
      setSourceFilter("all");
      setQuery("");
      onToast("请修正标红的行后再导入");
      window.setTimeout(() => {
        if (!firstErrorId) return;
        document
          .getElementById(`import-row-${firstErrorId}`)
          ?.scrollIntoView({ behavior: "smooth", block: "center" });
      }, 50);
      return { ok: false, firstErrorId };
    }
    return { ok: true, firstErrorId: null };
  };

  const runImport = async () => {
    setConfirmOpen(false);
    if (!validateRows().ok) return;
    const noKeySnaps = items
      .filter((it) => it.selected && !it.hasApiKey)
      .map((it) => ({ id: it.id, name: it.name.trim() || it.id }));
    setImporting(true);
    try {
      const payload: ImportItemDecision[] = items
        .filter((it) => it.selected)
        .map((it) => ({
          id: it.id,
          name: it.name.trim(),
          action: effectiveAction(it),
        }));
      if (payload.length === 0) {
        onToast("请至少勾选一项要导入或覆盖的提供商");
        return;
      }
      const res = await api.runImport(payload);
      await onRefresh();
      // Success: keep renames, uncheck all (do not re-default remaining importables).
      await load("clear");
      const parts = [
        `新增 ${res.importedProviders}`,
        `覆盖 ${res.overridden}`,
        `模型 +${res.importedModels}`,
      ];
      if (res.skipped) parts.push(`跳过 ${res.skipped}`);
      if (noKeySnaps.length > 0) {
        parts.push(`${noKeySnaps.length} 项无 Key 请补填`);
        setPostImportNoKey(noKeySnaps);
      } else {
        setPostImportNoKey([]);
      }
      onToast(`导入完成：${parts.join("，")}`);
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
      // Backend may have committed partial imports before failing; resync UI
      // while keeping the user's override checkboxes so they can retry.
      try {
        await onRefresh();
        await load("keep");
      } catch {
        /* ignore secondary refresh error */
      }
    } finally {
      setImporting(false);
    }
  };

  const requestImport = () => {
    if (!validateRows().ok) return;
    setConfirmOpen(true);
  };

  return (
    <div className="mx-auto max-w-5xl space-y-4">
      <div className="card p-4">
        <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
          <div>
            <h3 className="font-semibold">本机配置</h3>
            <div className="mt-2 flex flex-wrap gap-3 text-sm">
              <Det label="Claude" ok={state.paths.claudeExists} path={state.paths.claudeSettings} />
              <Det label="Codex" ok={state.paths.codexExists} path={state.paths.codexConfig} />
              <Det
                label="OpenCode"
                ok={state.paths.opencodeExists}
                path={state.paths.opencodeConfig}
              />
              <Det label="Pi" ok={state.paths.piExists} path={state.paths.piModels} />
            </div>
            <button
              type="button"
              className="mt-1 text-[11px] text-ink-3 underline hover:text-ink-2"
              onClick={() => setPathsOpen((v) => !v)}
            >
              {pathsOpen ? "收起路径" : "显示路径"}
            </button>
            {pathsOpen ? (
              <ul className="mt-2 space-y-1 font-mono text-[11px] text-ink-3">
                <li>Claude: {state.paths.claudeSettings}</li>
                <li>Codex: {state.paths.codexConfig}</li>
                <li>OpenCode: {state.paths.opencodeConfig}</li>
                <li>Pi: {state.paths.piModels}</li>
              </ul>
            ) : null}
          </div>
          <button
            type="button"
            className="btn-secondary shrink-0"
            disabled={busy}
            onClick={() => void load("defaults")}
          >
            {scanning ? "扫描中…" : "刷新扫描"}
          </button>
        </div>
        {scanning ? (
          <p className="mt-2 text-xs text-ink-3">扫描中…</p>
        ) : scanned ? (
          <div className="mt-2 flex flex-wrap items-center gap-1.5 text-xs">
            <span className="text-ink-3">共 {summary.total} 项</span>
            <SummaryChip
              active={statusFilter === "new"}
              label={`可导入 ${summary.neu}`}
              onClick={() => setStatusFilter((f) => (f === "new" ? "all" : "new"))}
            />
            {summary.extra > 0 ? (
              <SummaryChip
                active={statusFilter === "extra"}
                label={`可补 ${summary.extra}`}
                onClick={() => setStatusFilter((f) => (f === "extra" ? "all" : "extra"))}
              />
            ) : null}
            <SummaryChip
              active={statusFilter === "exists"}
              label={`已存在 ${summary.exists}`}
              onClick={() => setStatusFilter((f) => (f === "exists" ? "all" : "exists"))}
            />
            {summary.noKey > 0 ? (
              <span className="text-ink-3">无 Key {summary.noKey}</span>
            ) : null}
          </div>
        ) : (
          <p className="mt-2 text-xs text-ink-3">加载中…</p>
        )}
        {scanNotes.length > 0 ? (
          <ul className="mt-2 space-y-0.5 text-xs text-warn">
            {scanNotes.map((n) => (
              <li key={n}>⚠ {n}</li>
            ))}
          </ul>
        ) : null}
        <button
          type="button"
          className="mt-2 text-xs text-ink-3 underline hover:text-ink-2"
          onClick={() => setHelpOpen((v) => !v)}
        >
          {helpOpen ? "收起说明" : "说明"}
        </button>
        {helpOpen ? (
          <p className="mt-2 text-xs leading-relaxed text-ink-3">
            勾选=导入或覆盖，不勾选=跳过。可导入=新且有 Key；可补模型=库内已有但本机模型更多；
            覆盖会增量补模型。点击行空白可切换勾选。
          </p>
        ) : null}
      </div>

      {postImportNoKeyNames.length > 0 ? (
        <div className="rounded-lg border border-warn/40 bg-warn/10 px-4 py-3 text-sm">
          <div className="font-medium text-warn">以下导入项无 API Key，请补填：</div>
          <ul className="mt-1 list-inside list-disc text-xs text-ink-2">
            {postImportNoKeyNames.slice(0, 12).map((n) => (
              <li key={n}>{n}</li>
            ))}
            {postImportNoKeyNames.length > 12 ? (
              <li>…共 {postImportNoKeyNames.length} 个</li>
            ) : null}
          </ul>
          <div className="mt-2 flex flex-wrap gap-2">
            {onGoProviders ? (
              <button type="button" className="btn-primary !py-1 text-xs" onClick={onGoProviders}>
                查看提供商
              </button>
            ) : null}
            <button
              type="button"
              className="btn-secondary !py-1 text-xs"
              onClick={() => setPostImportNoKey([])}
            >
              关闭
            </button>
          </div>
        </div>
      ) : null}

      <div className="card p-4">
        <div className="mb-3 flex flex-wrap items-center gap-2">
          {(
            [
              ["all", "全部"],
              ["new", "可导入"],
              ["extra", "可补模型"],
              ["exists", "已存在"],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              className={
                statusFilter === id
                  ? "rounded-full bg-accent/20 px-2.5 py-1 text-xs text-accent"
                  : "rounded-full bg-surface-3 px-2.5 py-1 text-xs text-ink-2 hover:text-ink-1"
              }
              onClick={() => setStatusFilter(id)}
            >
              {label}
            </button>
          ))}
          <span className="mx-1 h-4 w-px bg-surface-3" />
          {(
            [
              ["all", "来源全部"],
              ["opencode", "OpenCode"],
              ["pi", "Pi"],
              ["claude", "Claude"],
              ["codex", "Codex"],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              className={
                sourceFilter === id
                  ? "rounded-full bg-accent/20 px-2.5 py-1 text-xs text-accent"
                  : "rounded-full bg-surface-3 px-2.5 py-1 text-xs text-ink-2 hover:text-ink-1"
              }
              onClick={() => setSourceFilter(id)}
            >
              {label}
            </button>
          ))}
          <input
            className="input ml-auto max-w-xs !py-1.5 text-sm"
            placeholder="搜索名称 / URL"
            value={query}
            disabled={importing}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>

        <div className="mb-3 flex flex-wrap items-center gap-2">
          <button
            type="button"
            className="btn-secondary"
            disabled={busy || filteredRows.length === 0}
            onClick={() => batchSelect((it) => !it.alreadyExists && it.hasApiKey)}
            title="勾选当前列表中「新且有 Key」的项"
          >
            勾选可导入
          </button>
          <button
            type="button"
            className="btn-secondary"
            disabled={busy || filteredRows.length === 0 || summary.extra === 0}
            onClick={() =>
              batchSelect((it) => it.alreadyExists && (it.extraModelCount ?? 0) > 0)
            }
            title="勾选当前列表中「本机有更多模型可补」的已存在项"
          >
            勾选可补模型
          </button>
          <button
            type="button"
            className="btn-secondary"
            disabled={busy || filteredRows.length === 0 || summary.exists === 0}
            onClick={() => batchSelect((it) => it.alreadyExists)}
            title="勾选当前列表中全部已存在项（覆盖）"
          >
            勾选已存在
          </button>
          <button
            type="button"
            className="btn-secondary"
            disabled={busy || filteredSelectedCount === 0}
            onClick={() => batchSelect(() => false)}
          >
            清空当前
          </button>
          <span className="text-xs text-ink-3">
            已选 {selectedCount}
            {filterActive
              ? ` · 当前列表 ${filteredRows.length} 项 · 其中已选 ${filteredSelectedCount}`
              : null}
            {filterActive && selectedOutsideFilter > 0 ? (
              <span className="text-warn"> · 含筛选外 {selectedOutsideFilter} 项</span>
            ) : null}
          </span>
          <button
            type="button"
            className="btn-primary min-w-[9.5rem] ml-auto"
            disabled={busy || selectedCount === 0}
            onClick={requestImport}
          >
            {importing
              ? "导入中…"
              : selectedCount > 0
                ? `导入所选 (${selectedCount})`
                : "导入所选"}
          </button>
        </div>

        {items.length === 0 ? (
          <p className="text-sm text-ink-3">
            {scanning
              ? "扫描中…"
              : agentMissing
                ? "未检测到本机 Claude / Codex / OpenCode / Pi 配置文件。安装并配置对应 Agent 后点击「刷新扫描」。"
                : "已检测到 Agent 配置，但没有可导入的提供商（可能均为内置官方或无可识别 baseUrl）。可点「刷新扫描」重试。"}
          </p>
        ) : filteredRows.length === 0 ? (
          <p className="text-sm text-ink-3">当前筛选下无项目</p>
        ) : (
          <ul className="divide-y divide-surface-3 rounded-lg border border-surface-3">
            {filteredRows.map((item) => (
              <ImportRow
                key={item.id}
                item={item}
                action={effectiveAction(item)}
                expanded={expandedIds.has(item.id)}
                importing={importing}
                dimmed={!item.hasApiKey}
                nameConflict={liveNameConflict(item)}
                onToggleSelect={(selected) => updateItem(item.id, { selected })}
                onNameChange={(name) => updateItem(item.id, { name })}
                onToggleExpand={() => toggleExpand(item.id)}
                onAutoRename={() => autoRename(item.id)}
              />
            ))}
          </ul>
        )}
      </div>

      {confirmOpen ? (
        <ConfirmDialog
          title="确认导入"
          message={[
            selectedPlan.toImport ? `新建 ${selectedPlan.toImport} 个提供商` : null,
            selectedPlan.toOverride
              ? `覆盖 ${selectedPlan.toOverride} 个（有 Key 才更新密钥；模型增量）`
              : null,
            selectedPlan.modelsToAdd > 0
              ? `预计模型变动：约 +${selectedPlan.modelsToAdd}（新建全量 + 覆盖可补）`
              : selectedPlan.toOverride > 0
                ? "覆盖项模型无增量，仅更新连接信息"
                : null,
            selectedPlan.noKey
              ? `其中 ${selectedPlan.noKey} 个无 Key（导入后请补填）`
              : null,
            "",
            "未勾选的项将跳过。",
          ]
            .filter((x) => x !== null)
            .join("\n")}
          confirmLabel="确认导入"
          busy={importing}
          onCancel={() => setConfirmOpen(false)}
          onConfirm={() => void runImport()}
        />
      ) : null}
    </div>
  );
}

function SummaryChip({
  active,
  label,
  onClick,
}: {
  readonly active: boolean;
  readonly label: string;
  readonly onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={
        active
          ? "rounded-full bg-accent/20 px-2 py-0.5 text-accent"
          : "rounded-full bg-surface-3 px-2 py-0.5 text-ink-2 hover:text-ink-1"
      }
      onClick={onClick}
    >
      {label}
    </button>
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
    <span className="inline-flex items-center gap-1.5" title={path}>
      <span className={ok ? "text-ok" : "text-ink-3"}>{ok ? "●" : "○"}</span>
      <span className={ok ? "text-ink-1" : "text-ink-3"}>{label}</span>
    </span>
  );
}
