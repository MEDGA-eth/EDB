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
      <div className="rounded-[var(--radius)] bg-(--color-bg-elevated) p-6 shadow-[var(--shadow-lg)] text-center">
        <h2 className="font-display text-xl font-bold">Session ended</h2>
        <p className="mt-2 text-(--color-fg-secondary)">
          The engine has shut down. Re-run <code>edb replay --ui=web &lt;tx&gt;</code> to resume.
        </p>
      </div>
    </div>
  );
}
