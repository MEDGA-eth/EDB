import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { useSession } from './session';

describe('useSession', () => {
  beforeEach(() => {
    localStorage.clear();
    useSession.setState({
      currentSnapshotId: 0,
      breakpoints: [],
      terminalHistory: [],
      panelTab: 'code',
      theme: 'light',
      connection: 'connected',
      sessionEnded: false,
    });
  });
  afterEach(() => { localStorage.clear(); });

  test('next/prev snapshot clamps within [0, max-1]', () => {
    const s = useSession.getState();
    s.setSnapshotId(0);
    s.nextSnapshot(3);
    expect(useSession.getState().currentSnapshotId).toBe(1);
    s.nextSnapshot(3);
    s.nextSnapshot(3);
    s.nextSnapshot(3);
    expect(useSession.getState().currentSnapshotId).toBe(2);
    s.prevSnapshot(); s.prevSnapshot(); s.prevSnapshot();
    expect(useSession.getState().currentSnapshotId).toBe(0);
  });

  test('addBreakpoint and removeBreakpoint', () => {
    const s = useSession.getState();
    s.addBreakpoint({ loc: { kind: 'Opcode', bytecode_address: '0x' + '0'.repeat(40), pc: 5 }, condition: null });
    expect(useSession.getState().breakpoints).toHaveLength(1);
    s.removeBreakpoint(0);
    expect(useSession.getState().breakpoints).toHaveLength(0);
  });

  test('appendTerminal and clearTerminal', () => {
    const s = useSession.getState();
    s.appendTerminal({ kind: 'input', ts: 0, text: 'foo' });
    s.appendTerminal({ kind: 'result', ts: 1, expr: 'foo', value: 42 });
    expect(useSession.getState().terminalHistory).toHaveLength(2);
    s.clearTerminal();
    expect(useSession.getState().terminalHistory).toHaveLength(0);
  });

  test('setPanelTab updates tab', () => {
    useSession.getState().setPanelTab('terminal');
    expect(useSession.getState().panelTab).toBe('terminal');
  });

  test('setTheme + persistence happens via middleware', () => {
    useSession.getState().setTheme('dark');
    expect(useSession.getState().theme).toBe('dark');
    expect(localStorage.getItem('edb-web:session')).toBeTruthy();
  });

  test('setConnection / setSessionEnded toggle correctly', () => {
    const s = useSession.getState();
    s.setConnection('degraded');
    expect(useSession.getState().connection).toBe('degraded');
    s.setSessionEnded(true);
    expect(useSession.getState().sessionEnded).toBe(true);
  });
});
