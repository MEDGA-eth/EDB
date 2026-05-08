import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { CodePanel } from './CodePanel';
import { makeWrapper, mockRpc } from '../../hooks/_test-utils';
import { useSession } from '../../store/session';

describe('<CodePanel />', () => {
  afterEach(cleanup);

  test('renders opcodes view for Opcodes payload', async () => {
    mockRpc({ edb_getCode: () => ({ kind: 'Opcodes', disasm: 'PUSH1 0x60' }) });
    useSession.getState().setSnapshotId(0);
    const { wrapper } = makeWrapper();
    render(<CodePanel />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('opcodes-view')).toBeTruthy());
  });

  test('renders solidity view for Source payload', async () => {
    mockRpc({
      edb_getCode: () => ({
        kind: 'Source',
        entry: 'a.sol',
        files: [{ path: 'a.sol', content: 'contract X{}' }],
      }),
    });
    useSession.getState().setSnapshotId(0);
    const { wrapper } = makeWrapper();
    render(<CodePanel />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('solidity-view')).toBeTruthy());
  });

  test('shows error card on RPC failure', async () => {
    mockRpc({
      edb_getCode: () => {
        throw new Error('boom');
      },
    });
    useSession.getState().setSnapshotId(0);
    const { wrapper } = makeWrapper();
    render(<CodePanel />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('error-card')).toBeTruthy());
  });
});
