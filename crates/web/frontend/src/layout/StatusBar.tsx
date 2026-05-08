import { useEffect, useRef } from 'react';
import { useSession } from '../store/session';
import { useSnapshotCount } from '../hooks/useSnapshotCount';
import { ConnectionIndicator } from '../components/ConnectionIndicator';
import { ThemeToggle } from '../components/ThemeToggle';
import { HelpOverlay } from '../components/HelpOverlay';

export function StatusBar() {
  const id = useSession((s) => s.currentSnapshotId);
  const setId = useSession((s) => s.setSnapshotId);
  const { data: count } = useSnapshotCount();
  // True while a programmatic hash write is pending, so the resulting
  // `hashchange` event doesn't echo back into the store.
  const skipNextHashRef = useRef(false);

  // hash <-> store binding (formerly in TopBar)
  useEffect(() => {
    const fromHash = parseInt((window.location.hash ?? '').replace(/^#/, ''), 10);
    if (Number.isFinite(fromHash)) setId(fromHash);
    const onHash = () => {
      if (skipNextHashRef.current) {
        // Consume the echo from our own write and stop.
        skipNextHashRef.current = false;
        return;
      }
      const next = parseInt(window.location.hash.replace(/^#/, ''), 10);
      if (!Number.isFinite(next)) return;
      // Only update the store if the hash genuinely differs — guards
      // against rapid-navigation echo loops under StrictMode.
      if (useSession.getState().currentSnapshotId !== next) setId(next);
    };
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, [setId]);

  useEffect(() => {
    const target = String(id);
    if (window.location.hash.replace(/^#/, '') === target) return;
    skipNextHashRef.current = true;
    window.location.hash = target;
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
