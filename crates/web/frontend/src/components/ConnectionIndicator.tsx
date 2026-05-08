import { useSession } from '../store/session';

const STYLES: Record<string, { color: string; label: string; emoji: string }> = {
  connected: { color: 'var(--color-success)', label: 'Connected', emoji: '🟢' },
  degraded: { color: 'var(--color-warn)', label: 'Degraded', emoji: '🟡' },
  offline: { color: 'var(--color-danger)', label: 'Offline', emoji: '🔴' },
};

export function ConnectionIndicator() {
  const state = useSession(s => s.connection);
  const meta = STYLES[state];
  return (
    <span data-testid="connection-indicator" data-state={state} aria-label={meta.label}
          style={{ color: meta.color }}>
      {meta.emoji} {meta.label}
    </span>
  );
}
