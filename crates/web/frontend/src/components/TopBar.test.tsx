import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import { TopBar } from './TopBar';
import { makeWrapper, mockRpc } from '../hooks/_test-utils';
import { useSession } from '../store/session';

describe('<TopBar />', () => {
  beforeEach(() => {
    window.location.hash = '';
    useSession.getState().setSnapshotId(0);
  });
  afterEach(cleanup);

  test('renders snapshot id and count when count loads', async () => {
    mockRpc({ edb_getSnapshotCount: () => 7 });
    useSession.getState().setSnapshotId(3);
    const { wrapper } = makeWrapper();
    render(<TopBar />, { wrapper });
    const label = screen.getByTestId('snapshot-label');
    expect(label.textContent).toContain('snapshot 3');
    await waitFor(() => expect(label.textContent).toContain('/ 7'));
  });

  test('renders txHash link to Etherscan when provided', () => {
    mockRpc({ edb_getSnapshotCount: () => 1 });
    const tx = '0x' + 'a'.repeat(64);
    const { wrapper } = makeWrapper();
    render(<TopBar txHash={tx} />, { wrapper });
    const link = screen.getByTestId('tx-link') as HTMLAnchorElement;
    expect(link.getAttribute('href')).toBe(`https://etherscan.io/tx/${tx}`);
    expect(link.getAttribute('target')).toBe('_blank');
    // Truncated label: first 10 chars + ellipsis + last 8
    expect(link.textContent).toContain(tx.slice(0, 10));
    expect(link.textContent).toContain(tx.slice(-8));
  });

  test('omits txHash link when not provided', () => {
    mockRpc({ edb_getSnapshotCount: () => 1 });
    const { wrapper } = makeWrapper();
    render(<TopBar />, { wrapper });
    expect(screen.queryByTestId('tx-link')).toBeNull();
  });

  test('hash binding: changing snapshot id writes to window.location.hash', async () => {
    mockRpc({ edb_getSnapshotCount: () => 10 });
    useSession.getState().setSnapshotId(0);
    const { wrapper } = makeWrapper();
    render(<TopBar />, { wrapper });
    await waitFor(() => expect(window.location.hash).toBe('#0'));
    act(() => {
      useSession.getState().setSnapshotId(5);
    });
    await waitFor(() => expect(window.location.hash).toBe('#5'));
  });

  test('hash binding: hashchange event updates the store', async () => {
    mockRpc({ edb_getSnapshotCount: () => 10 });
    useSession.getState().setSnapshotId(0);
    const { wrapper } = makeWrapper();
    render(<TopBar />, { wrapper });
    await waitFor(() => expect(window.location.hash).toBe('#0'));
    act(() => {
      window.location.hash = '#4';
      window.dispatchEvent(new HashChangeEvent('hashchange'));
    });
    await waitFor(() => expect(useSession.getState().currentSnapshotId).toBe(4));
  });
});
