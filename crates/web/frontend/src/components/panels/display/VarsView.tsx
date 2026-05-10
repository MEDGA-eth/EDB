import { Boxes, FileCode2 } from 'lucide-react';
import type { SnapshotInfo, SolValue } from '../../../lib/types';
import { formatSolValueParts } from '../../../lib/types';
import { useSession } from '../../../store/session';
import { useAvailableFiles } from '../../../hooks/useAvailableFiles';

/** Strip the directory chain from a path so the sidebar header shows just
 *  the file name; full path stays available via the wrapper's `title`. */
function basename(p: string): string {
  if (!p) return '';
  const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
  return idx === -1 ? p : p.slice(idx + 1);
}

/** `0xABCD…1234` style for compact metadata rendering. The full address
 *  remains available via the wrapper's `title` attribute. */
function shortAddr(addr: string): string {
  if (!addr) return '';
  if (addr.length <= 12) return addr;
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`;
}

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

  // Title attribute lets users hover to see the full value when the parent
  // grid cell ellipsises a long string (e.g., addresses in the narrow
  // sidebar). The parent uses `truncate`; inner spans stay simple and inherit
  // the nowrap behaviour, so we don't need `break-all` here anymore.
  const fullText = parts.value + (parts.suffix ? ` (${parts.suffix})` : '');
  if (parts.truncatedFullValue) {
    return (
      <details>
        <summary className="cursor-pointer truncate" title={fullText}>
          <span>{parts.value}</span>
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
    <span title={fullText}>
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
 * Render each variable as a row. Two layouts depending on context:
 *
 * Display panel (`compact={false}`), one-liner via subgrid:
 *   ┌──────────────┬───────────────────────────┬──────────┐
 *   │ name         │ <value, mono>             │ uint256  │
 *   └──────────────┴───────────────────────────┴──────────┘
 *
 * Sidebar (`compact={true}`), two lines per row, the value lives on its
 * own line so long hex addresses no longer wrap character-by-character:
 *   ┌─────────────────────────────────────────┐
 *   │ name                          [uint256] │
 *   │   0xcc6a4ad6e1b…7990                    │
 *   └─────────────────────────────────────────┘
 */
function VarList({
  entries,
  emptyHint,
  compact = false,
}: {
  entries: [string, SolValue | null][];
  emptyHint?: string;
  compact?: boolean;
}) {
  if (entries.length === 0)
    return (
      <div className="rounded border border-dashed border-(--color-border) px-3 py-2 text-[12px] italic text-(--color-fg-tertiary)">
        {emptyHint ?? '(none)'}
      </div>
    );

  if (compact) {
    return (
      <ul
        className="flex flex-col rounded-md border border-(--color-border) bg-(--color-bg-elevated)/40"
        role="list"
      >
        {entries.map(([k, v], i) => {
          const t = typeOf(v);
          return (
            <li
              key={k}
              data-testid={`var-row-${k}`}
              className={`flex flex-col gap-1 px-3 py-2 transition hover:bg-(--color-bg-hover)/40 ${
                i === 0 ? '' : 'border-t border-dashed border-(--color-border)'
              }`}
            >
              <div className="flex items-baseline justify-between gap-2 min-w-0">
                <span
                  className="min-w-0 flex-1 truncate font-display text-[12.5px] font-semibold text-(--color-fg)"
                  title={k}
                >
                  {k}
                </span>
                {t && (
                  <span
                    className="shrink-0 rounded-full border border-(--color-border) bg-(--color-bg) px-2 py-0.5 font-mono text-[10.5px] tracking-wide"
                    style={{ color: typeChipColour(t) }}
                  >
                    {t}
                  </span>
                )}
              </div>
              <div className="font-mono text-[12.5px] text-(--color-fg) leading-relaxed break-all">
                <SolValueCell value={v} />
              </div>
            </li>
          );
        })}
      </ul>
    );
  }

  return (
    <ul
      // Grid columns on the <ul>, inherited via grid-cols-subgrid on every
      // <li>, so name / value / chip widths line up across all rows even
      // when a particular row has no type chip.
      className="grid grid-cols-[minmax(0,11rem)_minmax(0,1fr)_auto] rounded-md border border-(--color-border) bg-(--color-bg-elevated)/40"
      role="list"
    >
      {entries.map(([k, v], i) => {
        const t = typeOf(v);
        return (
          <li
            key={k}
            data-testid={`var-row-${k}`}
            className={`grid grid-cols-subgrid col-span-3 items-baseline gap-3 px-3 py-1.5 transition hover:bg-(--color-bg-hover)/40 ${
              i === 0 ? '' : 'border-t border-dashed border-(--color-border)'
            }`}
          >
            <span
              className="truncate font-display text-[12.5px] font-semibold text-(--color-fg)"
              title={k}
            >
              {k}
            </span>
            <div className="min-w-0 truncate font-mono text-[12.5px] text-(--color-fg) leading-relaxed">
              <SolValueCell value={v} />
            </div>
            {t ? (
              <span
                className="shrink-0 rounded-full border border-(--color-border) bg-(--color-bg) px-2 py-0.5 font-mono text-[10.5px] tracking-wide"
                style={{ color: typeChipColour(t) }}
              >
                {t}
              </span>
            ) : (
              <span aria-hidden />
            )}
          </li>
        );
      })}
    </ul>
  );
}

export function VarsView({
  snap,
  compact = false,
}: {
  snap: SnapshotInfo | undefined;
  /** Sidebar context squeezes long values onto their own line; default
   *  (display panel) keeps the one-liner row layout. */
  compact?: boolean;
}) {
  if (!snap) return <span>(no snapshot)</span>;
  // Discriminate on detail.kind, Opcode snapshots carry no source variables.
  if (snap.detail.kind === 'Opcode')
    return (
      <div className="text-(--color-fg-tertiary) italic">
        Source-level variables are only available in hook (source) snapshots.
        Opcode-level snapshots do not capture decoded locals or state variables.
      </div>
    );
  // Hook snapshot: surface locals + state_variables.
  const { locals, state_variables, path, offset, length } = snap.detail;
  const targetAddr = snap.target_address;
  const localEntries = Object.entries(locals);
  const stateEntries = Object.entries(state_variables);

  /* Compact source-info: address / file / offset+length stacked on three
     lines so a 280px sidebar doesn't truncate any of them. Display panel
     uses the same data inline. */
  const sourceInfo = compact ? (
    <div
      className="flex flex-col gap-1.5 rounded-md border border-(--color-border) bg-(--color-bg-elevated)/40 px-3 py-2"
      data-testid="vars-source-info"
    >
      <div className="flex items-center gap-2 min-w-0" title={targetAddr}>
        <Boxes
          size={14}
          aria-hidden
          className="shrink-0 text-(--color-syn-type)"
        />
        <span
          className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-(--color-fg-secondary)"
          data-testid="vars-source-address"
        >
          {shortAddr(targetAddr)}
        </span>
      </div>
      <div className="flex items-center gap-2 min-w-0" title={path}>
        <FileCode2
          size={14}
          aria-hidden
          className="shrink-0 text-(--color-syn-type-std)"
        />
        <span
          className="min-w-0 flex-1 truncate font-mono text-[12px] font-semibold text-(--color-fg)"
          data-testid="vars-source-file"
        >
          {basename(path) || path}
        </span>
      </div>
      <div className="flex items-center gap-1.5">
        <span className="shrink-0 rounded-full border border-(--color-border) bg-(--color-bg) px-2 py-0.5 font-mono text-[10.5px] text-(--color-fg-tertiary)">
          offset <span className="text-(--color-fg-secondary)">{offset}</span>
        </span>
        <span className="shrink-0 rounded-full border border-(--color-border) bg-(--color-bg) px-2 py-0.5 font-mono text-[10.5px] text-(--color-fg-tertiary)">
          length <span className="text-(--color-fg-secondary)">{length}</span>
        </span>
      </div>
    </div>
  ) : (
    <div
      className="flex items-center gap-2 rounded-md border border-(--color-border) bg-(--color-bg-elevated)/40 px-2 py-1.5 min-w-0"
      title={`${targetAddr}\n${path}`}
      data-testid="vars-source-info"
    >
      <Boxes
        size={14}
        aria-hidden
        className="shrink-0 text-(--color-syn-type)"
      />
      <span
        className="shrink-0 font-mono text-[11.5px] text-(--color-fg-secondary)"
        data-testid="vars-source-address"
      >
        {shortAddr(targetAddr)}
      </span>
      <span className="shrink-0 text-(--color-fg-tertiary)">·</span>
      <FileCode2
        size={14}
        aria-hidden
        className="shrink-0 text-(--color-syn-type-std)"
      />
      <span
        className="min-w-0 flex-1 truncate font-mono text-[12px] font-semibold text-(--color-fg)"
        data-testid="vars-source-file"
      >
        {basename(path) || path}
      </span>
      <span className="shrink-0 rounded-full border border-(--color-border) bg-(--color-bg) px-2 py-0.5 font-mono text-[10.5px] text-(--color-fg-tertiary)">
        offset <span className="text-(--color-fg-secondary)">{offset}</span>
      </span>
      <span className="shrink-0 rounded-full border border-(--color-border) bg-(--color-bg) px-2 py-0.5 font-mono text-[10.5px] text-(--color-fg-tertiary)">
        length <span className="text-(--color-fg-secondary)">{length}</span>
      </span>
    </div>
  );

  return (
    <div className="flex flex-col gap-3" data-testid="vars-view">
      {sourceInfo}
      <section>
        <h3 className="mb-1.5 text-[12px] font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
          Locals
        </h3>
        <VarList
          entries={localEntries}
          compact={compact}
          emptyHint="No locals bound at this snapshot. Local variables appear as the function body assigns them, step forward (F11) to watch them populate."
        />
      </section>
      <section>
        <h3 className="mb-1.5 text-[12px] font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
          State Variables
        </h3>
        <VarList
          entries={stateEntries}
          compact={compact}
          emptyHint="This contract exposes no Solidity-visible state at this snapshot, or storage decoding hasn't been registered for it."
        />
      </section>
    </div>
  );
}
