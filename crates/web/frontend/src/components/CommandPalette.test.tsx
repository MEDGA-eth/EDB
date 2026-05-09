import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { CommandPalette } from './CommandPalette';
import { useSession } from '../store/session';
import { makeWrapper, mockRpc } from '../hooks/_test-utils';

const ADDR_A = '0x' + 'a'.repeat(40);

beforeEach(() => {
  localStorage.clear();
  useSession.setState({
    paletteOpen: true,
    currentSnapshotId: 0,
    breakpoints: [],
    terminalHistory: [],
    activeActivity: 'explorer',
    openFiles: [],
    activeFileId: null,
    theme: 'light',
    wordWrap: false,
    showLineNumbers: true,
    traceExpandTick: 0,
    traceCollapseTick: 0,
  });
  mockRpc({
    edb_getSnapshotCount: () => 4,
    edb_getTrace: () => [
      { id: 0, kind: 'CALL', code_address: ADDR_A, target_address: ADDR_A, children: [] },
    ],
    edb_getCodeByAddress: () => ({
      kind: 'Source',
      entry: 'X.sol',
      files: [
        { path: 'X.sol', content: '' },
        { path: 'Y.sol', content: '' },
      ],
    }),
    edb_getNextCall: () => 0,
    edb_getPrevCall: () => 0,
  });
});
afterEach(cleanup);

describe('<CommandPalette />', () => {
  test('renders nothing when closed', () => {
    useSession.setState({ paletteOpen: false });
    const { wrapper } = makeWrapper();
    render(<CommandPalette />, { wrapper });
    expect(screen.queryByTestId('command-palette')).toBeNull();
  });

  test('opens with input focused', async () => {
    const { wrapper } = makeWrapper();
    render(<CommandPalette />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('palette-input')).toBeTruthy());
    expect(document.activeElement).toBe(screen.getByTestId('palette-input'));
  });

  test('typing > shows commands', async () => {
    const { wrapper } = makeWrapper();
    render(<CommandPalette />, { wrapper });
    const input = screen.getByTestId('palette-input');
    await userEvent.type(input, '>next');
    await waitFor(() => {
      expect(screen.getByTestId('palette-row-cmd:nav.next')).toBeTruthy();
    });
  });

  test('typing : routes to snapshot list', async () => {
    const { wrapper } = makeWrapper();
    render(<CommandPalette />, { wrapper });
    const input = screen.getByTestId('palette-input');
    await userEvent.type(input, ':2');
    await waitFor(() => {
      expect(screen.getByTestId('palette-row-snap:2')).toBeTruthy();
    });
  });

  test('Enter dispatches the active row and closes', async () => {
    useSession.setState({ currentSnapshotId: 2 });
    const { wrapper } = makeWrapper();
    render(<CommandPalette />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('palette-input')).toBeTruthy());
    await userEvent.type(screen.getByTestId('palette-input'), '>first');
    // Wait for the debounced row build to settle and confirm the file rows
    // dropped out (command-mode is active).
    await waitFor(() => expect(screen.getByTestId('palette-row-cmd:nav.first')).toBeTruthy());
    await waitFor(() =>
      expect(screen.queryByTestId(/^palette-row-file:/)).toBeNull(),
    );
    await userEvent.type(screen.getByTestId('palette-input'), '{enter}');
    await waitFor(() => expect(useSession.getState().paletteOpen).toBe(false));
    expect(useSession.getState().currentSnapshotId).toBe(0);
  });

  test('Esc closes the palette', async () => {
    const { wrapper } = makeWrapper();
    render(<CommandPalette />, { wrapper });
    const input = screen.getByTestId('palette-input');
    await userEvent.type(input, '{Escape}');
    expect(useSession.getState().paletteOpen).toBe(false);
  });

  test('clicking a row dispatches its action', async () => {
    const { wrapper } = makeWrapper();
    render(<CommandPalette />, { wrapper });
    const input = screen.getByTestId('palette-input');
    await userEvent.type(input, '>last');
    const row = await screen.findByTestId('palette-row-cmd:nav.last');
    await userEvent.click(row);
    expect(useSession.getState().currentSnapshotId).toBe(3);
    expect(useSession.getState().paletteOpen).toBe(false);
  });

  test('selecting a row pushes it to recent localStorage', async () => {
    const { wrapper } = makeWrapper();
    render(<CommandPalette />, { wrapper });
    const input = screen.getByTestId('palette-input');
    await userEvent.type(input, '>theme');
    const row = await screen.findByTestId('palette-row-cmd:view.toggle-theme');
    // Click the row directly so we don't race the debounced row rebuild
    // against the Enter keypress.
    await userEvent.click(row);
    await waitFor(() => {
      const recent = JSON.parse(localStorage.getItem('edb-web:palette-recent')!);
      expect(recent).toContain('cmd:view.toggle-theme');
    });
  });
});
