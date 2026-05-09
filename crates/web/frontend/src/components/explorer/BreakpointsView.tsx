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
      <div className="flex flex-wrap h-auto min-h-9 items-center justify-between gap-2 border-b border-(--color-border) bg-(--color-bg) px-2 py-1">
        <span className="font-display text-[12px] font-semibold tracking-wide text-(--color-fg-secondary) uppercase">
          {breakpoints.length} breakpoint{breakpoints.length === 1 ? '' : 's'}
        </span>
        <div className="flex items-center gap-1">
          <ToolbarButton
            icon={Eye}
            label="Enable all"
            showLabel
            testid="bp-enable-all"
            onClick={enableAll}
            disabled={breakpoints.length === 0}
          />
          <ToolbarButton
            icon={EyeOff}
            label="Disable all"
            showLabel
            testid="bp-disable-all"
            onClick={disableAll}
            disabled={breakpoints.length === 0}
          />
          <ToolbarDivider />
          <ToolbarButton
            icon={Trash2}
            label="Clear all"
            showLabel
            testid="bp-clear-all"
            onClick={clear}
            disabled={breakpoints.length === 0}
          />
        </div>
      </div>
      {breakpoints.length === 0 ? (
        <div
          data-testid="breakpoints-empty"
          className="flex flex-col gap-1 px-3 py-3 text-xs italic text-(--color-fg-tertiary)"
        >
          <span>No breakpoints set.</span>
          <span>Click a line gutter in the editor to add one.</span>
          <span className="not-italic">
            Or use{' '}
            <code className="rounded bg-(--color-bg-elevated) px-1 font-mono text-[11px]">
              break addr:line
            </code>{' '}
            in the terminal.
          </span>
        </div>
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
        'flex flex-col gap-1.5 border-b border-(--color-border) px-2 py-2 text-[13px] hover:bg-(--color-bg-hover) ' +
        (enabled ? '' : 'opacity-60')
      }
    >
      <div className="flex items-center gap-1.5">
        <button
          type="button"
          title={enabled ? 'Toggle off (mute this breakpoint)' : 'Toggle on (re-enable this breakpoint)'}
          aria-label={enabled ? `Disable breakpoint ${idx + 1}` : `Enable breakpoint ${idx + 1}`}
          aria-pressed={!enabled}
          data-testid={`bp-toggle-${idx}`}
          onClick={onToggleEnabled}
          className="inline-flex h-6 items-center gap-1 rounded px-1.5 text-[11px] text-(--color-fg-tertiary) hover:bg-(--color-bg-hover) hover:text-(--color-fg)"
        >
          {enabled ? <Eye size={14} aria-hidden /> : <EyeOff size={14} aria-hidden />}
          <span>{enabled ? 'On' : 'Off'}</span>
        </button>
        <button
          type="button"
          title="Jump to first snapshot that hits this breakpoint"
          aria-label={`Jump to first hit of breakpoint ${idx + 1}`}
          onClick={onJump}
          data-testid={`bp-jump-${idx}`}
          className="flex-1 truncate text-left font-mono text-(--color-fg-secondary) hover:text-(--color-fg)"
        >
          {describe(bp)}
        </button>
        <button
          type="button"
          title="Remove this breakpoint"
          aria-label={`Remove breakpoint ${idx + 1}`}
          data-testid={`bp-remove-${idx}`}
          onClick={onRemove}
          className="inline-flex h-6 items-center gap-1 rounded px-1.5 text-[11px] text-(--color-fg-tertiary) hover:bg-(--color-bg-hover) hover:text-(--color-danger)"
        >
          <X size={14} aria-hidden /> <span>Remove</span>
        </button>
      </div>
      <div
        className="flex items-center gap-1.5 pl-1"
        title="Optional condition — the breakpoint only fires when this Solidity expression evaluates to true at the snapshot."
      >
        <CircleSlash2 size={12} className="text-(--color-fg-tertiary)" aria-hidden />
        <span className="text-[11px] text-(--color-fg-tertiary)">if</span>
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
          className="flex-1 rounded border border-(--color-border) bg-(--color-bg) px-1.5 py-0.5 font-mono text-[12px] outline-none focus:border-(--color-accent)"
        />
      </div>
    </li>
  );
}
