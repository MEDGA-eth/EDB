import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { TracePanel } from './TracePanel';
import { makeWrapper, mockRpc } from '../../hooks/_test-utils';
import { useSession } from '../../store/session';

const ADDR_A = '0x' + '0'.repeat(40);
const ADDR_B = '0x' + '1'.repeat(40);
const ADDR_C = '0x' + '2'.repeat(40);

function entry(id: number, parent: number | null, target: string, scheme = 'Call') {
  return {
    id,
    parent_id: parent,
    depth: parent === null ? 0 : 1,
    call_type: { Call: scheme },
    caller: ADDR_A,
    target,
    code_address: target,
    input: '0x',
    value: '0x0',
    result: { Success: { output: '0x', result: 'Stop' } },
    created_contract: false,
    create_scheme: null,
    bytecode: '0x6080',
    target_label: null,
    self_destruct: null,
    events: [],
    first_snapshot_id: id,
  };
}

const fakeTrace = {
  inner: [
    entry(0, null, ADDR_B, 'Call'),
    entry(1, 0, ADDR_C, 'StaticCall'),
  ],
};

describe('<TracePanel />', () => {
  afterEach(cleanup);

  test('renders nested trace from flat list', async () => {
    mockRpc({ edb_getTrace: () => fakeTrace });
    const { wrapper } = makeWrapper();
    render(<TracePanel />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('trace-entry-0')).toBeTruthy());
    expect(screen.getByTestId('trace-entry-1')).toBeTruthy();
  });

  test('plain left-click does NOT change the snapshot (reveal-only)', async () => {
    mockRpc({ edb_getTrace: () => fakeTrace });
    useSession.setState({ currentSnapshotId: 0 });
    const { wrapper } = makeWrapper();
    render(<TracePanel />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('trace-entry-1')).toBeTruthy());
    await userEvent.click(screen.getByTestId('trace-entry-1'));
    // Reveal-only mode: stepping position is preserved.
    expect(useSession.getState().currentSnapshotId).toBe(0);
  });

  test('shift+click jumps to first_snapshot_id', async () => {
    mockRpc({ edb_getTrace: () => fakeTrace });
    useSession.setState({ currentSnapshotId: 0 });
    const { wrapper } = makeWrapper();
    render(<TracePanel />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('trace-entry-1')).toBeTruthy());
    // user-event v14 doesn't take a `{ shiftKey: true }` option on click,
    // so go through fireEvent which lets us pass it on the synthetic event.
    fireEvent.click(screen.getByTestId('trace-entry-1'), { shiftKey: true });
    // entry 1 maps to first_snapshot_id == 1
    expect(useSession.getState().currentSnapshotId).toBe(1);
  });

  test('right-click opens a context menu with Jump / Reveal options', async () => {
    mockRpc({ edb_getTrace: () => fakeTrace });
    useSession.setState({ currentSnapshotId: 0 });
    const { wrapper } = makeWrapper();
    render(<TracePanel />, { wrapper });
    const node = await screen.findByTestId('trace-entry-1');
    // Fire a contextmenu event to open the menu.
    const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 100, clientY: 100 });
    node.dispatchEvent(ev);
    expect(await screen.findByTestId('trace-menu-1')).toBeTruthy();
    // The menu carries a "Jump to snapshot" item that flips the snapshot.
    const jump = screen.getByText(/Jump to snapshot/);
    await userEvent.click(jump);
    expect(useSession.getState().currentSnapshotId).toBe(1);
  });
});
