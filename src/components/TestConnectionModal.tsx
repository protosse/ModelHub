import { useEffect, useMemo, useRef, useState } from "react";
import type { Model, Provider, TestConnectionResult, TestPrompt } from "../types";
import * as api from "../api/tauri";
import {
  clearSingleTestLogs,
  ensureSingleTestSession,
  getSingleTestSession,
  patchSingleTestSession,
  runSingleTest,
  subscribeSingleTestSession,
} from "../lib/singleTestSession";
import { Modal } from "./Modal";

type Props = {
  readonly provider: Provider;
  readonly model: Model;
  readonly prompts: readonly TestPrompt[];
  readonly onClose: () => void;
  readonly onPromptsChanged: () => Promise<void>;
  readonly onToast: (msg: string) => void;
};

const FALLBACK_PROMPT = "请只回复一个单词：ok";

export function TestConnectionModal({
  provider,
  model,
  prompts,
  onClose,
  onPromptsChanged,
  onToast,
}: Props) {
  const seeded = useMemo(() => {
    return prompts.find((p) => p.isDefault) ?? prompts[0] ?? null;
  }, [prompts]);

  // Keep module session in sync for this model (resumes busy/result after reopen).
  ensureSingleTestSession({
    modelId: model.id,
    providerId: provider.id,
    modelApiId: model.modelId,
    providerName: provider.name,
    protocol: provider.protocol,
    defaultPrompt: seeded?.content ?? FALLBACK_PROMPT,
    defaultPromptId: seeded?.id ?? "",
  });

  const [, setTick] = useState(0);
  useEffect(() => {
    return subscribeSingleTestSession(() => setTick((n) => n + 1));
  }, []);

  const session = getSingleTestSession();
  const forThisModel = session?.modelId === model.id ? session : null;

  const busy = forThisModel?.busy ?? false;
  const prompt = forThisModel?.prompt ?? seeded?.content ?? FALLBACK_PROMPT;
  const timeoutSecs = forThisModel?.timeoutSecs ?? 30;
  const selectedPromptId = forThisModel?.selectedPromptId ?? seeded?.id ?? "";
  const saveName = forThisModel?.saveName ?? "";
  const result = forThisModel?.result ?? null;
  const liveLines = forThisModel?.liveLines ?? [];
  const showLog = forThisModel?.showLog ?? false;
  const logTab = forThisModel?.logTab ?? "timeline";

  const [saveBusy, setSaveBusy] = useState(false);
  const preferPromptId = useRef<string | null>(null);
  const logBoxRef = useRef<HTMLPreElement | null>(null);
  const stickToBottom = useRef(true);

  // Init default prompt once for fresh session
  useEffect(() => {
    const s = getSingleTestSession();
    if (!s || s.modelId !== model.id || s.busy) return;
    if (preferPromptId.current) {
      const preferred = prompts.find((p) => p.id === preferPromptId.current);
      if (preferred) {
        patchSingleTestSession({
          selectedPromptId: preferred.id,
          prompt: preferred.content,
          saveName: preferred.isDefault ? "" : preferred.name,
        });
        return;
      }
    }
    // only set default if session still has empty selection seed
    if (!s.selectedPromptId && seeded) {
      patchSingleTestSession({
        selectedPromptId: seeded.id,
        prompt: seeded.content,
        saveName: seeded.isDefault ? "" : seeded.name,
      });
      preferPromptId.current = seeded.id;
    }
  }, [prompts, model.id, seeded]);

  useEffect(() => {
    const el = logBoxRef.current;
    if (!el || !stickToBottom.current) return;
    el.scrollTop = el.scrollHeight;
  }, [liveLines, logTab, result, showLog]);

  const selectedPrompt = useMemo(
    () => prompts.find((x) => x.id === selectedPromptId) ?? null,
    [prompts, selectedPromptId],
  );

  const applyPrompt = (id: string) => {
    preferPromptId.current = id;
    const p = prompts.find((x) => x.id === id);
    patchSingleTestSession({
      selectedPromptId: id,
      prompt: p?.content ?? prompt,
      saveName: p && !p.isDefault ? p.name : "",
    });
  };

  const runTest = async () => {
    if (busy) return;
    const text = prompt.trim();
    if (!text) {
      onToast("提示词不能为空");
      return;
    }
    stickToBottom.current = true;
    try {
      await runSingleTest(text, timeoutSecs);
    } catch (e) {
      onToast(e instanceof Error ? e.message : String(e));
    }
  };

  const savePrompt = async () => {
    if (saveBusy || busy) return;
    const name = saveName.trim();
    const content = prompt.trim();
    if (!name) {
      onToast("请填写提示词名称");
      return;
    }
    if (!content) {
      onToast("提示词内容不能为空");
      return;
    }
    setSaveBusy(true);
    try {
      const existing = prompts.find(
        (p) => !p.isDefault && p.name.toLowerCase() === name.toLowerCase(),
      );
      const saved = await api.upsertTestPrompt({
        id: existing?.id ?? null,
        name,
        content,
      });
      preferPromptId.current = saved.id;
      patchSingleTestSession({
        selectedPromptId: saved.id,
        prompt: saved.content,
        saveName: saved.name,
      });
      await onPromptsChanged();
      onToast(existing ? "提示词已更新" : "提示词已保存");
    } catch (e) {
      onToast(`保存失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSaveBusy(false);
    }
  };

  const setDefaultSelected = async () => {
    const p = selectedPrompt;
    if (!p) return;
    if (p.isDefault) {
      onToast("已是默认提示词");
      return;
    }
    try {
      const saved = await api.setDefaultTestPrompt(p.id);
      preferPromptId.current = saved.id;
      patchSingleTestSession({ selectedPromptId: saved.id });
      await onPromptsChanged();
      onToast(`已将「${saved.name}」设为默认`);
    } catch (e) {
      onToast(`设置默认失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const deleteSelected = async () => {
    const p = selectedPrompt;
    if (!p) return;
    if (p.isDefault) {
      onToast("默认提示词不可删除，请先将其他提示词设为默认");
      return;
    }
    try {
      await api.deleteTestPrompt(p.id);
      preferPromptId.current = null;
      patchSingleTestSession({ selectedPromptId: "", saveName: "" });
      await onPromptsChanged();
      onToast("已删除提示词");
    } catch (e) {
      onToast(`删除失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const copyLogs = async () => {
    const text = result
      ? formatFullLog(result, liveLines)
      : liveLines.join("\n") || "（暂无日志）";
    try {
      await navigator.clipboard.writeText(text);
      onToast("已复制日志");
    } catch {
      onToast("复制失败");
    }
  };

  const timelineText =
    liveLines.length > 0
      ? liveLines.join("\n")
      : busy
        ? "等待日志…"
        : "点击「发送测试」后将实时输出请求日志。关闭弹窗后测试仍继续。";

  const onLogScroll = () => {
    const el = logBoxRef.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    stickToBottom.current = distance < 40;
  };

  return (
    <Modal onClose={onClose} xwide>
      <h3 className="mb-1 text-base font-semibold">测试连接</h3>
      <p className="mb-4 text-xs text-ink-3">
        会向该 Provider 发起一次真实 API 调用（max_tokens 较小），可能产生少量用量。仅在点击「发送测试」时请求；关闭弹窗不会中断进行中的测试。
      </p>

      <dl className="mb-4 grid grid-cols-3 gap-2 text-sm">
        <div className="text-ink-3">提供商</div>
        <div className="col-span-2">{provider.name}</div>
        <div className="text-ink-3">协议</div>
        <div className="col-span-2 font-mono text-xs">{provider.protocol}</div>
        <div className="text-ink-3">Model ID</div>
        <div className="col-span-2 font-mono text-xs">{model.modelId}</div>
      </dl>

      <label className="mb-1 block text-xs text-ink-3">已保存提示词</label>
      <div className="mb-3 flex flex-wrap gap-2">
        <select
          className="input min-w-[12rem] flex-1"
          value={selectedPromptId}
          disabled={busy}
          onChange={(e) => applyPrompt(e.target.value)}
        >
          {prompts.length === 0 ? <option value="">（无）</option> : null}
          {prompts.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
              {p.isDefault ? "（默认）" : ""}
            </option>
          ))}
        </select>
        <button
          type="button"
          className="btn-secondary"
          disabled={busy || !selectedPrompt || selectedPrompt.isDefault}
          title={
            selectedPrompt?.isDefault
              ? "已是默认"
              : "将当前选中的提示词设为默认（打开测试时优先使用）"
          }
          onClick={() => void setDefaultSelected()}
        >
          设为默认
        </button>
        <button
          type="button"
          className="btn-secondary"
          disabled={busy || !selectedPrompt || selectedPrompt.isDefault}
          title={
            selectedPrompt?.isDefault
              ? "默认提示词不可删除，请先设其他为默认"
              : "删除当前非默认提示词"
          }
          onClick={() => void deleteSelected()}
        >
          删除
        </button>
      </div>

      <label className="mb-1 block text-xs text-ink-3">提示词</label>
      <textarea
        className="input mb-3 min-h-[96px] w-full resize-y font-mono text-xs"
        value={prompt}
        disabled={busy}
        onChange={(e) => patchSingleTestSession({ prompt: e.target.value })}
        placeholder={FALLBACK_PROMPT}
      />

      <div className="mb-4 flex flex-wrap items-end gap-2">
        <div className="min-w-[10rem] flex-1">
          <label className="mb-1 block text-xs text-ink-3">另存为名称</label>
          <input
            className="input w-full"
            value={saveName}
            disabled={busy}
            onChange={(e) => patchSingleTestSession({ saveName: e.target.value })}
            placeholder="例如：简短连通"
          />
        </div>
        <button
          type="button"
          className="btn-secondary"
          disabled={saveBusy || busy}
          onClick={() => void savePrompt()}
        >
          {saveBusy ? "保存中…" : "保存提示词"}
        </button>
      </div>

      <div className="mb-4 flex flex-wrap items-end justify-between gap-3">
        <div className="w-28">
          <label className="mb-1 block text-xs text-ink-3">超时（秒）</label>
          <input
            type="number"
            min={5}
            max={300}
            step={1}
            className="input w-full tabular-nums"
            value={timeoutSecs}
            disabled={busy}
            onChange={(e) => patchSingleTestSession({ timeoutSecs: Number(e.target.value) })}
            title="单次请求超时，范围 5–300 秒"
          />
        </div>
        <div className="flex justify-end gap-2">
          <button type="button" className="btn-secondary" onClick={onClose}>
            关闭
          </button>
          <button
            type="button"
            className="btn-primary min-w-[7rem]"
            disabled={busy}
            onClick={() => void runTest()}
          >
            {busy ? "测试中…" : "发送测试"}
          </button>
        </div>
      </div>

      <div className="mb-4 rounded-md border border-surface-3 bg-surface-1">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b border-surface-3 px-3 py-2">
          <div className="flex items-center gap-2 text-sm font-medium">
            <span>网络请求日志</span>
            {busy ? (
              <span className="rounded bg-accent/15 px-1.5 py-0.5 text-[10px] font-normal text-accent">
                实时输出中…
              </span>
            ) : null}
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className="btn-ghost !px-2 !py-0.5 text-xs"
              onClick={() => patchSingleTestSession({ showLog: !showLog })}
            >
              {showLog ? "收起" : "展开"}
            </button>
            <button
              type="button"
              className="btn-ghost !px-2 !py-0.5 text-xs"
              disabled={busy || (!liveLines.length && !result)}
              onClick={() => clearSingleTestLogs()}
            >
              清空
            </button>
            <button
              type="button"
              className="btn-ghost !px-2 !py-0.5 text-xs"
              disabled={!liveLines.length && !result}
              onClick={() => void copyLogs()}
            >
              复制全部
            </button>
          </div>
        </div>
        {showLog ? (
          <div className="p-3">
            <div className="mb-2 flex gap-1">
              {(
                [
                  ["timeline", "时间线"],
                  ["request", "请求"],
                  ["response", "响应"],
                ] as const
              ).map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  className={
                    logTab === id
                      ? "rounded-md bg-accent/15 px-2 py-1 text-xs text-accent"
                      : "rounded-md px-2 py-1 text-xs text-ink-3 hover:bg-surface-2 hover:text-ink-1"
                  }
                  onClick={() => patchSingleTestSession({ logTab: id })}
                >
                  {label}
                </button>
              ))}
            </div>
            <pre
              ref={logBoxRef}
              onScroll={onLogScroll}
              className="h-56 overflow-auto whitespace-pre-wrap rounded bg-surface-0 p-2 font-mono text-[11px] leading-relaxed text-ink-1"
            >
              {logTab === "timeline"
                ? timelineText
                : logTab === "request"
                  ? result
                    ? formatRequestLog(result)
                    : busy
                      ? "请求信息将在发送过程中写入时间线；完成后可在此查看汇总。"
                      : "尚未发送请求。"
                  : result
                    ? formatResponseLog(result)
                    : busy
                      ? "等待响应…"
                      : "尚未发送请求。"}
            </pre>
          </div>
        ) : null}
      </div>

      {result ? (
        <div
          className={
            result.ok
              ? "rounded-md border border-ok/40 bg-ok/10 p-3 text-sm"
              : "rounded-md border border-danger/40 bg-danger/10 p-3 text-sm"
          }
        >
          <div className="mb-2 font-medium">{result.ok ? "成功" : "失败"}</div>
          <div className="space-y-1 text-xs text-ink-2">
            <div>耗时：{result.latencyMs} ms</div>
            {result.httpStatus != null ? <div>HTTP：{result.httpStatus}</div> : null}
            {result.requestUrl ? (
              <div className="break-all font-mono text-[11px] text-ink-3">
                {result.requestMethod || "POST"} {result.requestUrl}
              </div>
            ) : null}
            {result.error ? <div className="text-danger">{result.error}</div> : null}
            {result.responseText ? (
              <pre className="mt-2 max-h-28 overflow-auto whitespace-pre-wrap rounded bg-surface-0/60 p-2 font-mono text-[11px] text-ink-1">
                {result.responseText}
              </pre>
            ) : null}
          </div>
        </div>
      ) : null}
    </Modal>
  );
}

function formatRequestLog(result: TestConnectionResult): string {
  const lines = [
    `${result.requestMethod || "POST"} ${result.requestUrl || "(no url)"}`,
    "",
    "Headers:",
    ...(result.requestHeaders?.length ? result.requestHeaders : ["(none)"]),
    "",
    "Body:",
    result.requestBody ?? "(empty)",
  ];
  return lines.join("\n");
}

function formatResponseLog(result: TestConnectionResult): string {
  const lines = [
    `HTTP ${result.httpStatus ?? "—"}  (${result.latencyMs} ms)`,
    "",
    "Headers:",
    ...(result.responseHeaders?.length ? result.responseHeaders : ["(none)"]),
    "",
    "Body:",
    result.responseBody ?? result.responseText ?? "(empty)",
  ];
  return lines.join("\n");
}

function formatFullLog(result: TestConnectionResult, liveLines: readonly string[]): string {
  const timeline = liveLines.length ? liveLines : (result.logs ?? []);
  return [
    "=== timeline ===",
    ...timeline,
    "",
    "=== request ===",
    formatRequestLog(result),
    "",
    "=== response ===",
    formatResponseLog(result),
  ].join("\n");
}
