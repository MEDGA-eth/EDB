import { useEffect, useRef } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { rpc } from '../lib/rpc';
import { SnapshotInfo as SnapshotInfoSchema, type SnapshotInfo } from '../lib/types';
import { useSession } from '../store/session';
import { DISASM_PATH } from '../layout/FileTabPanel';

/**
 * When the current snapshot changes (via toolbar, palette, terminal,
 * URL-hash, etc.), automatically open the editor tab showing that
 * snapshot's source file. The cached snapshot is preferred so stepping
 * never blocks the UI; we fall through to a best-effort fetch otherwise.
 *
 * Skips initial mount (`currentSnapshotId === 0` with no prior value) so
 * we don't rip the focus away from a fresh blank IDE before the user
 * has even loaded a trace.
 */
export function useSnapshotFollow(): void {
  const id = useSession((s) => s.currentSnapshotId);
  const qc = useQueryClient();
  // Track the last id we acted on so StrictMode-induced double-runs don't
  // re-fetch redundantly.
  const lastSeenRef = useRef<number | null>(null);

  useEffect(() => {
    if (lastSeenRef.current === id) return;
    lastSeenRef.current = id;

    const cached = qc.getQueryData<SnapshotInfo>(['snapshot', id]);
    if (cached) {
      openForSnapshot(cached);
      return;
    }
    // Best-effort fetch. Bounded staleTime mirrors what the trace click
    // handler uses so we share cache hits.
    qc.fetchQuery({
      queryKey: ['snapshot', id] as const,
      queryFn: () => rpc('edb_getSnapshotInfo', SnapshotInfoSchema, [id]),
      staleTime: 30_000,
    }).then(
      (snap) => openForSnapshot(snap),
      () => {
        // best-effort: failure to fetch leaves the editor where it was
      },
    );
  }, [id, qc]);
}

function openForSnapshot(snap: SnapshotInfo): void {
  const addr = snap.bytecode_address;
  if (!addr) return;
  const path = snap.detail.kind === 'Hook' ? snap.detail.path : DISASM_PATH;
  const session = useSession.getState();
  session.openFile({ addr, path });
  // openFile is idempotent (no-op when the tab already exists). Set it
  // active too so dockview surfaces it when stepping causes a tab change.
  const fileId = `${addr}::${path}`;
  session.setActiveFile(fileId);
}
