import { useEffect } from "react";

type Props = {
  readonly onClose: () => void;
  readonly children: React.ReactNode;
  readonly wide?: boolean;
};

export function Modal({ onClose, children, wide }: Props) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className={`card max-h-[85vh] w-full overflow-auto p-5 ${wide ? "max-w-lg" : "max-w-md"}`}
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
  return (
    <Modal onClose={onCancel}>
      <h3 className="mb-2 text-base font-semibold">{title}</h3>
      <p className="text-sm text-ink-2 whitespace-pre-wrap">{message}</p>
      <div className="mt-5 flex justify-end gap-2">
        <button type="button" className="btn-secondary" disabled={busy} onClick={onCancel}>
          取消
        </button>
        <button
          type="button"
          className={danger ? "btn-danger" : "btn-primary"}
          disabled={busy}
          onClick={onConfirm}
        >
          {busy ? "处理中…" : confirmLabel}
        </button>
      </div>
    </Modal>
  );
}
