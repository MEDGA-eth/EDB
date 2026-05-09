import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { QueryClient } from '@tanstack/react-query';
import { runTermCommand } from './termCommands';
import { useSession } from '../store/session';

function freshCtx() {
  return { queryClient: new QueryClient(), snapshotCount: 100 };
}

describe('runTermCommand', () => {
  beforeEach(() => {
    useSession.setState({
      currentSnapshotId: 0,
      breakpoints: [],
      terminalHistory: [],
    });
  });
  afterEach(() => {
    useSession.setState({ breakpoints: [], terminalHistory: [] });
  });

  test('unknown verbs fall through (handled=false)', () => {
    expect(runTermCommand('1 + 1', freshCtx()).handled).toBe(false);
    expect(runTermCommand('balanceOf(msg.sender)', freshCtx()).handled).toBe(false);
  });

  test('empty input is not handled', () => {
    expect(runTermCommand('   ', freshCtx()).handled).toBe(false);
  });

  test('goto sets snapshot id and clamps', () => {
    runTermCommand('goto 7', freshCtx());
    expect(useSession.getState().currentSnapshotId).toBe(7);
    const r = runTermCommand('goto 9999', freshCtx());
    expect(useSession.getState().currentSnapshotId).toBe(99); // snapshotCount=100 → clamp to 99
    expect(r.message).toContain('clamped');
  });

  test('goto with non-numeric prints usage', () => {
    const r = runTermCommand('goto foo', freshCtx());
    expect(r.handled).toBe(true);
    expect(r.message).toContain('usage');
  });

  test('break <addr>:<line> adds a Source breakpoint', () => {
    const r = runTermCommand('break 0x1111111111111111111111111111111111111111:42', freshCtx());
    expect(r.handled).toBe(true);
    const bps = useSession.getState().breakpoints;
    expect(bps).toHaveLength(1);
    expect(bps[0].loc?.kind).toBe('Source');
    if (bps[0].loc?.kind === 'Source') {
      expect(bps[0].loc.bytecode_address).toBe('0x1111111111111111111111111111111111111111');
      expect(bps[0].loc.line_number).toBe(42);
    }
  });

  test('break <addr>:pc=<n> adds an Opcode breakpoint', () => {
    const r = runTermCommand('break 0x2222222222222222222222222222222222222222:pc=128', freshCtx());
    expect(r.handled).toBe(true);
    const bp = useSession.getState().breakpoints[0];
    expect(bp.loc?.kind).toBe('Opcode');
    if (bp.loc?.kind === 'Opcode') {
      expect(bp.loc.pc).toBe(128);
    }
  });

  test('break with bad spec prints usage', () => {
    const r = runTermCommand('break nonsense', freshCtx());
    expect(r.message).toContain('usage');
    expect(useSession.getState().breakpoints).toHaveLength(0);
  });

  test('bp lists configured breakpoints', () => {
    runTermCommand('break 0x3333333333333333333333333333333333333333:7', freshCtx());
    const r = runTermCommand('bp', freshCtx());
    expect(r.handled).toBe(true);
    expect(r.message).toContain('#0');
    expect(r.message).toContain('7');
  });

  test('bp on empty list says no breakpoints', () => {
    const r = runTermCommand('bp', freshCtx());
    expect(r.message).toContain('no breakpoints');
  });

  test('unbreak removes by index', () => {
    runTermCommand('break 0x4444444444444444444444444444444444444444:1', freshCtx());
    runTermCommand('break 0x5555555555555555555555555555555555555555:2', freshCtx());
    const r = runTermCommand('unbreak 0', freshCtx());
    expect(r.handled).toBe(true);
    expect(useSession.getState().breakpoints).toHaveLength(1);
  });

  test('unbreak out of range surfaces a friendly error', () => {
    const r = runTermCommand('unbreak 9', freshCtx());
    expect(r.message).toContain('no breakpoint');
  });

  test('clear empties terminal history', () => {
    useSession.getState().appendTerminal({ kind: 'message', ts: 0, text: 'x' });
    expect(useSession.getState().terminalHistory).toHaveLength(1);
    runTermCommand('clear', freshCtx());
    expect(useSession.getState().terminalHistory).toHaveLength(0);
  });

  test('aliases: s/n/o/c map to nav.* via commands.ts', () => {
    expect(runTermCommand('s', freshCtx()).handled).toBe(true);
    expect(runTermCommand('n', freshCtx()).handled).toBe(true);
    expect(runTermCommand('o', freshCtx()).handled).toBe(true);
    expect(runTermCommand('c', freshCtx()).handled).toBe(true);
  });

  test('help returns markdown and is handled', () => {
    const r = runTermCommand('help', freshCtx());
    expect(r.handled).toBe(true);
    expect(r.message).toContain('Built-in commands');
  });
});
