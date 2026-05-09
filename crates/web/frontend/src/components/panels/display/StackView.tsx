import type { SnapshotInfo } from '../../../lib/types';

export function StackView({ snap }: { snap: SnapshotInfo | undefined }) {
  if (snap?.detail.kind !== 'Opcode') {
    return (
      <div className="text-(--color-fg-tertiary) italic">
        Stack is only available in opcode-level snapshots.
        Source-level (hook) snapshots do not capture the EVM stack.
      </div>
    );
  }
  const stack = snap.detail.stack;
  if (stack.length === 0)
    return <span className="text-(--color-fg-tertiary)">(empty)</span>;
  return (
    <ol reversed start={stack.length} className="list-decimal pl-6">
      {stack.map((v, i) => (
        <li key={i}>{v}</li>
      ))}
    </ol>
  );
}
