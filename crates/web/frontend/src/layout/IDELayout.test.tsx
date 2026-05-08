import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { IDELayout } from './IDELayout';
import { makeWrapper, mockRpc } from '../hooks/_test-utils';
import { useSession } from '../store/session';

describe('<IDELayout />', () => {
  beforeEach(() => {
    useSession.setState({
      currentSnapshotId: 0,
      activeActivity: 'explorer',
      openFiles: [],
      activeFileId: null,
      layoutJson: null,
    });
    window.location.hash = '';
    // dockview reads ResizeObserver; happy-dom 16+ provides one but stub if absent
    if (typeof globalThis.ResizeObserver === 'undefined') {
      class RO { observe(){} unobserve(){} disconnect(){} }
      globalThis.ResizeObserver = RO as unknown as typeof ResizeObserver;
    }
  });
  afterEach(cleanup);

  test('renders shell chrome (activity bar + side bar + status bar) with explorer empty state', async () => {
    mockRpc({
      edb_getSnapshotCount: () => 1,
      edb_getTrace: () => [],
    });
    const { wrapper } = makeWrapper();
    render(<IDELayout />, { wrapper });

    expect(screen.getByTestId('ide-layout')).toBeTruthy();
    expect(screen.getByTestId('activity-bar')).toBeTruthy();
    expect(screen.getByTestId('side-bar')).toBeTruthy();
    expect(screen.getByTestId('status-bar')).toBeTruthy();
    // No files → editor-empty placeholder
    expect(screen.getByTestId('editor-empty')).toBeTruthy();
    await waitFor(() => expect(screen.getByTestId('explorer-empty')).toBeTruthy());
  });

  test('switching activity updates the side bar', async () => {
    mockRpc({
      edb_getSnapshotCount: () => 1,
      edb_getTrace: () => [],
    });
    useSession.setState({ activeActivity: 'breakpoints' });
    const { wrapper } = makeWrapper();
    render(<IDELayout />, { wrapper });
    expect(screen.getByTestId('side-bar-breakpoints')).toBeTruthy();
  });
});
