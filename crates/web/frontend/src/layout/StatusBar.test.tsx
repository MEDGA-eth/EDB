import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import { StatusBar } from './StatusBar';
import { makeWrapper, mockRpc } from '../hooks/_test-utils';
import { useSession } from '../store/session';

describe('<StatusBar />', () => {
  beforeEach(() => {
    window.location.hash = '';
    useSession.getState().setSnapshotId(0);
  });
  afterEach(cleanup);

  test('renders snapshot id and count', async () => {
    mockRpc({ edb_getSnapshotCount: () => 7 });
    useSession.getState().setSnapshotId(3);
    const { wrapper } = makeWrapper();
    render(<StatusBar />, { wrapper });
    const label = screen.getByTestId('snapshot-label');
    expect(label.textContent).toContain('snapshot 3');
    await waitFor(() => expect(label.textContent).toContain('/ 7'));
  });

  test('hash binding: store change writes window.location.hash', async () => {
    mockRpc({ edb_getSnapshotCount: () => 10 });
    useSession.getState().setSnapshotId(0);
    const { wrapper } = makeWrapper();
    render(<StatusBar />, { wrapper });
    await waitFor(() => expect(window.location.hash).toBe('#0'));
    act(() => useSession.getState().setSnapshotId(5));
    await waitFor(() => expect(window.location.hash).toBe('#5'));
  });

  test('hash binding: hashchange event updates the store', async () => {
    mockRpc({ edb_getSnapshotCount: () => 10 });
    useSession.getState().setSnapshotId(0);
    const { wrapper } = makeWrapper();
    render(<StatusBar />, { wrapper });
    await waitFor(() => expect(window.location.hash).toBe('#0'));
    act(() => {
      window.location.hash = '#4';
      window.dispatchEvent(new HashChangeEvent('hashchange'));
    });
    await waitFor(() => expect(useSession.getState().currentSnapshotId).toBe(4));
  });
});
