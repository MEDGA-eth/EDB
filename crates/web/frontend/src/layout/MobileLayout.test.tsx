import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MobileLayout } from './MobileLayout';
import { makeWrapper, mockRpc } from '../hooks/_test-utils';
import { useSession } from '../store/session';

const snapshot = {
  id: 0,
  frame_id: [0, 0],
  next_id: 1,
  prev_id: 0,
  target_address: '0x' + '0'.repeat(40),
  bytecode_address: '0x' + '0'.repeat(40),
  detail: {
    Opcode: {
      id: 0,
      frame_id: [0, 0],
      pc: 0,
      opcode: 0x00,
      memory: [],
      stack: [],
      calldata: '0x',
      transient_storage: {},
    },
  },
};

function setupMocks() {
  mockRpc({
    edb_getSnapshotCount: () => 1,
    edb_getSnapshotInfo: () => snapshot,
    edb_getTrace: () => ({ inner: [] }),
    edb_getCode: () => ({ Opcode: { bytecode_address: '0x' + '0'.repeat(40), codes: { '0': 'STOP' } } }),
    edb_getStorageDiff: () => ({}),
  });
}

describe('<MobileLayout />', () => {
  beforeEach(() => {
    useSession.getState().setPanelTab('code');
    useSession.getState().setSnapshotId(0);
  });
  afterEach(cleanup);

  test('shows the active tab content (code)', async () => {
    setupMocks();
    const { wrapper } = makeWrapper();
    render(<MobileLayout />, { wrapper });
    expect(screen.getByTestId('mobile-layout')).toBeTruthy();
    expect(screen.getByTestId('mobile-tab-code')).toBeTruthy();
    await waitFor(() => expect(screen.getByTestId('opcodes-view')).toBeTruthy());
  });

  test('clicking a tab switches the active panel', async () => {
    setupMocks();
    const { wrapper } = makeWrapper();
    render(<MobileLayout />, { wrapper });
    await userEvent.click(screen.getByTestId('mobile-tab-terminal'));
    expect(useSession.getState().panelTab).toBe('terminal');
    await waitFor(() => expect(screen.getByTestId('terminal-panel')).toBeTruthy());
  });
});
