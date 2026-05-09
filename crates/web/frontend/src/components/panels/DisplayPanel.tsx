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
import { WatchView } from './display/WatchView';
import { StackView } from './display/StackView';
import { MemoryView } from './display/MemoryView';
import { StorageView } from './display/StorageView';
import { TransientView } from './display/TransientView';
import { CalldataView } from './display/CalldataView';
import { OutputView } from './display/OutputView';
import { formatMemory } from './display/formatMemory';

type Tab =
  | 'vars'
  | 'watch'
  | 'stack'
  | 'memory'
  | 'storage'
  | 'transient'
  | 'calldata'
  | 'output';
const TABS: { key: Tab; label: string }[] = [
  { key: 'vars', label: 'Variables' },
  { key: 'watch', label: 'Watch' },
  { key: 'stack', label: 'Stack' },
  { key: 'memory', label: 'Memory' },
  { key: 'storage', label: 'Storage' },
  { key: 'transient', label: 'Transient' },
  { key: 'calldata', label: 'Calldata' },
  { key: 'output', label: 'Output' },
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
    else if (tab === 'calldata') text = opcode?.calldata ?? '';
    else if (tab === 'output') {
      // Best-effort: pull from the rendered DOM since the trace entry is
      // resolved inside <OutputView />. If the user copies "output" without
      // the panel being visible, this falls back to empty.
      const el = document.querySelector('[data-testid="output-full"]') as HTMLElement | null;
      text = el?.innerText ?? '';
    }
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
          showLabel
          testid="display-refresh"
          onClick={refreshActive}
        />
        <ToolbarButton
          icon={Copy}
          label="Copy"
          showLabel
          testid="display-copy"
          onClick={copyActive}
        />
        <ToolbarDivider />
        <span className="font-display text-[12px] text-(--color-fg-tertiary)">
          snapshot <span className="font-mono">{id + 1}</span>
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
        {tab === 'watch' ? (
          <WatchView snapshotId={id} />
        ) : !data ? (
          <span className="text-(--color-fg-tertiary)">Loading…</span>
        ) : (
          {
            vars: <VarsView snap={snap} />,
            stack: <StackView snap={snap} />,
            memory: <MemoryView snap={snap} />,
            storage: <StorageView id={id} />,
            transient: <TransientView snap={snap} />,
            calldata: <CalldataView snap={snap} />,
            output: <OutputView snap={snap} />,
          }[tab as Exclude<Tab, 'watch'>]
        )}
      </div>
    </div>
  );
}
