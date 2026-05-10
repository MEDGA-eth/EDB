import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { QueryClient } from '@tanstack/react-query';
import { COMMANDS, getCommand, type CommandCtx } from './commands';
import { useSession } from '../store/session';

function ctx(overrides: Partial<CommandCtx> = {}): CommandCtx {
  return {
    queryClient: new QueryClient(),
    snapshotCount: 5,
    ...overrides,
  };
}

describe('command registry', () => {
  beforeEach(() => {
    useSession.setState({
      currentSnapshotId: 0,
      breakpoints: [],
      terminalHistory: [],
      paletteOpen: false,
      activeActivity: 'explorer',
      theme: 'light',
      wordWrap: false,
      showLineNumbers: true,
      traceExpandTick: 0,
      traceCollapseTick: 0,
      watchExpressions: [],
      traceCallFilters: [
        'CALL',
        'STATICCALL',
        'DELEGATECALL',
        'CALLCODE',
        'CREATE',
        'CREATE2',
      ],
    });
  });
  afterEach(() => useSession.setState({ paletteOpen: false }));

  test('every command has a unique id', () => {
    const ids = COMMANDS.map((c) => c.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  test('nav.next bumps the snapshot', () => {
    getCommand('nav.next')!.run(ctx());
    expect(useSession.getState().currentSnapshotId).toBe(1);
  });

  test('nav.next disabled at the last snapshot', () => {
    useSession.setState({ currentSnapshotId: 4 });
    expect(getCommand('nav.next')!.enabled?.(ctx({ snapshotCount: 5 }))).toBe(false);
    useSession.setState({ currentSnapshotId: 3 });
    expect(getCommand('nav.next')!.enabled?.(ctx({ snapshotCount: 5 }))).toBe(true);
  });

  test('nav.prev disabled at snapshot 0', () => {
    useSession.setState({ currentSnapshotId: 0 });
    expect(getCommand('nav.prev')!.enabled?.(ctx())).toBe(false);
    useSession.setState({ currentSnapshotId: 1 });
    expect(getCommand('nav.prev')!.enabled?.(ctx())).toBe(true);
  });

  test('nav.last clamps to count-1', () => {
    getCommand('nav.last')!.run(ctx({ snapshotCount: 7 }));
    expect(useSession.getState().currentSnapshotId).toBe(6);
  });

  test('nav.last disabled when already at last', () => {
    useSession.setState({ currentSnapshotId: 6 });
    expect(getCommand('nav.last')!.enabled?.(ctx({ snapshotCount: 7 }))).toBe(false);
  });

  test('nav.first goes to 0', () => {
    useSession.setState({ currentSnapshotId: 4 });
    getCommand('nav.first')!.run(ctx());
    expect(useSession.getState().currentSnapshotId).toBe(0);
  });

  test('nav.first disabled when already at 0 or count is 0', () => {
    useSession.setState({ currentSnapshotId: 0 });
    expect(getCommand('nav.first')!.enabled?.(ctx({ snapshotCount: 5 }))).toBe(false);
    expect(getCommand('nav.first')!.enabled?.(ctx({ snapshotCount: 0 }))).toBe(false);
    useSession.setState({ currentSnapshotId: 2 });
    expect(getCommand('nav.first')!.enabled?.(ctx({ snapshotCount: 5 }))).toBe(true);
  });

  test('nav.next-call disabled without nextCallId', () => {
    const cmd = getCommand('nav.next-call')!;
    expect(cmd.enabled?.(ctx())).toBe(false);
    expect(cmd.enabled?.(ctx({ nextCallId: 3 }))).toBe(true);
    cmd.run(ctx({ nextCallId: 3 }));
    expect(useSession.getState().currentSnapshotId).toBe(3);
  });

  test('nav.next-call disabled when target equals current', () => {
    useSession.setState({ currentSnapshotId: 3 });
    expect(getCommand('nav.next-call')!.enabled?.(ctx({ nextCallId: 3 }))).toBe(false);
    expect(getCommand('nav.next-call')!.enabled?.(ctx({ nextCallId: 4 }))).toBe(true);
  });

  test('view.toggle-theme flips theme', () => {
    expect(useSession.getState().theme).toBe('light');
    getCommand('view.toggle-theme')!.run(ctx());
    expect(useSession.getState().theme).toBe('dark');
  });

  test('view.toggle-wrap and toggle-line-numbers flip flags', () => {
    getCommand('view.toggle-wrap')!.run(ctx());
    expect(useSession.getState().wordWrap).toBe(true);
    getCommand('view.toggle-line-numbers')!.run(ctx());
    expect(useSession.getState().showLineNumbers).toBe(false);
  });

  test('trace.expand-all bumps the expand tick', () => {
    getCommand('trace.expand-all')!.run(ctx());
    expect(useSession.getState().traceExpandTick).toBe(1);
  });

  test('terminal.clear empties terminal history', () => {
    useSession.getState().appendTerminal({ kind: 'input', ts: 0, text: 'foo' });
    getCommand('terminal.clear')!.run(ctx());
    expect(useSession.getState().terminalHistory).toHaveLength(0);
  });

  test('breakpoints.clear-all is gated on having breakpoints', () => {
    const cmd = getCommand('breakpoints.clear-all')!;
    expect(cmd.enabled?.(ctx())).toBe(false);
    useSession
      .getState()
      .addBreakpoint({ loc: { kind: 'Opcode', bytecode_address: '0x' + '0'.repeat(40), pc: 1 }, condition: null });
    expect(cmd.enabled?.(ctx())).toBe(true);
    cmd.run(ctx());
    expect(useSession.getState().breakpoints).toHaveLength(0);
  });

  test('layout.refresh-active invalidates snapshot queries', () => {
    const qc = new QueryClient();
    qc.setQueryData(['snapshot', 0], 'cached');
    getCommand('layout.refresh-active')!.run(ctx({ queryClient: qc }));
    const state = qc.getQueryState(['snapshot', 0]);
    expect(state?.isInvalidated).toBe(true);
  });

  test('nav.step-over jumps to current.next_id when cached', () => {
    const qc = new QueryClient();
    qc.setQueryData(['snapshot', 0], { id: 0, frame_id: [0, 0], next_id: 7, prev_id: 0 });
    getCommand('nav.step-over')!.run(ctx({ queryClient: qc }));
    expect(useSession.getState().currentSnapshotId).toBe(7);
  });

  test('nav.step-over falls back to currentId+1 when cache misses', () => {
    const qc = new QueryClient();
    // No cached snapshot. step-over should fallback to currentId+1.
    useSession.setState({ currentSnapshotId: 2 });
    // enabled() requires cached neighbour; bypass by calling run() directly,
    // simulating a hotkey press that fires regardless.
    getCommand('nav.step-over')!.run(ctx({ queryClient: qc }));
    expect(useSession.getState().currentSnapshotId).toBe(3);
  });

  test('nav.step-over disabled at end of trace (next_id === id)', () => {
    const qc = new QueryClient();
    qc.setQueryData(['snapshot', 4], { id: 4, frame_id: [0, 0], next_id: 4, prev_id: 3 });
    useSession.setState({ currentSnapshotId: 4 });
    expect(getCommand('nav.step-over')!.enabled?.(ctx({ queryClient: qc }))).toBe(false);
  });

  test('nav.go-back pops the navigation history', () => {
    const qc = new QueryClient();
    // Simulate two prior navigations: 0 → 5, 5 → 7. History stack should be [0, 5].
    useSession.setState({ currentSnapshotId: 0, navHistory: [] });
    useSession.getState().setSnapshotId(5);
    useSession.getState().setSnapshotId(7);
    expect(useSession.getState().navHistory).toEqual([0, 5]);
    getCommand('nav.go-back')!.run(ctx({ queryClient: qc }));
    expect(useSession.getState().currentSnapshotId).toBe(5);
    expect(useSession.getState().navHistory).toEqual([0]);
    getCommand('nav.go-back')!.run(ctx({ queryClient: qc }));
    expect(useSession.getState().currentSnapshotId).toBe(0);
    expect(useSession.getState().navHistory).toEqual([]);
  });

  test('nav.go-back disabled when navHistory is empty', () => {
    const qc = new QueryClient();
    useSession.setState({ currentSnapshotId: 5, navHistory: [] });
    expect(getCommand('nav.go-back')!.enabled?.(ctx({ queryClient: qc }))).toBe(false);
  });

  test('nav.step-out walks next_id until frame_id[0] differs', () => {
    const qc = new QueryClient();
    // 0 → 1 → 2 (still frame 0) → 3 (frame 1)
    qc.setQueryData(['snapshot', 0], { id: 0, frame_id: [0, 0], next_id: 1, prev_id: 0 });
    qc.setQueryData(['snapshot', 1], { id: 1, frame_id: [0, 0], next_id: 2, prev_id: 0 });
    qc.setQueryData(['snapshot', 2], { id: 2, frame_id: [0, 0], next_id: 3, prev_id: 1 });
    qc.setQueryData(['snapshot', 3], { id: 3, frame_id: [1, 0], next_id: 4, prev_id: 2 });
    useSession.setState({ currentSnapshotId: 0 });
    getCommand('nav.step-out')!.run(ctx({ queryClient: qc }));
    expect(useSession.getState().currentSnapshotId).toBe(3);
  });

  test('nav.step-out is a no-op when chain runs out without frame change', () => {
    const qc = new QueryClient();
    qc.setQueryData(['snapshot', 0], { id: 0, frame_id: [0, 0], next_id: 1, prev_id: 0 });
    qc.setQueryData(['snapshot', 1], { id: 1, frame_id: [0, 0], next_id: 1, prev_id: 0 });
    useSession.setState({ currentSnapshotId: 0 });
    getCommand('nav.step-out')!.run(ctx({ queryClient: qc }));
    expect(useSession.getState().currentSnapshotId).toBe(0);
  });

  test('nav.continue jumps to next breakpoint hit ahead of currentId', async () => {
    const qc = new QueryClient();
    const addr = '0x' + '0'.repeat(40);
    useSession
      .getState()
      .addBreakpoint({ loc: { kind: 'Opcode', bytecode_address: addr, pc: 1 }, condition: null });
    // Pre-populate the bp-hits cache so fetchQuery returns synchronously
    // without an actual network call (no fetch mock in scope).
    const bp = useSession.getState().breakpoints[0];
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const { enabled: _omit, ...rest } = bp;
    qc.setQueryData(['bp-hits', JSON.stringify(rest, Object.keys(rest).sort())], [3, 7, 12]);
    useSession.setState({ currentSnapshotId: 5 });
    await getCommand('nav.continue')!.run(ctx({ queryClient: qc, snapshotCount: 20 }));
    expect(useSession.getState().currentSnapshotId).toBe(7);
  });

  test('nav.continue jumps to last when no breakpoint hits ahead', async () => {
    const qc = new QueryClient();
    useSession.setState({ currentSnapshotId: 3 });
    await getCommand('nav.continue')!.run(ctx({ queryClient: qc, snapshotCount: 8 }));
    expect(useSession.getState().currentSnapshotId).toBe(7);
  });

  test('nav.reverse-continue jumps to most recent prior hit', async () => {
    const qc = new QueryClient();
    const addr = '0x' + '0'.repeat(40);
    useSession
      .getState()
      .addBreakpoint({ loc: { kind: 'Opcode', bytecode_address: addr, pc: 1 }, condition: null });
    const bp = useSession.getState().breakpoints[0];
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    const { enabled: _omit, ...rest } = bp;
    qc.setQueryData(['bp-hits', JSON.stringify(rest, Object.keys(rest).sort())], [2, 4, 9]);
    useSession.setState({ currentSnapshotId: 7 });
    await getCommand('nav.reverse-continue')!.run(ctx({ queryClient: qc, snapshotCount: 20 }));
    expect(useSession.getState().currentSnapshotId).toBe(4);
  });

  test('nav.reverse-continue falls back to 0 with no prior hits', async () => {
    const qc = new QueryClient();
    useSession.setState({ currentSnapshotId: 5 });
    await getCommand('nav.reverse-continue')!.run(ctx({ queryClient: qc, snapshotCount: 10 }));
    expect(useSession.getState().currentSnapshotId).toBe(0);
  });

  test('nav.reverse-step-out walks prev_id until frame_id[0] differs', () => {
    const qc = new QueryClient();
    qc.setQueryData(['snapshot', 3], { id: 3, frame_id: [1, 0], next_id: 4, prev_id: 2 });
    qc.setQueryData(['snapshot', 2], { id: 2, frame_id: [1, 0], next_id: 3, prev_id: 1 });
    qc.setQueryData(['snapshot', 1], { id: 1, frame_id: [0, 0], next_id: 2, prev_id: 0 });
    useSession.setState({ currentSnapshotId: 3 });
    getCommand('nav.reverse-step-out')!.run(ctx({ queryClient: qc }));
    expect(useSession.getState().currentSnapshotId).toBe(1);
  });
});
