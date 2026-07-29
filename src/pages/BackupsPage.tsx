import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type { FullState } from "../types";
import * as api from "../api/tauri";
import { ConfirmDialog } from "../components/Modal";
import {
  BACKUP_AGENT_FILTERS,
  type BackupAgentFilter,
  type BackupSnapshot,
  agentLabel,
  formatBackupStamp,
  formatRelativeStamp,
  groupBackupSnapshots,
  latestStampByAgent,
} from "../lib/backups";

type Props = {
  readonly state: FullState;
  readonly active?: boolean;
  readonly onToast: (msg: string) => void;
};

async function copyText(text: string): Promise<void> {
  await navigator.clipboard.writeText(text);
}

export function BackupsPage({ state, active = true, onToast }: Props) {
  const [snapshots, setSnapshots] = useState<readonly BackupSnapshot[]>([]);
  const [busy, setBusy] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [agentFilter, setAgentFilter] = useState<BackupAgentFilter>("all");
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());
  const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(() => new Set());
  const [pendingRestore, setPendingRestore] = useState<BackupSnapshot | null>(null);
  const [pendingDelete, setPendingDelete] = useState<readonly BackupSnapshot[] | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const onToastRef = useRef(onToast);
  onToastRef.current = onToast;
  const loadSeq = useRef(0);

  const load = useCallback(async () => {
    const seq = ++loadSeq.current;
    setBusy(true);
    try {
      const entries = await api.listBackups();
      if (seq !== loadSeq.current) return;
      const next = groupBackupSnapshots(entries);
      const available = new Set(next.map((snapshot) => snapshot.id));
      setSnapshots(next);
      setSelectedIds((prev) => new Set([...prev].filter((id) => available.has(id))));
      setLoaded(true);
    } catch (e) {
      if (seq !== loadSeq.current) return;
      onToastRef.current(e instanceof Error ? e.message : String(e));
    } finally {
      if (seq === loadSeq.current) setBusy(false);
    }
  }, []);

  // First mount + each return to this keep-alive tab.
  useEffect(() => {
    if (!active) return;
    void load();
  }, [active, load]);

  const filtered = useMemo(() => {
    if (agentFilter === "all") return snapshots;
    return snapshots.filter((s) => s.agent === agentFilter);
  }, [snapshots, agentFilter]);
  const selectedSnapshots = useMemo(
    () => snapshots.filter((snapshot) => selectedIds.has(snapshot.id)),
    [snapshots, selectedIds],
  );
  const filteredIds = useMemo(() => filtered.map((snapshot) => snapshot.id), [filtered]);
  const allFilteredSelected =
    filteredIds.length > 0 && filteredIds.every((id) => selectedIds.has(id));

  const latestByAgent = useMemo(() => latestStampByAgent(snapshots), [snapshots]);
  const fileCount = useMemo(
    () => snapshots.reduce((n, s) => n + s.files.length, 0),
    [snapshots],
  );

  const toggleExpand = (id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleSelected = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleAllFiltered = () => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (allFilteredSelected) {
        for (const id of filteredIds) next.delete(id);
      } else {
        for (const id of filteredIds) next.add(id);
      }
      return next;
    });
  };

  const onCopy = async (text: string, okMsg: string) => {
    try {
      await copyText(text);
      onToast(okMsg);
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
    }
  };

  const onReveal = async (path: string) => {
    try {
      await revealItemInDir(path);
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
    }
  };

  const confirmRestore = async () => {
    if (!pendingRestore || restoring) return;
    setRestoring(true);
    try {
      const res = await api.restoreBackup(pendingRestore.agent, pendingRestore.stamp);
      onToast(res.message);
      setPendingRestore(null);
      await load();
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
    } finally {
      setRestoring(false);
    }
  };

  const confirmDelete = async () => {
    if (!pendingDelete?.length || deleting) return;
    setDeleting(true);
    try {
      const removed = await api.deleteBackups(
        pendingDelete.map((snapshot) => ({
          agent: snapshot.agent,
          stamp: snapshot.stamp,
        })),
      );
      const deletedIds = new Set(pendingDelete.map((snapshot) => snapshot.id));
      setExpanded((prev) => {
        const next = new Set(prev);
        for (const id of deletedIds) next.delete(id);
        return next;
      });
      setSelectedIds((prev) => new Set([...prev].filter((id) => !deletedIds.has(id))));
      setPendingDelete(null);
      await load();
      onToast(`已删除 ${removed} 组备份快照`);
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
    } finally {
      setDeleting(false);
    }
  };

  const keep = state.config.backupKeepCount;
  const root = `${state.paths.modelhubDir}/backups`;
  const pageBusy = busy || restoring || deleting;

  return (
    <div className="mx-auto flex h-full max-w-4xl flex-col gap-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 space-y-1">
          <p className="text-sm text-ink-2">
            Apply 前自动备份各 Agent 配置。每个 Agent 保留最近{" "}
            <span className="font-medium text-ink-1">{keep}</span> 组快照。
          </p>
          <p className="text-xs text-ink-3">
            一键恢复会先备份当前配置，再写回该快照；建议先退出对应 Agent。目录：
            <span className="ml-1 font-mono">{root}</span>
          </p>
        </div>
        <button
          type="button"
          className="btn-secondary shrink-0"
          disabled={pageBusy}
          onClick={() => void load()}
        >
          {busy ? "刷新中…" : "刷新"}
        </button>
      </div>

      <div className="grid grid-cols-3 gap-3">
        <StatCard label="快照组" value={String(snapshots.length)} />
        <StatCard label="备份文件" value={String(fileCount)} />
        <StatCard label="保留策略" value={`每 Agent ${keep}`} />
      </div>

      {snapshots.length > 0 ? (
        <div className="card flex flex-wrap gap-x-4 gap-y-2 p-3 text-xs text-ink-3">
          {BACKUP_AGENT_FILTERS.filter((f) => f.id !== "all").map((f) => {
            const stamp = latestByAgent[f.id];
            return (
              <div key={f.id} className="min-w-[9rem]">
                <div className="font-medium text-ink-2">{f.label}</div>
                <div className="font-mono">
                  {stamp ? formatBackupStamp(stamp) : "尚无备份"}
                </div>
              </div>
            );
          })}
        </div>
      ) : null}

      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-2">
          {BACKUP_AGENT_FILTERS.map((f) => {
            const count =
              f.id === "all"
                ? snapshots.length
                : snapshots.filter((s) => s.agent === f.id).length;
            const on = agentFilter === f.id;
            return (
              <button
                key={f.id}
                type="button"
                className={
                  on
                    ? "rounded-full bg-accent/15 px-3 py-1 text-xs font-medium text-accent"
                    : "rounded-full bg-surface-2 px-3 py-1 text-xs text-ink-2 hover:bg-surface-3"
                }
                onClick={() => setAgentFilter(f.id)}
              >
                {f.label}
                <span className="ml-1 text-ink-3">{count}</span>
              </button>
            );
          })}
        </div>
        <div className="flex items-center gap-2 text-xs">
          <label className="flex items-center gap-1.5 text-ink-2">
            <input
              type="checkbox"
              checked={allFilteredSelected}
              disabled={pageBusy || filteredIds.length === 0}
              onChange={toggleAllFiltered}
            />
            全选当前
          </label>
          <span className="text-ink-3">已选 {selectedIds.size}</span>
          <button
            type="button"
            className="btn-danger !px-2 !py-1 text-xs"
            disabled={pageBusy || selectedSnapshots.length === 0}
            onClick={() => setPendingDelete(selectedSnapshots)}
          >
            删除所选
          </button>
        </div>
      </div>

      <div className="card flex min-h-0 flex-1 flex-col overflow-hidden">
        {!loaded && busy ? (
          <div className="p-8 text-center text-sm text-ink-3">正在读取备份…</div>
        ) : snapshots.length === 0 ? (
          <div className="space-y-2 p-8 text-center text-sm text-ink-3">
            <div>暂无备份</div>
            <div className="text-xs">
              在「Agent 应用」写出配置后，会自动在此生成快照。
            </div>
          </div>
        ) : filtered.length === 0 ? (
          <div className="p-8 text-center text-sm text-ink-3">
            当前 Agent 筛选下没有备份
          </div>
        ) : (
          <ul className="min-h-0 flex-1 divide-y divide-surface-3 overflow-y-auto">
            {filtered.map((snap) => {
              const open = expanded.has(snap.id);
              const rel = formatRelativeStamp(snap.stamp);
              return (
                <li key={snap.id} className="px-4 py-3">
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <label className="flex shrink-0 items-center pt-1">
                      <input
                        type="checkbox"
                        checked={selectedIds.has(snap.id)}
                        disabled={pageBusy}
                        onChange={() => toggleSelected(snap.id)}
                        aria-label={`选择 ${agentLabel(snap.agent)} ${formatBackupStamp(snap.stamp)} 备份`}
                      />
                    </label>
                    <button
                      type="button"
                      className="min-w-0 flex-1 text-left"
                      onClick={() => toggleExpand(snap.id)}
                    >
                      <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
                        <span className="text-sm font-medium text-ink-1">
                          {formatBackupStamp(snap.stamp)}
                        </span>
                        {rel ? (
                          <span className="text-[11px] text-ink-3">{rel}</span>
                        ) : null}
                      </div>
                      <div className="mt-0.5 flex flex-wrap items-center gap-2 text-xs text-ink-3">
                        <span className="badge bg-surface-2 text-ink-2">
                          {agentLabel(snap.agent)}
                        </span>
                        <span>
                          {snap.files.length} 个文件
                          {!open
                            ? ` · ${snap.files.map((f) => f.fileName).join("、")}`
                            : ""}
                        </span>
                      </div>
                    </button>
                    <div className="flex shrink-0 flex-wrap gap-1.5">
                      <button
                        type="button"
                        className="btn-primary !px-2 !py-1 text-xs"
                        disabled={pageBusy}
                        onClick={() => setPendingRestore(snap)}
                      >
                        恢复
                      </button>
                      <button
                        type="button"
                        className="btn-ghost !px-2 !py-1 text-xs"
                        onClick={() => void onReveal(snap.files[0]?.path ?? snap.dirPath)}
                      >
                        打开位置
                      </button>
                      <button
                        type="button"
                        className="btn-secondary !px-2 !py-1 text-xs"
                        onClick={() => toggleExpand(snap.id)}
                      >
                        {open ? "收起" : "详情"}
                      </button>
                      <button
                        type="button"
                        className="btn-danger !px-2 !py-1 text-xs"
                        disabled={pageBusy}
                        onClick={() => setPendingDelete([snap])}
                      >
                        删除
                      </button>
                    </div>
                  </div>

                  {open ? (
                    <div className="mt-3 space-y-2 rounded-lg border border-surface-3 bg-surface-0/60 p-3">
                      <div className="font-mono text-[11px] text-ink-3 break-all">
                        {snap.dirPath}
                      </div>
                      <ul className="space-y-2">
                        {snap.files.map((f) => (
                          <li
                            key={f.path}
                            className="flex flex-wrap items-center justify-between gap-2 rounded-md bg-surface-1 px-3 py-2"
                          >
                            <div className="min-w-0">
                              <div className="font-mono text-sm text-ink-1">
                                {f.fileName}
                              </div>
                              <div className="mt-0.5 truncate font-mono text-[11px] text-ink-3">
                                {f.path}
                              </div>
                            </div>
                            <div className="flex shrink-0 gap-1">
                              <button
                                type="button"
                                className="btn-ghost !px-2 !py-1 text-xs"
                                onClick={() => void onCopy(f.path, "已复制文件路径")}
                              >
                                复制路径
                              </button>
                              <button
                                type="button"
                                className="btn-ghost !px-2 !py-1 text-xs"
                                onClick={() => void onReveal(f.path)}
                              >
                                打开位置
                              </button>
                            </div>
                          </li>
                        ))}
                      </ul>
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </div>

      {pendingRestore ? (
        <ConfirmDialog
          title={`恢复 ${agentLabel(pendingRestore.agent)} 快照？`}
          message={[
            `时间：${formatBackupStamp(pendingRestore.stamp)}`,
            `文件：${pendingRestore.files.map((f) => f.fileName).join("、")}`,
            "",
            "将覆盖该 Agent 当前配置路径上的对应文件。",
            "恢复前会自动再备份一份当前文件，便于回退。",
            "建议先退出对应 Agent，恢复后再启动。",
            pendingRestore.agent === "claude" || pendingRestore.agent === "codex"
              ? "Claude / Codex 恢复后通常需要重启才能生效。"
              : "",
          ]
            .filter(Boolean)
            .join("\n")}
          confirmLabel={restoring ? "恢复中…" : "确认恢复"}
          danger
          busy={restoring}
          onCancel={() => {
            if (!restoring) setPendingRestore(null);
          }}
          onConfirm={() => void confirmRestore()}
        />
      ) : null}

      {pendingDelete ? (
        <ConfirmDialog
          title={pendingDelete.length === 1 ? "删除这组备份？" : `删除 ${pendingDelete.length} 组备份？`}
          message={[
            pendingDelete.length === 1
              ? `${agentLabel(pendingDelete[0]!.agent)} · ${formatBackupStamp(pendingDelete[0]!.stamp)}`
              : `包含 ${new Set(pendingDelete.map((snapshot) => snapshot.agent)).size} 个 Agent，共 ${pendingDelete.reduce((count, snapshot) => count + snapshot.files.length, 0)} 个文件`,
            "",
            "将永久删除这一整组备份快照，且无法撤销。",
            "不会修改该 Agent 当前正在使用的配置文件。",
          ].join("\n")}
          confirmLabel={deleting ? "删除中…" : "确认删除"}
          danger
          busy={deleting}
          onCancel={() => {
            if (!deleting) setPendingDelete(null);
          }}
          onConfirm={() => void confirmDelete()}
        />
      ) : null}
    </div>
  );
}

function StatCard({ label, value }: { readonly label: string; readonly value: string }) {
  return (
    <div className="card px-3 py-2.5">
      <div className="text-[11px] text-ink-3">{label}</div>
      <div className="mt-0.5 text-sm font-semibold text-ink-1">{value}</div>
    </div>
  );
}
