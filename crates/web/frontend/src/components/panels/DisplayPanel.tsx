import { useMemo, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Copy, RefreshCw } from 'lucide-react';
import { useSession } from '../../store/session';
import { useSnapshotInfo } from '../../hooks/useSnapshotInfo';
import { useStorageDiff } from '../../hooks/useStorageDiff';
import { ErrorBoundary } from '../ErrorBoundary';
import { ErrorCard } from '../ErrorCard';
import { Toolbar, ToolbarButton, ToolbarDivider } from '../Toolbar';
import type { SnapshotInfo } from '../../lib/types';

type Tab = 'vars' | 'stack' | 'memory' | 'storage' | 'transient';
const TABS: { key: Tab; label: string }[] = [
  { key: 'vars', label: 'Variables' },
  { key: 'stack', label: 'Stack' },
  { key: 'memory', label: 'Memory' },
  { key: 'storage', label: 'Storage' },
  { key: 'transient', label: 'Transient' },
];

export function DisplayPanel() {
  return (
    <ErrorBoundary label="DisplayPanel">
      <DisplayPanelInner />
    </ErrorBoundary>
  );
}

interface StorageRow {
  slot: string;
  before: string | null;
  after: string | null;
}

function DisplayPanelInner() {
  const id = useSession((s) => s.currentSnapshotId);
  const { data, error, refetch } = useSnapshotInfo(id);
  const { data: storageData } = useStorageDiff(id);
  const [tab, setTab] = useState<Tab>('vars');
  const qc = useQueryClient();

  if (error) return <ErrorCard message={(error as Error).message} onRetry={() => refetch()} />;

  // `useSnapshotInfo` is currently schema-less (z.unknown), so we narrow defensively.
  const snap = data as SnapshotInfo | undefined;
  const opcode = snap?.detail?.kind === 'Opcode' ? snap.detail : undefined;

  function copyActive() {
    let text = '';
    if (tab === 'vars') text = JSON.stringify(data, null, 2);
    else if (tab === 'stack') text = (opcode?.stack ?? []).join('\n');
    else if (tab === 'memory') text = formatMemory(opcode?.memory ?? []);
    else if (tab === 'storage')
      text = (storageData ?? [])
        .map((d: StorageRow) => `${d.slot}\t${d.before ?? ''}\t${d.after ?? ''}`)
        .join('\n');
    else if (tab === 'transient')
      text = Object.entries(opcode?.transient_storage ?? {})
        .map(([k, v]) => `${k}=${v}`)
        .join('\n');
    if (navigator.clipboard?.writeText) {
      void navigator.clipboard.writeText(text);
    }
  }

  function refreshActive() {
    qc.invalidateQueries({ queryKey: ['snapshot', id] });
    qc.invalidateQueries({ queryKey: ['storage-diff', id] });
  }

  return (
    <div className="flex h-full flex-col">
      <Toolbar testid="display-toolbar">
        <ToolbarButton
          icon={RefreshCw}
          label="Refresh"
          testid="display-refresh"
          onClick={refreshActive}
        />
        <ToolbarButton
          icon={Copy}
          label="Copy active tab"
          testid="display-copy"
          onClick={copyActive}
        />
        <ToolbarDivider />
        <span className="font-display text-[11px] text-(--color-fg-tertiary)">
          snapshot {id}
        </span>
      </Toolbar>
      <div
        role="tablist"
        className="flex gap-1 border-b border-(--color-border) bg-(--color-bg)"
      >
        {TABS.map(({ key, label }) => (
          <button
            key={key}
            role="tab"
            aria-selected={tab === key}
            data-testid={`display-tab-${key}`}
            onClick={() => setTab(key)}
            className={`px-3 py-1 text-sm ${
              tab === key
                ? 'text-(--color-accent) font-semibold'
                : 'text-(--color-fg-secondary)'
            }`}
          >
            {label}
          </button>
        ))}
      </div>
      <div
        data-testid={`display-tab-content-${tab}`}
        className="flex-1 overflow-auto p-3 font-mono text-sm"
      >
        {!data ? (
          <span className="text-(--color-fg-tertiary)">Loading…</span>
        ) : (
          {
            vars: <VarsView snap={snap} />,
            stack: <StackView snap={snap} />,
            memory: <MemoryView snap={snap} />,
            storage: <StorageView id={id} />,
            transient: <TransientView snap={snap} />,
          }[tab]
        )}
      </div>
    </div>
  );
}

function VarsView({ snap }: { snap: SnapshotInfo | undefined }) {
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

function StackView({ snap }: { snap: SnapshotInfo | undefined }) {
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

function formatMemory(mem: number[]): string {
  const rows: string[] = [];
  for (let i = 0; i < mem.length; i += 32) {
    const slice = mem
      .slice(i, i + 32)
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('');
    rows.push(`${i.toString(16).padStart(6, '0')}: ${slice}`);
  }
  return rows.join('\n');
}

/**
 * For a 16 KB memory image we'd render ~512 rows of hex; the synchronous
 * formatting can stutter paint. Default to the first 4 KB and let the user
 * opt into the full dump.
 */
const MEMORY_FORMAT_DEFAULT_BYTES = 4 * 1024;
const MEMORY_LARGE_THRESHOLD = 16 * 1024;

function MemoryView({ snap }: { snap: SnapshotInfo | undefined }) {
  const mem = snap?.detail.kind === 'Opcode' ? snap.detail.memory : [];
  const [showAll, setShowAll] = useState(false);
  const isLarge = mem.length > MEMORY_LARGE_THRESHOLD;
  // Stable identity for memoization: array reference + length covers both
  // "same snapshot" (reference equal) and "snapshot changed" (length-change
  // common, reference change definitive).
  const formatted = useMemo(() => {
    if (mem.length === 0) return '';
    if (isLarge && !showAll) {
      return formatMemory(mem.slice(0, MEMORY_FORMAT_DEFAULT_BYTES));
    }
    return formatMemory(mem);
  }, [mem, isLarge, showAll]);
  return (
    <div>
      {isLarge && !showAll && (
        <div className="mb-2 flex items-center gap-2 text-xs text-(--color-fg-tertiary)">
          <span>
            Showing first {MEMORY_FORMAT_DEFAULT_BYTES} bytes of {mem.length}.
          </span>
          <button
            type="button"
            data-testid="memory-show-all"
            onClick={() => setShowAll(true)}
            className="rounded border border-(--color-border) px-2 py-0.5 hover:bg-(--color-bg-hover)"
          >
            Show full memory
          </button>
        </div>
      )}
      <pre>{formatted}</pre>
    </div>
  );
}

function StorageView({ id }: { id: number }) {
  const { data, error } = useStorageDiff(id);
  if (error) return <span>{(error as Error).message}</span>;
  if (!data) return <span>Loading…</span>;
  // Drop slots that didn't change — the engine sometimes echoes touched-but-
  // unchanged slots, and rendering them with red strikethrough → green text
  // visually implies a mutation that didn't happen.
  const rows = data.filter((d) => d.before !== d.after);
  if (rows.length === 0)
    return <span className="text-(--color-fg-tertiary)">(no storage changes)</span>;
  return (
    <table className="w-full">
      <tbody>
        {rows.map((d, i) => (
          <tr key={i} data-testid={`storage-row-${i}`}>
            <td className="pr-3 text-(--color-fg-secondary)">{d.slot}</td>
            <td className="pr-3 line-through text-(--color-danger)">{d.before ?? '∅'}</td>
            <td className="text-(--color-success)">{d.after ?? '∅'}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function TransientView({ snap }: { snap: SnapshotInfo | undefined }) {
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
