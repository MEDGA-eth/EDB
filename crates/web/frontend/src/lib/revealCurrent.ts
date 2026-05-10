import type { QueryClient } from '@tanstack/react-query';
import { useSession } from '../store/session';
import { DISASM_PATH } from '../layout/FileTabPanel';
import type { SnapshotInfo } from './types';

/**
 * Open (or focus) the file/disasm tab matching the current snapshot's
 * location, then pulse `revealTick` so the hosting view re-runs its
 * scroll-to-current effect even when neither the active tab nor the
 * snapshot id changed since the last reveal.
 *
 * - Hook snapshot → opens `(bytecode_address, source_path)`.
 * - Opcode snapshot → opens `(bytecode_address, '<disasm>')`.
 *
 * No-op when the snapshot data isn't in the react-query cache yet.
 */
export function revealCurrentLocation(qc: QueryClient): void {
  const id = useSession.getState().currentSnapshotId;
  const snap = qc.getQueryData<SnapshotInfo>(['snapshot', id]);
  if (!snap) return;
  const addr = snap.bytecode_address;
  const path = snap.detail.kind === 'Hook' ? snap.detail.path : DISASM_PATH;
  useSession.getState().openFile({ addr, path });
  useSession.getState().bumpRevealTick();
}
