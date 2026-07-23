type Props = {
  readonly message: string | null;
};

export function Toast({ message }: Props) {
  if (!message) return null;
  return (
    <div className="pointer-events-none fixed bottom-6 right-6 z-[100] max-w-sm animate-in">
      <div className="pointer-events-auto rounded-lg border border-surface-3 bg-surface-2 px-4 py-3 text-sm text-ink-1 shadow-xl shadow-black/40">
        {message}
      </div>
    </div>
  );
}
