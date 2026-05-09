import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
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

  test('clicking a node sets snapshot id to first_snapshot_id', async () => {
    mockRpc({ edb_getTrace: () => fakeTrace });
    const { wrapper } = makeWrapper();
    render(<TracePanel />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('trace-entry-1')).toBeTruthy());
    await userEvent.click(screen.getByTestId('trace-entry-1'));
    // entry 1 maps to first_snapshot_id == 1
    expect(useSession.getState().currentSnapshotId).toBe(1);
  });
});
