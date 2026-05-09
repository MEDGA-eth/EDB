import { useQueryClient } from '@tanstack/react-query';
import {
  ChevronsRight,
  CornerDownRight,
  CornerUpRight,
  Pause,
  Play,
  RotateCcw,
  SkipBack,
  SkipForward,
  Square,
  StepForward,
  Undo2,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import type { CommandCtx } from '../lib/commands';
import { getCommand } from '../lib/commands';
import { useNextCall } from '../hooks/useNextCall';
import { usePrevCall } from '../hooks/usePrevCall';
import { useSnapshotCount } from '../hooks/useSnapshotCount';
import { useSession } from '../store/session';

interface ToolbarItem {
  id: string;
  label: string;
  shortcut: string;
  Icon: LucideIcon;
  /** override commands.ts when needed (e.g. restart resets snapshot id) */
  run?: (ctx: CommandCtx & { setSnapshot: (id: number) => void }) => void;
  /** show this group separator before this button */
  groupBreak?: boolean;
}

const ITEMS: ToolbarItem[] = [
  { id: 'nav.continue', label: 'Continue', shortcut: 'F5', Icon: Play },
  { id: 'nav.step-over', label: 'Step Over', shortcut: 'F10', Icon: StepForward },
  { id: 'nav.next', label: 'Step Into', shortcut: 'F11', Icon: CornerDownRight },
  { id: 'nav.step-out', label: 'Step Out', shortcut: '⇧F11', Icon: CornerUpRight },
  {
    id: 'nav.restart',
    label: 'Restart',
    shortcut: '⇧⌘F5',
    Icon: RotateCcw,
    groupBreak: true,
    run: ({ setSnapshot }) => setSnapshot(0),
  },
  // Without a live debugger session to halt, "Stop" parks at the last
  // snapshot — same intuition as "run to end". Wired in fire() since it
  // needs `snapshotCount` from the toolbar's render scope.
  { id: 'nav.stop', label: 'Stop', shortcut: '⇧F5', Icon: Square },
  {
    id: 'nav.reverse-continue',
    label: 'Reverse Continue',
    shortcut: '⌥F5',
    Icon: Undo2,
    groupBreak: true,
  },
  { id: 'nav.reverse-step-over', label: 'Reverse Step', shortcut: '⌥F10', Icon: ChevronsRight },
  { id: 'nav.prev-call', label: 'Prev Call', shortcut: '⌥←', Icon: SkipBack, groupBreak: true },
  { id: 'nav.next-call', label: 'Next Call', shortcut: '⌥→', Icon: SkipForward },
];

export function DebugToolbar() {
  const qc = useQueryClient();
  const snapshotCountQ = useSnapshotCount();
  const snapshotCount = snapshotCountQ.data ?? 0;
  const cur = useSession((s) => s.currentSnapshotId);
  const prevCallQ = usePrevCall(cur);
  const nextCallQ = useNextCall(cur);
  const setSnapshot = useSession((s) => s.setSnapshotId);
  const conn = useSession((s) => s.connection);

  const ctx: CommandCtx & { setSnapshot: (id: number) => void } = {
    queryClient: qc,
    snapshotCount,
    prevCallId: typeof prevCallQ.data === 'number' ? prevCallQ.data : undefined,
    nextCallId: typeof nextCallQ.data === 'number' ? nextCallQ.data : undefined,
    setSnapshot,
  };

  const disabled = conn === 'offline';

  function fire(item: ToolbarItem) {
    if (disabled) return;
    if (item.run) {
      item.run(ctx);
      return;
    }
    if (item.id === 'nav.stop') {
      setSnapshot(Math.max(0, snapshotCount - 1));
      return;
    }
    const cmd = getCommand(item.id);
    if (!cmd) return;
    if (cmd.enabled && !cmd.enabled(ctx)) return;
    cmd.run(ctx);
  }

  return (
    <div
      data-testid="debug-toolbar"
      role="toolbar"
      aria-label="Debug toolbar"
      className="flex items-center gap-1 border-b border-(--color-border) bg-(--color-bg-elevated) px-3 py-1.5"
    >
      <span className="font-display text-xs font-semibold tracking-wide text-(--color-fg-tertiary) uppercase mr-2">
        Debug
      </span>
      {ITEMS.map((item) => {
        const cmd = item.run ? null : getCommand(item.id);
        const enabled = item.run ? !disabled : cmd && (!cmd.enabled || cmd.enabled(ctx)) && !disabled;
        return (
          <span key={item.id} className="contents">
            {item.groupBreak && (
              <span aria-hidden className="mx-1 h-5 w-px bg-(--color-border)" />
            )}
            <button
              type="button"
              data-testid={`tb-${item.id}`}
              onClick={() => fire(item)}
              disabled={!enabled}
              title={`${item.label} (${item.shortcut})`}
              aria-label={`${item.label}, shortcut ${item.shortcut}`}
              className="inline-flex items-center gap-1.5 rounded px-2 py-1 text-sm text-(--color-fg-secondary) transition enabled:hover:bg-(--color-bg-hover) enabled:hover:text-(--color-fg) disabled:opacity-40"
            >
              <item.Icon size={16} aria-hidden />
              <span className="hidden md:inline">{item.label}</span>
              <kbd className="hidden md:inline rounded border border-(--color-border) bg-(--color-bg) px-1 text-[10px] text-(--color-fg-tertiary)">
                {item.shortcut}
              </kbd>
            </button>
          </span>
        );
      })}
      {disabled && (
        <span className="ml-auto inline-flex items-center gap-1 text-xs text-(--color-fg-tertiary)">
          <Pause size={12} aria-hidden /> disconnected
        </span>
      )}
    </div>
  );
}
