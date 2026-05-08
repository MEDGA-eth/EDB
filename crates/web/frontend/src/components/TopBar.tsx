import { useEffect } from 'react';
import { useSession } from '../store/session';
import { useSnapshotCount } from '../hooks/useSnapshotCount';
import { ConnectionIndicator } from './ConnectionIndicator';
import { ThemeToggle } from './ThemeToggle';
import { HelpOverlay } from './HelpOverlay';

export function TopBar({ txHash }: { txHash?: string }) {
  const id = useSession((s) => s.currentSnapshotId);
  const setId = useSession((s) => s.setSnapshotId);
  const { data: count } = useSnapshotCount();

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
    <header className="flex items-center justify-between border-b border-(--color-border) bg-(--color-bg) px-4 py-2 font-display">
      <div className="flex items-center gap-3">
        <span className="font-bold">edb</span>
        {txHash && (
          <a
            href={`https://etherscan.io/tx/${txHash}`}
            target="_blank"
            rel="noreferrer"
            className="text-(--color-accent) underline-offset-2 hover:underline"
            data-testid="tx-link"
          >
            {txHash.slice(0, 10)}…{txHash.slice(-8)}
          </a>
        )}
        <span className="text-(--color-fg-tertiary)" data-testid="snapshot-label">
          snapshot {id} / {count ?? '…'}
        </span>
      </div>
      <div className="flex items-center gap-3">
        <ConnectionIndicator />
        <ThemeToggle />
        <HelpOverlay />
      </div>
    </header>
  );
}
