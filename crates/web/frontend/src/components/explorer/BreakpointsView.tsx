import { useQueryClient } from '@tanstack/react-query';
import { Trash2 } from 'lucide-react';
import { z } from 'zod';
import { rpc } from '../../lib/rpc';
import { useSession } from '../../store/session';
import { ToolbarButton } from '../Toolbar';
import { ErrorBoundary } from '../ErrorBoundary';
import type { Breakpoint } from '../../lib/types';

const HitsSchema = z.array(z.number().int().nonnegative());

function describe(bp: Breakpoint): string {
  if (!bp.loc) return bp.condition ? `(condition) ${bp.condition}` : '(empty)';
  if (bp.loc.kind === 'Opcode') return `${bp.loc.bytecode_address.slice(0, 10)}… @ pc=${bp.loc.pc}`;
  return `${bp.loc.bytecode_address.slice(0, 10)}… ${bp.loc.file_path.split('/').pop() ?? bp.loc.file_path}:${bp.loc.line_number}`;
}

export function BreakpointsView() {
  return (
    <ErrorBoundary label="BreakpointsView">
      <BreakpointsViewInner />
    </ErrorBoundary>
  );
}

function BreakpointsViewInner() {
  const breakpoints = useSession((s) => s.breakpoints);
  const remove = useSession((s) => s.removeBreakpoint);
  const clear = useSession((s) => s.clearBreakpoints);
  const setSnap = useSession((s) => s.setSnapshotId);
  const qc = useQueryClient();

  async function jumpToFirstHit(idx: number) {
    const bp = breakpoints[idx];
    if (!bp) return;
    const hits = await qc.fetchQuery({
      queryKey: ['bp-hits', JSON.stringify(bp, Object.keys(bp).sort())] as const,
      queryFn: () => rpc('edb_getBreakpointHits', HitsSchema, [bp]),
    });
    if (hits.length > 0) setSnap(hits[0]);
  }

  return (
    <div data-testid="breakpoints-view" className="flex h-full flex-col">
      <div className="flex h-7 items-center justify-between gap-2 border-b border-(--color-border) bg-(--color-bg) px-2">
        <span className="font-display text-[11px] font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
          {breakpoints.length} breakpoint{breakpoints.length === 1 ? '' : 's'}
        </span>
        <ToolbarButton
          icon={Trash2}
          label="Clear all breakpoints"
          testid="bp-clear-all"
          onClick={clear}
          disabled={breakpoints.length === 0}
        />
      </div>
      {breakpoints.length === 0 ? (
        <p
          data-testid="breakpoints-empty"
          className="px-3 py-3 text-xs text-(--color-fg-tertiary)"
        >
          No breakpoints. Use the editor toolbar or `break addr:line` in the terminal.
        </p>
      ) : (
        <ul className="flex-1 overflow-auto">
          {breakpoints.map((bp, i) => (
            <li
              key={i}
              data-testid={`bp-row-${i}`}
              className="flex items-center gap-1 border-b border-(--color-border) px-2 py-1.5 text-xs hover:bg-(--color-bg-hover)"
            >
              <button
                type="button"
                title="Jump to first hit"
                onClick={() => void jumpToFirstHit(i)}
                data-testid={`bp-jump-${i}`}
                className="flex-1 truncate text-left font-mono text-(--color-fg-secondary) hover:text-(--color-fg)"
              >
                {describe(bp)}
              </button>
              <ToolbarButton
                icon={Trash2}
                label="Remove breakpoint"
                testid={`bp-remove-${i}`}
                onClick={() => remove(i)}
              />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
