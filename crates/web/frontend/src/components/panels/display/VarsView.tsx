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

function typeOf(v: SolValue | null): string | null {
  if (!v) return null;
  if (v.type === 'Uint') return `uint${v.value.bits}`;
  if (v.type === 'Int') return `int${v.value.bits}`;
  if (v.type === 'FixedBytes') return `bytes${v.value.size}`;
  if (v.type === 'Bool') return 'bool';
  if (v.type === 'Address') return 'address';
  if (v.type === 'String') return 'string';
  if (v.type === 'Bytes') return 'bytes';
  if (v.type === 'Function') return 'function';
  if (v.type === 'Array') return `${typeOf(v.value[0]) ?? 'T'}[]`;
  if (v.type === 'FixedArray') return `${typeOf(v.value[0]) ?? 'T'}[${v.value.length}]`;
  if (v.type === 'Tuple') return `(${v.value.length} fields)`;
  if (v.type === 'CustomStruct') return v.value.name;
  return null;
}

/** Coloured chip distinguishing primitive / reference / aggregate types. */
function typeChipColour(t: string): string {
  if (/^(uint|int)\d*$/.test(t)) return 'var(--color-syn-type-std)';
  if (t === 'address') return 'var(--color-syn-type)';
  if (t === 'bool') return 'var(--color-syn-atom)';
  if (t === 'string' || t === 'bytes' || /^bytes\d+$/.test(t)) return 'var(--color-syn-string)';
  if (t.endsWith(']') || t.startsWith('(')) return 'var(--color-syn-func)';
  return 'var(--color-syn-modifier)';
}

/**
 * Render each variable as a card-style row. Layout:
 *
 *   ┌─────────────────────────────┬──────────┐
 *   │ name                        │ uint256  │  ← type chip on the right
 *   ├─────────────────────────────┴──────────┤
 *   │ <value, mono, full width>              │
 *   └────────────────────────────────────────┘
 *
 * Compared to a plain two-column table this gives each variable real
 * breathing room, surfaces the type with a coloured chip, and lets long
 * values wrap underneath the name without truncating the column.
 */
function VarList({
  entries,
  emptyHint,
}: {
  entries: [string, SolValue | null][];
  emptyHint?: string;
}) {
  if (entries.length === 0)
    return (
      <div className="rounded border border-dashed border-(--color-border) px-3 py-2 text-[12px] italic text-(--color-fg-tertiary)">
        {emptyHint ?? '(none)'}
      </div>
    );
  return (
    <ul className="flex flex-col gap-1.5" role="list">
      {entries.map(([k, v]) => {
        const t = typeOf(v);
        return (
          <li
            key={k}
            data-testid={`var-row-${k}`}
            className="rounded-md border border-(--color-border) bg-(--color-bg-elevated)/60 px-3 py-2 hover:border-(--color-border-strong) transition"
          >
            <div className="flex items-baseline justify-between gap-2">
              <span
                className="break-all font-display text-[14px] font-semibold text-(--color-fg)"
                title={k}
              >
                {k}
              </span>
              {t && (
                <span
                  className="shrink-0 rounded-full px-2 py-0.5 font-mono text-[11px] tracking-wide"
                  style={{
                    color: typeChipColour(t),
                    backgroundColor: 'var(--color-bg)',
                    border: '1px solid var(--color-border)',
                  }}
                >
                  {t}
                </span>
              )}
            </div>
            <div className="mt-1.5 font-mono text-[13px] text-(--color-fg) leading-relaxed">
              <SolValueCell value={v} />
            </div>
          </li>
        );
      })}
    </ul>
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
        <h3 className="mb-1.5 text-[12px] font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
          Locals
        </h3>
        <VarList
          entries={localEntries}
          emptyHint="No locals bound at this snapshot. Local variables appear as the function body assigns them — step forward (F11) to watch them populate."
        />
      </section>
      <section>
        <h3 className="mb-1.5 text-[12px] font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
          State Variables
        </h3>
        <VarList
          entries={stateEntries}
          emptyHint="This contract exposes no Solidity-visible state at this snapshot, or storage decoding hasn't been registered for it."
        />
      </section>
    </div>
  );
}
