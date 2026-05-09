import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Copy, RefreshCw } from 'lucide-react';
import { useSession } from '../../store/session';
import { useSnapshotInfo } from '../../hooks/useSnapshotInfo';
import { useStorageDiff } from '../../hooks/useStorageDiff';
import { ErrorBoundary } from '../ErrorBoundary';
import { ErrorCard } from '../ErrorCard';
import { Toolbar, ToolbarButton, ToolbarDivider } from '../Toolbar';
import type { SnapshotInfo } from '../../lib/types';
import { storageDiffRows } from '../../lib/types';
import { VarsView } from './display/VarsView';
import { StackView } from './display/StackView';
import { MemoryView } from './display/MemoryView';
import { StorageView } from './display/StorageView';
import { TransientView } from './display/TransientView';
import { formatMemory } from './display/formatMemory';

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
  const { data: storageData } = useStorageDiff(id);
  const [tab, setTab] = useState<Tab>('vars');
  const qc = useQueryClient();

  if (error)
    return (
      <ErrorCard
        message={(error as Error).message}
        cause={error}
        onRetry={() => refetch()}
      />
    );

  const snap: SnapshotInfo | undefined = data;
  const opcode = snap?.detail?.kind === 'Opcode' ? snap.detail : undefined;

  function copyActive() {
    let text = '';
    if (tab === 'vars') text = JSON.stringify(data, null, 2);
    else if (tab === 'stack') text = (opcode?.stack ?? []).join('\n');
    else if (tab === 'memory') text = formatMemory(opcode?.memory ?? []);
    else if (tab === 'storage')
      text = storageDiffRows(storageData ?? {})
        .map((d) => `${d.slot}\t${d.before}\t${d.after}`)
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
            className={`px-3 py-1 text-sm border-b-2 -mb-px ${
              tab === key
                ? 'border-(--color-accent) text-(--color-accent) font-semibold'
                : 'border-transparent text-(--color-fg-secondary)'
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
