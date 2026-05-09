import type { SnapshotInfo } from '../../../lib/types';

export function StackView({ snap }: { snap: SnapshotInfo | undefined }) {
  const stack = snap?.detail.kind === 'Opcode' ? snap.detail.stack : [];
  if (stack.length === 0) return <span>(no stack in source mode)</span>;
  return (
    <ol reversed start={stack.length} className="list-decimal pl-6">
      {stack.map((v, i) => (
        <li key={i}>{v}</li>
      ))}
    </ol>
  );
}
