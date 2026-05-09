import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { CircleSlash2, Eye, EyeOff, Trash2, X } from 'lucide-react';
import { z } from 'zod';
import { rpc } from '../../lib/rpc';
import { useSession } from '../../store/session';
import { ToolbarButton, ToolbarDivider } from '../Toolbar';
import { ErrorBoundary } from '../ErrorBoundary';
import { breakpointToWire, type Breakpoint } from '../../lib/types';

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
  const setEnabled = useSession((s) => s.setBreakpointEnabled);
  const setCondition = useSession((s) => s.setBreakpointCondition);
  const enableAll = useSession((s) => s.enableAllBreakpoints);
  const disableAll = useSession((s) => s.disableAllBreakpoints);
  const qc = useQueryClient();

  async function jumpToFirstHit(idx: number) {
    const bp = breakpoints[idx];
    if (!bp) return;
    const hits = await qc.fetchQuery({
      queryKey: ['bp-hits', JSON.stringify(bp, Object.keys(bp).sort())] as const,
      queryFn: () => rpc('edb_getBreakpointHits', HitsSchema, [breakpointToWire(bp)]),
    });
    if (hits.length > 0) setSnap(hits[0]);
  }

  return (
    <div data-testid="breakpoints-view" className="flex h-full flex-col">
      <div className="flex h-7 items-center justify-between gap-2 border-b border-(--color-border) bg-(--color-bg) px-2">
        <span className="font-display text-[11px] font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
          {breakpoints.length} breakpoint{breakpoints.length === 1 ? '' : 's'}
        </span>
        <div className="flex items-center gap-1">
          <ToolbarButton
            icon={Eye}
            label="Enable all breakpoints"
            testid="bp-enable-all"
            onClick={enableAll}
            disabled={breakpoints.length === 0}
          />
          <ToolbarButton
            icon={EyeOff}
            label="Disable all breakpoints"
            testid="bp-disable-all"
            onClick={disableAll}
            disabled={breakpoints.length === 0}
          />
          <ToolbarDivider />
          <ToolbarButton
            icon={Trash2}
            label="Clear all breakpoints"
            testid="bp-clear-all"
            onClick={clear}
            disabled={breakpoints.length === 0}
          />
        </div>
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
            <BreakpointRow
              key={i}
              idx={i}
              bp={bp}
              onJump={() => void jumpToFirstHit(i)}
              onRemove={() => remove(i)}
              onToggleEnabled={() => setEnabled(i, !(bp.enabled ?? true))}
              onConditionChange={(value) => setCondition(i, value)}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

function BreakpointRow({
  idx,
  bp,
  onJump,
  onRemove,
  onToggleEnabled,
  onConditionChange,
}: {
  idx: number;
  bp: Breakpoint;
  onJump(): void;
  onRemove(): void;
  onToggleEnabled(): void;
  onConditionChange(value: string | null): void;
}) {
  const enabled = bp.enabled ?? true;
  // local-input state so typing doesn't run through the store on every key,
  // but commits on blur/Enter to keep persistence churn-free.
  const [draft, setDraft] = useState<string>(bp.condition ?? '');
  useEffect(() => {
    setDraft(bp.condition ?? '');
  }, [bp.condition]);

  function commit() {
    const trimmed = draft.trim();
    onConditionChange(trimmed.length === 0 ? null : trimmed);
  }

  return (
    <li
      data-testid={`bp-row-${idx}`}
      data-enabled={enabled ? 'true' : 'false'}
      className={
        'flex flex-col gap-1 border-b border-(--color-border) px-2 py-1.5 text-xs hover:bg-(--color-bg-hover) ' +
        (enabled ? '' : 'opacity-60')
      }
    >
      <div className="flex items-center gap-1">
        <button
          type="button"
          title={enabled ? 'Disable breakpoint' : 'Enable breakpoint'}
          aria-label={enabled ? `Disable breakpoint ${idx + 1}` : `Enable breakpoint ${idx + 1}`}
          aria-pressed={!enabled}
          data-testid={`bp-toggle-${idx}`}
          onClick={onToggleEnabled}
          className="flex h-5 w-5 items-center justify-center text-(--color-fg-tertiary) hover:text-(--color-fg)"
        >
          {enabled ? <Eye size={12} aria-hidden /> : <EyeOff size={12} aria-hidden />}
        </button>
        <button
          type="button"
          title="Jump to first hit"
          aria-label={`Jump to first hit of breakpoint ${idx + 1}`}
          onClick={onJump}
          data-testid={`bp-jump-${idx}`}
          className="flex-1 truncate text-left font-mono text-(--color-fg-secondary) hover:text-(--color-fg)"
        >
          {describe(bp)}
        </button>
        <button
          type="button"
          title="Remove breakpoint"
          aria-label={`Remove breakpoint ${idx + 1}`}
          data-testid={`bp-remove-${idx}`}
          onClick={onRemove}
          className="flex h-5 w-5 items-center justify-center text-(--color-fg-tertiary) hover:text-(--color-danger)"
        >
          <X size={12} aria-hidden />
        </button>
      </div>
      <div className="flex items-center gap-1 pl-6">
        <CircleSlash2 size={10} className="text-(--color-fg-tertiary)" aria-hidden />
        <input
          data-testid={`bp-condition-${idx}`}
          aria-label={`Condition for breakpoint ${idx + 1}`}
          placeholder="condition (optional, e.g. x > 0)"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
          }}
          className="flex-1 rounded border border-(--color-border) bg-(--color-bg) px-1.5 py-0.5 font-mono text-[11px] outline-none focus:border-(--color-accent)"
        />
      </div>
    </li>
  );
}
