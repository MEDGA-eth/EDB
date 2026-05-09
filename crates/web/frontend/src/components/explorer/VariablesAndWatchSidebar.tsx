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
      {/* Single overflow-auto root so the whole pane scrolls as one
          column. Previously each section managed its own scroll, which
          meant a tall Variables list pushed Watch off-screen with no way
          to reach the watch input until you collapsed the locals. */}
      <div
        className="flex h-full flex-col gap-1 overflow-auto py-2"
        data-testid="vars-watch-sidebar"
      >
        <SectionHeader>Variables</SectionHeader>
        <div className="border-b border-(--color-border) px-3 pb-3">
          <VarsView snap={snapQ.data} />
        </div>
        <SectionHeader>Watch</SectionHeader>
        <div className="px-3 pb-3">
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
