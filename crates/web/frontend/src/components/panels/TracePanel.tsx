import { useTrace } from '../../hooks/useTrace';
import { useSession } from '../../store/session';
import { ErrorBoundary } from '../ErrorBoundary';
import { ErrorCard } from '../ErrorCard';

interface Entry { id: number; kind: string; code_address: string; target_address: string; children?: Entry[] }

export function TracePanel() {
  return (
    <ErrorBoundary label="TracePanel">
      <TracePanelInner />
    </ErrorBoundary>
  );
}

function TracePanelInner() {
  const { data, isLoading, error, refetch } = useTrace();
  if (isLoading) return <div className="p-4 text-(--color-fg-tertiary)">Loading trace…</div>;
  if (error) return <ErrorCard message={(error as Error).message} onRetry={() => refetch()} />;
  if (!data) return null;
  return (
    <div data-testid="trace-panel" className="h-full overflow-auto p-2 font-mono text-sm">
      {(data as Entry[]).map((e) => <TraceNode key={e.id} entry={e} depth={0} />)}
    </div>
  );
}

function TraceNode({ entry, depth }: { entry: Entry; depth: number }) {
  const setId = useSession((s) => s.setSnapshotId);
  return (
    <>
      <button type="button"
              data-testid={`trace-entry-${entry.id}`}
              onClick={() => setId(entry.id)}
              className="block w-full rounded px-2 py-0.5 text-left hover:bg-(--color-bg-hover)"
              style={{ paddingLeft: `${depth * 16 + 8}px` }}>
        <span className="text-(--color-syn-keyword)">[{entry.kind}]</span>{' '}
        <span className="text-(--color-fg-secondary)">→</span>{' '}
        <span className="text-(--color-syn-type)">{entry.target_address.slice(0, 10)}…</span>
      </button>
      {entry.children?.map((c) => <TraceNode key={c.id} entry={c} depth={depth + 1} />)}
    </>
  );
}
