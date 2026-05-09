import { useSession } from '../store/session';

export function SessionEndedOverlay() {
  const ended = useSession((s) => s.sessionEnded);
  if (!ended) return null;
  return (
    <div
      role="dialog"
      aria-modal="true"
      data-testid="session-ended"
      className="fixed inset-0 z-50 flex items-center justify-center bg-(--color-bg-root)/85"
    >
      <div className="rounded-[var(--radius)] bg-(--color-bg-elevated) p-6 shadow-[var(--shadow-lg)] text-center max-w-[480px]">
        <h2 className="font-display text-xl font-bold">Session ended</h2>
        <p className="mt-2 text-(--color-fg-secondary)">
          EDB lost the connection to the engine.
        </p>
        <p className="mt-2 text-(--color-fg-secondary)">
          This usually means the engine crashed or you ran{' '}
          <code className="rounded bg-(--color-bg-hover) px-1 py-0.5 font-mono text-[12px]">Ctrl+C</code>{' '}
          in the terminal that started it. Check the terminal for logs, then re-run{' '}
          <code className="rounded bg-(--color-bg-hover) px-1 py-0.5 font-mono text-[12px]">edb replay --ui=web &lt;tx&gt;</code>{' '}
          to start a new session.
        </p>
      </div>
    </div>
  );
}
