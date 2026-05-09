import { useSession } from '../../store/session';
import { useSnapshotInfo } from '../../hooks/useSnapshotInfo';
import { ErrorBoundary } from '../ErrorBoundary';
import { VarsView } from '../panels/display/VarsView';
import { WatchView } from '../panels/display/WatchView';

/**
 * Sidebar variant that stacks the Variables and Watch views in one
 * scroll column. Same building blocks as the Display panel's tabs, but
 * always-visible side-by-side so the user can keep both in view while
 * stepping through the trace.
 */
export function VariablesAndWatchSidebar() {
  const id = useSession((s) => s.currentSnapshotId);
  const snapQ = useSnapshotInfo(id);
  return (
    <ErrorBoundary label="VariablesAndWatchSidebar">
      <div className="flex h-full flex-col gap-1 py-2" data-testid="vars-watch-sidebar">
        <SectionHeader>Variables</SectionHeader>
        <div className="px-3 pb-2 border-b border-(--color-border)">
          <VarsView snap={snapQ.data} />
        </div>
        <SectionHeader>Watch</SectionHeader>
        <div className="flex-1 overflow-auto px-3 pb-2">
          <WatchView snapshotId={id} />
        </div>
      </div>
    </ErrorBoundary>
  );
}

function SectionHeader({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-6 items-center px-3 font-display text-[11px] font-semibold tracking-wide text-(--color-fg-tertiary) uppercase">
      {children}
    </div>
  );
}
