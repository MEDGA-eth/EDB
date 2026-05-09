import type { SnapshotInfo, SolValue } from '../../../lib/types';
import { formatSolValue } from '../../../lib/types';

function VarTable({ entries }: { entries: [string, SolValue | null][] }) {
  if (entries.length === 0) return <span className="text-(--color-fg-tertiary)">(none)</span>;
  return (
    <table className="w-full">
      <tbody>
        {entries.map(([k, v]) => (
          <tr key={k} data-testid={`var-row-${k}`}>
            <td className="pr-3 text-(--color-fg-secondary)">{k}</td>
            <td className="break-all">{formatSolValue(v)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function VarsView({ snap }: { snap: SnapshotInfo | undefined }) {
  if (!snap) return <span>(no snapshot)</span>;
  // Discriminate on detail.kind — Opcode snapshots carry no source variables.
  if (snap.detail.kind === 'Opcode')
    return <span>(no source variables in opcode mode)</span>;
  // Hook snapshot: surface locals + state_variables.
  const { locals, state_variables, path, offset, length } = snap.detail;
  const localEntries = Object.entries(locals);
  const stateEntries = Object.entries(state_variables);
  return (
    <div className="flex flex-col gap-3" data-testid="vars-view">
      <div className="text-xs text-(--color-fg-tertiary)">
        {path} · offset {offset} · length {length}
      </div>
      <section>
        <h3 className="mb-1 text-xs font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
          Locals
        </h3>
        <VarTable entries={localEntries} />
      </section>
      <section>
        <h3 className="mb-1 text-xs font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
          State Variables
        </h3>
        <VarTable entries={stateEntries} />
      </section>
    </div>
  );
}
