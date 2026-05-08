import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { DesktopLayout } from './DesktopLayout';
import { makeWrapper, mockRpc } from '../hooks/_test-utils';
import { useSession } from '../store/session';

const snapshot = {
  id: 0,
  frame_id: { trace_entry_id: 0, step_id: 0 },
  next_id: 1,
  prev_id: 0,
  target_address: '0x' + '0'.repeat(40),
  bytecode_address: '0x' + '0'.repeat(40),
  detail: {
    kind: 'Opcode',
    pc: 0,
    opcode: 0x00,
    memory: [],
    stack: [],
    calldata: [],
    transient_storage: {},
  },
};

describe('<DesktopLayout />', () => {
  afterEach(cleanup);

  test('renders all 4 panels', async () => {
    mockRpc({
      edb_getSnapshotCount: () => 1,
      edb_getSnapshotInfo: () => snapshot,
      edb_getTrace: () => [],
      edb_getCode: () => ({ kind: 'Opcodes', disasm: '0000: STOP' }),
      edb_getStorageDiff: () => [],
    });
    useSession.getState().setSnapshotId(0);
    const { wrapper } = makeWrapper();
    render(<DesktopLayout />, { wrapper });

    expect(screen.getByTestId('desktop-layout')).toBeTruthy();
    await waitFor(() => expect(screen.getByTestId('opcodes-view')).toBeTruthy());
    await waitFor(() => expect(screen.getByTestId('trace-panel')).toBeTruthy());
    await waitFor(() => expect(screen.getByTestId('display-tab-vars')).toBeTruthy());
    expect(screen.getByTestId('terminal-panel')).toBeTruthy();
  });
});
