import { useEffect, useRef } from 'react';
import { useSession } from '../store/session';
import { useSnapshotCount } from '../hooks/useSnapshotCount';
import { ConnectionIndicator } from '../components/ConnectionIndicator';
import { ThemeToggle } from '../components/ThemeToggle';
import { HelpOverlay } from '../components/HelpOverlay';

export function StatusBar() {
  const id = useSession((s) => s.currentSnapshotId);
  const setId = useSession((s) => s.setSnapshotId);
  const { data: count, isSuccess: countLoaded } = useSnapshotCount();
  // True while a programmatic hash write is pending, so the resulting
  // `hashchange` event doesn't echo back into the store.
  const skipNextHashRef = useRef(false);
  // Latest snapshot count — read inside listeners that outlive the closure.
  const countRef = useRef<number | undefined>(count);
  useEffect(() => {
    countRef.current = count;
  }, [count]);

  // When the snapshot count itself shrinks (e.g., trace reload returns fewer
  // snapshots), reclamp `currentSnapshotId` so the URL never points past the
  // last valid id. We deliberately skip the `count === 0` case — that's the
  // loading state, not an empty trace; the trace just hasn't arrived yet.
  useEffect(() => {
    if (!countLoaded) return;
    if (typeof count !== 'number' || count <= 0) return;
    const cur = useSession.getState().currentSnapshotId;
    const hi = count - 1;
    if (cur > hi) {
      setId(hi);
      const target = String(hi);
      if (window.location.hash.replace(/^#/, '') !== target) {
        skipNextHashRef.current = true;
        window.location.hash = target;
      }
    }
  }, [count, countLoaded, setId]);

  // Clamp `n` to the current valid snapshot range. Returns the clamped value.
  // When count is undefined (still loading), only the lower bound applies.
  const clampToRange = (n: number): number => {
    const lo = 0;
    const c = countRef.current;
    if (typeof c === 'number') {
      const hi = Math.max(0, c - 1);
      return Math.min(Math.max(lo, n), hi);
    }
    return Math.max(lo, n);
  };

  // hash <-> store binding (formerly in TopBar)
  useEffect(() => {
    // Initial mount: read the hash and clamp; if it was out of bounds,
    // rewrite the hash so the URL reflects the clamped value.
    const fromHash = parseInt((window.location.hash ?? '').replace(/^#/, ''), 10);
    if (Number.isFinite(fromHash)) {
      const clamped = clampToRange(fromHash);
      setId(clamped);
      if (clamped !== fromHash) {
        skipNextHashRef.current = true;
        window.location.hash = String(clamped);
      }
    }
    const onHash = () => {
      if (skipNextHashRef.current) {
        // Consume the echo from our own write and stop.
        skipNextHashRef.current = false;
        return;
      }
      const next = parseInt(window.location.hash.replace(/^#/, ''), 10);
      if (!Number.isFinite(next)) return;
      const clamped = clampToRange(next);
      // Only update the store if the hash genuinely differs — guards
      // against rapid-navigation echo loops under StrictMode.
      if (useSession.getState().currentSnapshotId !== clamped) setId(clamped);
      if (clamped !== next) {
        skipNextHashRef.current = true;
        window.location.hash = String(clamped);
      }
    };
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
    // setId is stable; clampToRange reads countRef.current so no dep needed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [setId]);

  useEffect(() => {
    const target = String(id);
    if (window.location.hash.replace(/^#/, '') === target) return;
    skipNextHashRef.current = true;
    window.location.hash = target;
  }, [id]);

  return (
    <footer
      className="flex h-6 items-center justify-between border-t border-(--color-border) bg-(--color-bg) px-3 font-display text-[12px]"
      data-testid="status-bar"
    >
      <div className="flex items-center gap-3">
        <span className="font-bold text-(--color-fg-secondary)">edb</span>
        <span className="text-(--color-fg-secondary)" data-testid="snapshot-label">
          snapshot {id} / {count ?? '…'}
        </span>
      </div>
      <div className="flex items-center gap-3">
        <PaletteHint />
        <ConnectionIndicator />
        <ThemeToggle />
        <HelpOverlay />
      </div>
    </footer>
  );
}

/** Detect macOS once per page load — doesn't change between renders. */
const IS_MAC =
  typeof navigator !== 'undefined' &&
  typeof navigator.platform === 'string' &&
  navigator.platform.toLowerCase().includes('mac');

/**
 * Small `kbd`-styled hint reminding users of the command-palette shortcut.
 * Sits in the status bar so it's always visible without ever stealing focus.
 */
function PaletteHint() {
  return (
    <span
      data-testid="palette-hint"
      aria-hidden
      className="hidden items-center gap-1 text-(--color-fg-tertiary) sm:inline-flex"
    >
      <kbd className="rounded border border-(--color-border) bg-(--color-bg-elevated) px-1 font-mono text-[10px]">
        {IS_MAC ? '⌘' : 'Ctrl'}P
      </kbd>
    </span>
  );
}
