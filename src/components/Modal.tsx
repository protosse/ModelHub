import { useEffect, useRef } from "react";

type Props = {
  readonly onClose: () => void;
  readonly children: React.ReactNode;
  readonly wide?: boolean;
  /** Wider than `wide` — for logs / long content. */
  readonly xwide?: boolean;
};

export function Modal({ onClose, children, wide, xwide }: Props) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const width = xwide ? "max-w-2xl" : wide ? "max-w-lg" : "max-w-md";

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className={`card max-h-[85vh] w-full overflow-auto p-5 ${width}`}
        role="dialog"
        aria-modal="true"
      >
        {children}
      </div>
    </div>
  );
}

type ConfirmProps = {
  readonly title: string;
  readonly message: string;
  readonly confirmLabel?: string;
  readonly danger?: boolean;
  readonly busy?: boolean;
  readonly onCancel: () => void;
  readonly onConfirm: () => void;
};

export function ConfirmDialog({
  title,
  message,
  confirmLabel = "确认",
  danger,
  busy,
  onCancel,
  onConfirm,
}: ConfirmProps) {
  // Guard against Enter on a focused button also synthesizing a click.
  const locked = useRef(false);

  useEffect(() => {
    if (!busy) locked.current = false;
  }, [busy]);

  const fireConfirm = () => {
    if (busy || locked.current) return;
    locked.current = true;
    onConfirm();
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (busy || e.repeat) return;
      if (e.key !== "Enter") return;
      // Confirm dialogs have no text fields; Enter always means confirm.
      e.preventDefault();
      e.stopPropagation();
      fireConfirm();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // fireConfirm closes over latest busy/onConfirm via deps below
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [busy, onConfirm]);

  return (
    <Modal onClose={() => !busy && onCancel()}>
      <h3 className="mb-2 text-base font-semibold">{title}</h3>
      <p className="text-sm text-ink-2 whitespace-pre-wrap">{message}</p>
      <p className="mt-2 text-[11px] text-ink-3">回车确认 · Esc 取消</p>
      <div className="mt-5 flex justify-end gap-2">
        <button type="button" className="btn-secondary" disabled={busy} onClick={onCancel}>
          取消
        </button>
        <button
          type="button"
          className={danger ? "btn-danger" : "btn-primary"}
          disabled={busy}
          onClick={fireConfirm}
          autoFocus
        >
          {busy ? "处理中…" : confirmLabel}
        </button>
      </div>
    </Modal>
  );
}
