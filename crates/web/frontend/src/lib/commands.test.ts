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

  test('nav.last clamps to count-1', () => {
    getCommand('nav.last')!.run(ctx({ snapshotCount: 7 }));
    expect(useSession.getState().currentSnapshotId).toBe(6);
  });

  test('nav.first goes to 0', () => {
    useSession.setState({ currentSnapshotId: 4 });
    getCommand('nav.first')!.run(ctx());
    expect(useSession.getState().currentSnapshotId).toBe(0);
  });

  test('nav.next-call disabled without nextCallId', () => {
    const cmd = getCommand('nav.next-call')!;
    expect(cmd.enabled?.(ctx())).toBe(false);
    expect(cmd.enabled?.(ctx({ nextCallId: 3 }))).toBe(true);
    cmd.run(ctx({ nextCallId: 3 }));
    expect(useSession.getState().currentSnapshotId).toBe(3);
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
});
