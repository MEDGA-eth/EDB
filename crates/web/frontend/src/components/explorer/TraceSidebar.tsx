import { TracePanel } from '../panels/TracePanel';

/**
 * Sidebar adapter for the call-trace tree. Renders the same UI as the
 * dockview `TracePanel` but is mounted in the IDE sidebar (the activity-bar
 * "trace" slot) so it stays visible alongside the editor and Display panel.
 *
 * Kept as a thin wrapper so {@link TracePanel} can still be referenced by
 * persisted dockview layouts that pre-date the sidebar move (the layout
 * version was bumped to drop those, but the component registration stays
 * for safety).
 */
export function TraceSidebar() {
  return (
    <div data-testid="trace-sidebar" className="flex h-full flex-col">
      <TracePanel />
    </div>
  );
}
