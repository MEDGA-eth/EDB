interface Props {
  code?: number;
  message: string;
  onRetry?: () => void;
}

export function ErrorCard({ code, message, onRetry }: Props) {
  return (
    <div
      role="alert"
      data-testid="error-card"
      className="rounded-[var(--radius)] border border-(--color-danger) bg-(--color-bg-elevated) p-4 text-(--color-fg)"
    >
      <div className="font-display font-semibold text-(--color-danger)">
        Error{code !== undefined ? ` (${code})` : ''}
      </div>
      <div className="mt-1 text-sm">{message}</div>
      {onRetry && (
        <button
          type="button"
          onClick={onRetry}
          className="mt-2 rounded-[var(--radius)] bg-(--color-accent) px-3 py-1 text-white"
        >
          Retry
        </button>
      )}
    </div>
  );
}
