import type { SnapshotInfo, SolValue } from '../../../lib/types';
import { formatSolValueParts } from '../../../lib/types';
import { useSession } from '../../../store/session';
import { useAvailableFiles } from '../../../hooks/useAvailableFiles';

/**
 * Render a `SolValue` for a Vars table cell:
 *
 * - The primary value is the main monospaced text.
 * - The type suffix (`uint256`, `int8`, `bytes4`, …) is shown in
 *   `--color-fg-tertiary` so it reads as secondary metadata.
 * - For `Bytes` longer than the truncation threshold, the truncated head/tail
 *   form is shown by default with a `<details>` toggle to expand the full
 *   hex payload in a separate row.
 * - For `Address` values, the cell becomes a button: clicking it opens the
 *   first file under that address in the editor (no-op if the address has
 *   no known files).
 */
function SolValueCell({ value }: { value: SolValue | null }) {
  const parts = formatSolValueParts(value);
  const openFile = useSession((s) => s.openFile);
  const { perAddress } = useAvailableFiles();

  // Address click-through: open the first file under that address, if any.
  if (value && value.type === 'Address') {
    const addrLc = value.value.toLowerCase();
    const entry = perAddress.find((p) => p.addr === addrLc);
    const firstFile = entry?.files[0];
    if (firstFile) {
      return (
        <button
          type="button"
          data-testid={`var-address-${addrLc}`}
          onClick={() => openFile({ addr: firstFile.addr, path: firstFile.path })}
          className="text-(--color-syn-type) hover:underline focus:underline"
          title={`Open ${firstFile.path}`}
        >
          {parts.value}
        </button>
      );
    }
    // Fall through to plain rendering if the address has no known files.
  }

  if (parts.truncatedFullValue) {
    return (
      <details>
        <summary className="cursor-pointer">
          <span className="break-all">{parts.value}</span>
          {parts.suffix && (
            <span className="ml-2 text-xs text-(--color-fg-tertiary)">
              ({parts.suffix})
            </span>
          )}
        </summary>
        <pre className="mt-1 break-all whitespace-pre-wrap text-xs text-(--color-fg-secondary)">
          {parts.truncatedFullValue}
        </pre>
      </details>
    );
  }

  return (
    <span className="break-all">
      {parts.value}
      {parts.suffix && (
        <span className="ml-2 text-xs text-(--color-fg-tertiary)">
          ({parts.suffix})
        </span>
      )}
    </span>
  );
}

function VarTable({ entries }: { entries: [string, SolValue | null][] }) {
  if (entries.length === 0) return <span className="text-(--color-fg-tertiary)">(none)</span>;
  return (
    <table className="w-full">
      <tbody>
        {entries.map(([k, v]) => (
          <tr key={k} data-testid={`var-row-${k}`}>
            <td className="pr-3 text-(--color-fg-secondary) align-top">{k}</td>
            <td>
              <SolValueCell value={v} />
            </td>
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
    return (
      <div className="text-(--color-fg-tertiary) italic">
        Source-level variables are only available in hook (source) snapshots.
        Opcode-level snapshots do not capture decoded locals or state variables.
      </div>
    );
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
