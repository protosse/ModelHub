import { useEffect, useState } from "react";
import type { BackupEntry } from "../types";
import * as api from "../api/tauri";

type Props = {
  readonly onToast: (msg: string) => void;
};

export function BackupsPage({ onToast }: Props) {
  const [items, setItems] = useState<readonly BackupEntry[]>([]);
  const [busy, setBusy] = useState(false);

  const load = async () => {
    setBusy(true);
    try {
      setItems(await api.listBackups());
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  return (
    <div className="mx-auto max-w-3xl space-y-4">
      <div className="flex items-center justify-between">
        <p className="text-sm text-ink-2">Apply 前自动备份。恢复请手动复制路径或后续版本支持一键恢复。</p>
        <button type="button" className="btn-secondary" disabled={busy} onClick={() => void load()}>
          刷新
        </button>
      </div>
      <div className="card overflow-hidden">
        {items.length === 0 ? (
          <div className="p-8 text-center text-sm text-ink-3">暂无备份</div>
        ) : (
          <table className="w-full text-left text-sm">
            <thead className="bg-surface-2 text-xs text-ink-3">
              <tr>
                <th className="px-4 py-2 font-medium">时间</th>
                <th className="px-4 py-2 font-medium">Agent</th>
                <th className="px-4 py-2 font-medium">文件</th>
                <th className="px-4 py-2 font-medium">路径</th>
              </tr>
            </thead>
            <tbody>
              {items.map((b) => (
                <tr key={b.path} className="border-t border-surface-3">
                  <td className="px-4 py-2 font-mono text-xs">{b.stamp}</td>
                  <td className="px-4 py-2">{b.agent}</td>
                  <td className="px-4 py-2">{b.fileName}</td>
                  <td className="max-w-[280px] truncate px-4 py-2 font-mono text-[11px] text-ink-3">
                    {b.path}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
