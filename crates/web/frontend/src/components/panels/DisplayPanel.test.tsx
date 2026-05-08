import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { DisplayPanel } from './DisplayPanel';
import { makeWrapper, mockRpc } from '../../hooks/_test-utils';
import { useSession } from '../../store/session';

const sample = {
  id: 0,
  frame_id: { trace_entry_id: 0, step_id: 0 },
  next_id: 1,
  prev_id: 0,
  target_address: '0x' + '0'.repeat(40),
  bytecode_address: '0x' + '0'.repeat(40),
  detail: {
    kind: 'Opcode',
    id: 0,
    frame_id: { trace_entry_id: 0, step_id: 0 },
    pc: 0,
    opcode: 0x60,
    memory: [
      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
      1,
    ],
    stack: ['0x1', '0x2'],
    calldata: [],
    transient_storage: {},
  },
};

describe('<DisplayPanel />', () => {
  afterEach(cleanup);

  test('renders all 5 tabs and switches', async () => {
    mockRpc({ edb_getSnapshotInfo: () => sample, edb_getStorageDiff: () => [] });
    useSession.getState().setSnapshotId(0);
    const { wrapper } = makeWrapper();
    render(<DisplayPanel />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('display-tab-vars')).toBeTruthy());
    for (const t of ['vars', 'stack', 'memory', 'storage', 'transient'] as const) {
      await userEvent.click(screen.getByTestId(`display-tab-${t}`));
      expect(screen.getByTestId(`display-tab-content-${t}`)).toBeTruthy();
    }
  });

  test('memory tab renders 32-byte rows', async () => {
    mockRpc({ edb_getSnapshotInfo: () => sample, edb_getStorageDiff: () => [] });
    useSession.getState().setSnapshotId(0);
    const { wrapper } = makeWrapper();
    render(<DisplayPanel />, { wrapper });
    await userEvent.click(await screen.findByTestId('display-tab-memory'));
    const content = await screen.findByTestId('display-tab-content-memory');
    expect(content.textContent).toMatch(/000000:/);
  });

  test('storage tab renders diff rows', async () => {
    mockRpc({
      edb_getSnapshotInfo: () => sample,
      edb_getStorageDiff: () => [{ slot: '0x0', before: '0x0', after: '0x1' }],
    });
    useSession.getState().setSnapshotId(0);
    const { wrapper } = makeWrapper();
    render(<DisplayPanel />, { wrapper });
    await userEvent.click(await screen.findByTestId('display-tab-storage'));
    expect(await screen.findByTestId('storage-row-0')).toBeTruthy();
  });
});
