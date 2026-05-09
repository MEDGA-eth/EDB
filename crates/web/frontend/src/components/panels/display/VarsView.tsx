import type { SnapshotInfo } from '../../../lib/types';

export function VarsView({ snap }: { snap: SnapshotInfo | undefined }) {
  if (!snap) return <span>(no snapshot)</span>;
  // Discriminate on detail.kind — Opcode snapshots carry no source variables.
  if (snap.detail.kind === 'Opcode')
    return <span>(no source variables in opcode mode)</span>;
  // Hook snapshot: surface locals + state_variables.
  const { locals, state_variables, path, offset, length } = snap.detail;
  return (
    <pre>
      {JSON.stringify({ path, offset, length, locals, state_variables }, null, 2)}
    </pre>
  );
}
