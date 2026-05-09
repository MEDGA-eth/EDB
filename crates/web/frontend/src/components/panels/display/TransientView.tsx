import type { SnapshotInfo } from '../../../lib/types';

export function TransientView({ snap }: { snap: SnapshotInfo | undefined }) {
  const t = snap?.detail.kind === 'Opcode' ? snap.detail.transient_storage : {};
  const entries = Object.entries(t);
  if (entries.length === 0) return <span>(empty)</span>;
  return (
    <ul>
      {entries.map(([k, v]) => (
        <li key={k}>
          {k} = {v}
        </li>
      ))}
    </ul>
  );
}
