import { useState } from 'react';
import { useSession } from '../../store/session';
import { useSnapshotInfo } from '../../hooks/useSnapshotInfo';
import { useStorageDiff } from '../../hooks/useStorageDiff';
import { ErrorBoundary } from '../ErrorBoundary';
import { ErrorCard } from '../ErrorCard';

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

function DisplayPanelInner() {
  const id = useSession((s) => s.currentSnapshotId);
  const { data, error, refetch } = useSnapshotInfo(id);
  const [tab, setTab] = useState<Tab>('vars');

  if (error) return <ErrorCard message={(error as Error).message} onRetry={() => refetch()} />;

  return (
    <div className="flex h-full flex-col">
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
            vars: <VarsView snap={data} />,
            stack: <StackView snap={data} />,
            memory: <MemoryView snap={data} />,
            storage: <StorageView id={id} />,
            transient: <TransientView snap={data} />,
          }[tab]
        )}
      </div>
    </div>
  );
}

function VarsView({ snap }: { snap: unknown }) {
  // For Hook snapshots, the engine returns locals + state_variables; for Opcode snapshots, this is empty.
  const detail = (snap as { detail?: { kind?: string } }).detail;
  if (detail?.kind === 'Opcode') return <span>(no source variables in opcode mode)</span>;
  return <pre>{JSON.stringify(snap, null, 2)}</pre>;
}

function StackView({ snap }: { snap: unknown }) {
  const stack = (snap as { detail?: { stack?: string[] } }).detail?.stack ?? [];
  return (
    <ol reversed start={stack.length} className="list-decimal pl-6">
      {stack.map((v, i) => (
        <li key={i}>{v}</li>
      ))}
    </ol>
  );
}

function MemoryView({ snap }: { snap: unknown }) {
  const mem = (snap as { detail?: { memory?: number[] } }).detail?.memory ?? [];
  const rows: string[] = [];
  for (let i = 0; i < mem.length; i += 32) {
    const slice = mem
      .slice(i, i + 32)
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('');
    rows.push(`${i.toString(16).padStart(6, '0')}: ${slice}`);
  }
  return <pre>{rows.join('\n')}</pre>;
}

function StorageView({ id }: { id: number }) {
  const { data, error } = useStorageDiff(id);
  if (error) return <span>{(error as Error).message}</span>;
  if (!data) return <span>Loading…</span>;
  return (
    <table className="w-full">
      <tbody>
        {data.map((d, i) => (
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

function TransientView({ snap }: { snap: unknown }) {
  const t =
    (snap as { detail?: { transient_storage?: Record<string, string> } }).detail
      ?.transient_storage ?? {};
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
