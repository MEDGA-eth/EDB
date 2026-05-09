import type { SnapshotInfo } from '../../../lib/types';

export function TransientView({ snap }: { snap: SnapshotInfo | undefined }) {
  if (snap?.detail.kind !== 'Opcode') {
    return (
      <div className="text-(--color-fg-tertiary) italic">
        Transient storage is only available in opcode-level snapshots.
        Source-level (hook) snapshots do not capture transient storage.
      </div>
    );
  }
  const t = snap.detail.transient_storage;
  const entries = Object.entries(t);
  if (entries.length === 0)
    return <span className="text-(--color-fg-tertiary)">(empty)</span>;
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
