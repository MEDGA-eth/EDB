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

  test('renders snapshot id (1-based) and count', async () => {
    mockRpc({ edb_getSnapshotCount: () => 7 });
    useSession.getState().setSnapshotId(3);
    const { wrapper } = makeWrapper();
    render(<StatusBar />, { wrapper });
    const label = screen.getByTestId('snapshot-label');
    // Display is 1-based: store id 3 → label '4'.
    expect(label.textContent).toContain('snapshot 4');
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

  test('hash binding: clamps an out-of-bounds hash to count - 1 and rewrites', async () => {
    mockRpc({ edb_getSnapshotCount: () => 5 });
    useSession.getState().setSnapshotId(0);
    const { wrapper } = makeWrapper();
    render(<StatusBar />, { wrapper });
    // Wait until the snapshot count has loaded.
    await waitFor(() => expect(screen.getByTestId('snapshot-label').textContent).toContain('/ 5'));
    act(() => {
      window.location.hash = '#42';
      window.dispatchEvent(new HashChangeEvent('hashchange'));
    });
    // 42 → clamped to 4 (count-1). The hash is rewritten to match.
    await waitFor(() => expect(useSession.getState().currentSnapshotId).toBe(4));
    await waitFor(() => expect(window.location.hash).toBe('#4'));
  });

  test('hash binding: clamps a negative hash to 0', async () => {
    mockRpc({ edb_getSnapshotCount: () => 5 });
    useSession.getState().setSnapshotId(2);
    const { wrapper } = makeWrapper();
    render(<StatusBar />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('snapshot-label').textContent).toContain('/ 5'));
    act(() => {
      window.location.hash = '#-7';
      window.dispatchEvent(new HashChangeEvent('hashchange'));
    });
    await waitFor(() => expect(useSession.getState().currentSnapshotId).toBe(0));
    await waitFor(() => expect(window.location.hash).toBe('#0'));
  });
});
