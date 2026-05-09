import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import {
  ChevronsRight,
  CornerDownRight,
  CornerUpRight,
  LayoutGrid,
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
import { ensureFixedPanel, getDockviewApi } from '../lib/dockviewBridge';

interface ToolbarItem {
  id: string;
  label: string;
  /** Verbose tooltip — explains what each Step variant actually does so
   *  users can pick the right one without consulting docs. */
  hint: string;
  shortcut: string;
  Icon: LucideIcon;
  /** override commands.ts when needed (e.g. restart resets snapshot id) */
  run?: (ctx: CommandCtx & { setSnapshot: (id: number) => void }) => void;
  /** show this group separator before this button */
  groupBreak?: boolean;
}

const ITEMS: ToolbarItem[] = [
  {
    id: 'nav.continue',
    label: 'Continue',
    hint: 'Run forward to the next breakpoint (or end of trace if none)',
    shortcut: 'F5',
    Icon: Play,
  },
  // Order: Step Into → Step Out → Step Over. Step Over is the least
  // stable of the trio at the moment (the engine occasionally skips
  // hooks across CALL boundaries it shouldn't), so it sits last to
  // discourage muscle-memory clicking.
  {
    id: 'nav.next',
    label: 'Step Into',
    hint: 'Advance exactly one snapshot; descend into the callee if next op is a CALL',
    shortcut: 'F11',
    Icon: CornerDownRight,
  },
  {
    id: 'nav.step-out',
    label: 'Step Out',
    hint: 'Run forward until the current frame returns — one call level shallower',
    shortcut: '⇧F11',
    Icon: CornerUpRight,
  },
  {
    id: 'nav.step-over',
    label: 'Step Over',
    hint: 'Advance one snapshot; skip the inside of any CALL / CREATE — same call depth (BETA)',
    shortcut: 'F10',
    Icon: StepForward,
  },
  {
    id: 'nav.restart',
    label: 'Restart',
    hint: 'Jump back to the very first snapshot (snapshot 0)',
    shortcut: '⇧⌘F5',
    Icon: RotateCcw,
    groupBreak: true,
    run: ({ setSnapshot }) => setSnapshot(0),
  },
  // Without a live debugger session to halt, "Stop" parks at the last
  // snapshot — same intuition as "run to end". Wired in fire() since it
  // needs `snapshotCount` from the toolbar's render scope.
  {
    id: 'nav.stop',
    label: 'Stop',
    hint: 'Park at the last snapshot in the trace (no live process to halt)',
    shortcut: '⇧F5',
    Icon: Square,
  },
  {
    id: 'nav.reverse-continue',
    label: 'Reverse Continue',
    hint: 'Run backwards to the previous breakpoint (or start of trace)',
    shortcut: '⌥F5',
    Icon: Undo2,
    groupBreak: true,
  },
  {
    id: 'nav.reverse-step-over',
    label: 'Reverse Step',
    hint: 'Step backwards — the inverse of Step Over',
    shortcut: '⌥F10',
    Icon: ChevronsRight,
  },
  {
    id: 'nav.prev-call',
    label: 'Prev Call',
    hint: 'Jump to the previous CALL / DELEGATECALL / CREATE frame',
    shortcut: '⌥←',
    Icon: SkipBack,
    groupBreak: true,
  },
  {
    id: 'nav.next-call',
    label: 'Next Call',
    hint: 'Jump to the next CALL / DELEGATECALL / CREATE frame',
    shortcut: '⌥→',
    Icon: SkipForward,
  },
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
              title={`${item.label} — ${item.hint} (${item.shortcut})`}
              aria-label={`${item.label}, shortcut ${item.shortcut}. ${item.hint}`}
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
      <span className="ml-auto inline-flex items-center gap-2">
        <ReopenMenu />
        {disabled && (
          <span className="inline-flex items-center gap-1 text-[12px] text-(--color-fg-tertiary)">
            <Pause size={14} aria-hidden /> disconnected
          </span>
        )}
      </span>
    </div>
  );
}

/**
 * "Reopen" menu — surfaces every fixed panel the user has closed so they
 * can recover from an accidental ⨯ click without remembering to open the
 * command palette. Reading the live dockview api on click keeps the list
 * accurate even after layout changes the user may have made.
 */
function ReopenMenu() {
  const [open, setOpen] = useState(false);
  const api = getDockviewApi();

  // Compute closed-panel list lazily on each render — cheap, and avoids
  // wiring up dockview event subscribers in this small component.
  const closed: Array<{ id: 'display' | 'terminal'; label: string }> = [];
  if (!api?.getPanel('display')) closed.push({ id: 'display', label: 'Display' });
  if (!api?.getPanel('terminal')) closed.push({ id: 'terminal', label: 'Terminal' });

  if (closed.length === 0) return null;

  return (
    <span className="relative inline-flex">
      <button
        type="button"
        data-testid="reopen-menu"
        onClick={() => setOpen((o) => !o)}
        title={`Reopen ${closed.length} closed panel${closed.length === 1 ? '' : 's'}`}
        className={
          'inline-flex items-center gap-1.5 rounded border-2 border-(--color-accent) ' +
          'bg-(--color-accent) px-3 py-1 text-[13px] font-semibold text-white ' +
          'shadow-[var(--shadow-sm)] hover:brightness-110 ' +
          'animate-[pulse_2.4s_ease-in-out_infinite]'
        }
      >
        <LayoutGrid size={16} aria-hidden />
        <span>Reopen panel{closed.length === 1 ? '' : 's'}</span>
        <span className="rounded-full bg-white/25 px-1.5 py-px font-mono text-[11px]">
          {closed.length}
        </span>
      </button>
      {open && (
        <div
          role="menu"
          // Click anywhere else: capture-phase mousedown closes via stopPropagation
          // pattern (we rely on the parent capturing). For simplicity, just
          // close on the next click anywhere via window listener.
          onMouseDown={(e) => e.stopPropagation()}
          className="absolute right-0 top-[calc(100%+4px)] z-50 min-w-[180px] rounded border border-(--color-border-strong) bg-(--color-bg-elevated) py-1 shadow-[var(--shadow-md)]"
        >
          {closed.map((c) => (
            <button
              key={c.id}
              role="menuitem"
              type="button"
              data-testid={`reopen-${c.id}`}
              onClick={() => {
                ensureFixedPanel(c.id, c.label);
                setOpen(false);
              }}
              className="block w-full px-3 py-1.5 text-left text-[13px] text-(--color-fg) hover:bg-(--color-bg-hover)"
            >
              {c.label} panel
            </button>
          ))}
        </div>
      )}
    </span>
  );
}
