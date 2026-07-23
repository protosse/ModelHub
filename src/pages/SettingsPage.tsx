import type { FullState } from "../types";

type Props = {
  readonly state: FullState;
};

export function SettingsPage({ state }: Props) {
  const c = state.config;
  const p = state.paths;
  return (
    <div className="mx-auto max-w-2xl space-y-4">
      <div className="card space-y-3 p-4 text-sm">
        <h3 className="font-semibold">应用</h3>
        <Row k="语言" v={c.language} />
        <Row k="备份保留份数" v={String(c.backupKeepCount)} />
        <Row k="数据目录" v={p.modelhubDir} mono />
      </div>
      <div className="card space-y-3 p-4 text-sm">
        <h3 className="font-semibold">Agent 路径（检测结果）</h3>
        <Row k="Claude" v={p.claudeSettings} mono />
        <Row k="Codex" v={p.codexConfig} mono />
        <Row k="OpenCode" v={p.opencodeConfig} mono />
        <Row k="Pi models" v={p.piModels} mono />
      </div>
      <div className="card p-4 text-xs text-ink-3">
        v0.1.0 · 路径覆盖与设置编辑将在后续版本完善。详见 docs/REQUIREMENTS.md。
      </div>
    </div>
  );
}

function Row({
  k,
  v,
  mono,
}: {
  readonly k: string;
  readonly v: string;
  readonly mono?: boolean;
}) {
  return (
    <div className="grid grid-cols-3 gap-2">
      <div className="text-ink-3">{k}</div>
      <div className={`col-span-2 break-all ${mono ? "font-mono text-xs" : ""}`}>{v}</div>
    </div>
  );
}
