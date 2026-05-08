import { useEffect } from 'react';
import { useSession } from '../store/session';
import { useSnapshotCount } from '../hooks/useSnapshotCount';
import { ConnectionIndicator } from '../components/ConnectionIndicator';
import { ThemeToggle } from '../components/ThemeToggle';
import { HelpOverlay } from '../components/HelpOverlay';

export function StatusBar() {
  const id = useSession((s) => s.currentSnapshotId);
  const setId = useSession((s) => s.setSnapshotId);
  const { data: count } = useSnapshotCount();

  // hash <-> store binding (formerly in TopBar)
  useEffect(() => {
    const fromHash = parseInt((window.location.hash ?? '').replace(/^#/, ''), 10);
    if (Number.isFinite(fromHash)) setId(fromHash);
    const onHash = () => {
      const next = parseInt(window.location.hash.replace(/^#/, ''), 10);
      if (Number.isFinite(next)) setId(next);
    };
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, [setId]);

  useEffect(() => {
    window.location.hash = String(id);
  }, [id]);

  return (
    <footer
      className="flex h-[22px] items-center justify-between border-t border-(--color-border) bg-(--color-bg) px-3 font-display text-xs"
      data-testid="status-bar"
    >
      <div className="flex items-center gap-3">
        <span className="font-bold text-(--color-fg-secondary)">edb</span>
        <span className="text-(--color-fg-tertiary)" data-testid="snapshot-label">
          snapshot {id} / {count ?? '…'}
        </span>
      </div>
      <div className="flex items-center gap-3">
        <ConnectionIndicator />
        <ThemeToggle />
        <HelpOverlay />
      </div>
    </footer>
  );
}
