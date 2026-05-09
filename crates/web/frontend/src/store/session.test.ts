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
      activeActivity: 'explorer',
      openFiles: [],
      activeFileId: null,
      layoutJson: null,
      paletteOpen: false,
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

  test('setActivity changes activity', () => {
    useSession.getState().setActivity('trace');
    expect(useSession.getState().activeActivity).toBe('trace');
  });

  test('openFile adds new file and activates it; idempotent for same path', () => {
    const s = useSession.getState();
    const addr = '0x' + 'a'.repeat(40);
    s.openFile({ addr, path: 'foo.sol' });
    expect(useSession.getState().openFiles).toHaveLength(1);
    expect(useSession.getState().activeFileId).toBe(`${addr}::foo.sol`);

    s.openFile({ addr, path: 'foo.sol' });
    expect(useSession.getState().openFiles).toHaveLength(1);

    s.openFile({ addr, path: 'bar.sol' });
    expect(useSession.getState().openFiles).toHaveLength(2);
    expect(useSession.getState().activeFileId).toBe(`${addr}::bar.sol`);
  });

  test('closeFile removes file and reassigns active when active was closed', () => {
    const s = useSession.getState();
    const addr = '0x' + 'a'.repeat(40);
    s.openFile({ addr, path: 'foo.sol' });
    s.openFile({ addr, path: 'bar.sol' });
    s.closeFile(`${addr}::bar.sol`);
    expect(useSession.getState().openFiles).toHaveLength(1);
    expect(useSession.getState().activeFileId).toBe(`${addr}::foo.sol`);

    s.closeFile(`${addr}::foo.sol`);
    expect(useSession.getState().openFiles).toHaveLength(0);
    expect(useSession.getState().activeFileId).toBeNull();
  });

  test('setActiveFile changes active file id', () => {
    const s = useSession.getState();
    s.setActiveFile('foo');
    expect(useSession.getState().activeFileId).toBe('foo');
    s.setActiveFile(null);
    expect(useSession.getState().activeFileId).toBeNull();
  });

  test('setLayoutJson stores a layout snapshot', () => {
    useSession.getState().setLayoutJson('{"foo":1}');
    expect(useSession.getState().layoutJson).toBe('{"foo":1}');
  });

  test('togglePalette flips paletteOpen', () => {
    expect(useSession.getState().paletteOpen).toBe(false);
    useSession.getState().togglePalette();
    expect(useSession.getState().paletteOpen).toBe(true);
    useSession.getState().setPaletteOpen(false);
    expect(useSession.getState().paletteOpen).toBe(false);
  });

  test('toggleTheme flips theme', () => {
    expect(useSession.getState().theme).toBe('light');
    useSession.getState().toggleTheme();
    expect(useSession.getState().theme).toBe('dark');
    useSession.getState().toggleTheme();
    expect(useSession.getState().theme).toBe('light');
  });

  test('toggleWordWrap and toggleLineNumbers flip their flags', () => {
    expect(useSession.getState().wordWrap).toBe(false);
    useSession.getState().toggleWordWrap();
    expect(useSession.getState().wordWrap).toBe(true);
    expect(useSession.getState().showLineNumbers).toBe(true);
    useSession.getState().toggleLineNumbers();
    expect(useSession.getState().showLineNumbers).toBe(false);
  });

  test('bumpTraceExpand / bumpTraceCollapse increment counters', () => {
    expect(useSession.getState().traceExpandTick).toBe(0);
    useSession.getState().bumpTraceExpand();
    useSession.getState().bumpTraceExpand();
    expect(useSession.getState().traceExpandTick).toBe(2);
    useSession.getState().bumpTraceCollapse();
    expect(useSession.getState().traceCollapseTick).toBe(1);
  });

  test('setBreakpointCondition updates the condition for a single bp', () => {
    const s = useSession.getState();
    const addr = '0x' + '0'.repeat(40);
    s.addBreakpoint({ loc: { kind: 'Opcode', bytecode_address: addr, pc: 1 }, condition: null });
    s.setBreakpointCondition(0, 'x > 0');
    expect(useSession.getState().breakpoints[0].condition).toBe('x > 0');
    s.setBreakpointCondition(0, null);
    expect(useSession.getState().breakpoints[0].condition).toBeNull();
  });

  test('setBreakpointEnabled / enableAll / disableAll toggle enabled flag', () => {
    const s = useSession.getState();
    const addr = '0x' + '0'.repeat(40);
    s.addBreakpoint({ loc: { kind: 'Opcode', bytecode_address: addr, pc: 1 }, condition: null });
    s.addBreakpoint({ loc: { kind: 'Opcode', bytecode_address: addr, pc: 2 }, condition: null });
    expect(useSession.getState().breakpoints[0].enabled).toBe(true);
    s.setBreakpointEnabled(0, false);
    expect(useSession.getState().breakpoints[0].enabled).toBe(false);
    s.disableAllBreakpoints();
    expect(useSession.getState().breakpoints.every((bp) => bp.enabled === false)).toBe(true);
    s.enableAllBreakpoints();
    expect(useSession.getState().breakpoints.every((bp) => bp.enabled === true)).toBe(true);
  });

  test('addWatchExpression skips empty + duplicate; clear empties', () => {
    const s = useSession.getState();
    s.addWatchExpression('msg.sender');
    s.addWatchExpression('msg.sender'); // dup
    s.addWatchExpression('   '); // blank
    expect(useSession.getState().watchExpressions).toEqual(['msg.sender']);
    s.addWatchExpression('balanceOf(this)');
    expect(useSession.getState().watchExpressions).toEqual([
      'msg.sender',
      'balanceOf(this)',
    ]);
    s.removeWatchExpression('msg.sender');
    expect(useSession.getState().watchExpressions).toEqual(['balanceOf(this)']);
    s.clearWatchExpressions();
    expect(useSession.getState().watchExpressions).toEqual([]);
  });

  test('toggleTraceCallFilter flips on/off and resetTraceCallFilters restores all', () => {
    const s = useSession.getState();
    s.toggleTraceCallFilter('CALL');
    expect(useSession.getState().traceCallFilters).not.toContain('CALL');
    s.toggleTraceCallFilter('CALL');
    expect(useSession.getState().traceCallFilters).toContain('CALL');
    // Toggle off twice in a row without flicker.
    s.toggleTraceCallFilter('STATICCALL');
    s.toggleTraceCallFilter('DELEGATECALL');
    s.resetTraceCallFilters();
    expect(useSession.getState().traceCallFilters).toEqual([
      'CALL',
      'STATICCALL',
      'DELEGATECALL',
      'CALLCODE',
      'CREATE',
      'CREATE2',
    ]);
  });

  test('watchExpressions persist across rehydration', () => {
    useSession.getState().addWatchExpression('1 + 1');
    const raw = localStorage.getItem('edb-web:session')!;
    const persisted = JSON.parse(raw).state;
    expect(persisted.watchExpressions).toEqual(['1 + 1']);
  });

  test('clearBreakpoints empties the list', () => {
    const s = useSession.getState();
    s.addBreakpoint({ loc: { kind: 'Opcode', bytecode_address: '0x' + '0'.repeat(40), pc: 1 }, condition: null });
    s.addBreakpoint({ loc: { kind: 'Opcode', bytecode_address: '0x' + '0'.repeat(40), pc: 2 }, condition: null });
    expect(useSession.getState().breakpoints).toHaveLength(2);
    useSession.getState().clearBreakpoints();
    expect(useSession.getState().breakpoints).toHaveLength(0);
  });

  test('persistence excludes per-session fields', () => {
    const s = useSession.getState();
    s.setActivity('terminal');
    s.openFile({ addr: '0x' + 'a'.repeat(40), path: 'p.sol' });
    s.setLayoutJson('{"l":true}');
    const raw = localStorage.getItem('edb-web:session')!;
    const persisted = JSON.parse(raw).state;
    expect(persisted.activeActivity).toBe('explorer');
    expect(persisted.openFiles).toEqual([]);
    expect(persisted.activeFileId).toBeNull();
    expect(persisted.layoutJson).toBe('{"l":true}');
    expect(persisted.theme).toBe('light');
  });
});
