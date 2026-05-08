import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { TracePanel } from './TracePanel';
import { makeWrapper, mockRpc } from '../../hooks/_test-utils';
import { useSession } from '../../store/session';

const fakeTrace = [
  { id: 0, kind: 'CALL', code_address: '0x' + '0'.repeat(40), target_address: '0x' + '1'.repeat(40),
    children: [
      { id: 1, kind: 'STATICCALL', code_address: '0x' + '0'.repeat(40), target_address: '0x' + '2'.repeat(40), children: [] },
    ] },
];

describe('<TracePanel />', () => {
  afterEach(cleanup);

  test('renders nested trace', async () => {
    mockRpc({ edb_getTrace: () => fakeTrace });
    const { wrapper } = makeWrapper();
    render(<TracePanel />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('trace-entry-0')).toBeTruthy());
    expect(screen.getByTestId('trace-entry-1')).toBeTruthy();
  });

  test('clicking a node sets snapshot id', async () => {
    mockRpc({ edb_getTrace: () => fakeTrace });
    const { wrapper } = makeWrapper();
    render(<TracePanel />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('trace-entry-1')).toBeTruthy());
    await userEvent.click(screen.getByTestId('trace-entry-1'));
    expect(useSession.getState().currentSnapshotId).toBe(1);
  });
});
