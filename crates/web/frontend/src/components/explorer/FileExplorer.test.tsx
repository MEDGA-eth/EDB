import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { FileExplorer } from './FileExplorer';
import { makeWrapper, mockRpc } from '../../hooks/_test-utils';
import { useSession } from '../../store/session';

const ADDR_A = '0x' + 'a'.repeat(40);
const ADDR_B = '0x' + 'b'.repeat(40);

describe('<FileExplorer />', () => {
  afterEach(() => {
    cleanup();
    useSession.setState({ openFiles: [], activeFileId: null });
  });

  test('shows empty state when trace has no entries', async () => {
    mockRpc({ edb_getTrace: () => [] });
    const { wrapper } = makeWrapper();
    render(<FileExplorer />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('explorer-empty')).toBeTruthy());
  });

  test('lists unique code addresses from the trace', async () => {
    mockRpc({
      edb_getTrace: () => [
        {
          id: 0,
          kind: 'Call',
          code_address: ADDR_A,
          target_address: ADDR_A,
          children: [
            { id: 1, kind: 'Call', code_address: ADDR_B, target_address: ADDR_B, children: [] },
            { id: 2, kind: 'Call', code_address: ADDR_A, target_address: ADDR_A, children: [] },
          ],
        },
      ],
      edb_getCodeByAddress: () => ({ kind: 'Opcodes', disasm: 'STOP' }),
    });
    const { wrapper } = makeWrapper();
    render(<FileExplorer />, { wrapper });
    await waitFor(() => expect(screen.getByTestId(`explorer-addr-${ADDR_A}`)).toBeTruthy());
    await waitFor(() => expect(screen.getByTestId(`explorer-addr-${ADDR_B}`)).toBeTruthy());
  });

  test('clicking a file calls openFile in the session store', async () => {
    mockRpc({
      edb_getTrace: () => [
        { id: 0, kind: 'Call', code_address: ADDR_A, target_address: ADDR_A, children: [] },
      ],
      edb_getCodeByAddress: () => ({
        kind: 'Source',
        entry: 'a.sol',
        files: [{ path: 'a.sol', content: 'contract X{}' }],
      }),
    });
    const { wrapper } = makeWrapper();
    render(<FileExplorer />, { wrapper });
    const node = await waitFor(() =>
      screen.getByTestId(`explorer-file-${ADDR_A}-a.sol`),
    );
    await userEvent.click(node);
    expect(useSession.getState().openFiles).toHaveLength(1);
    expect(useSession.getState().openFiles[0]?.path).toBe('a.sol');
  });

  test('arrow keys move focus between rows; Enter on a file opens it', async () => {
    mockRpc({
      edb_getTrace: () => [
        {
          id: 0,
          kind: 'Call',
          code_address: ADDR_A,
          target_address: ADDR_A,
          children: [
            { id: 1, kind: 'Call', code_address: ADDR_B, target_address: ADDR_B, children: [] },
          ],
        },
      ],
      edb_getCodeByAddress: () => ({
        kind: 'Source',
        entry: 'a.sol',
        files: [{ path: 'a.sol', content: '' }],
      }),
    });
    const { wrapper } = makeWrapper();
    render(<FileExplorer />, { wrapper });
    // Wait for both addresses to render.
    await waitFor(() => expect(screen.getByTestId(`explorer-addr-${ADDR_A}`)).toBeTruthy());
    await waitFor(() => expect(screen.getByTestId(`explorer-addr-${ADDR_B}`)).toBeTruthy());
    const tree = screen.getByTestId('file-explorer');
    expect(tree.getAttribute('role')).toBe('tree');
    // Wait for the file rows under ADDR_A to appear.
    await waitFor(() =>
      expect(screen.getByTestId(`explorer-file-${ADDR_A}-a.sol`)).toBeTruthy(),
    );
    // Press ArrowDown a few times then Enter — should land on a file and open it.
    fireEvent.keyDown(tree, { key: 'ArrowDown' });
    fireEvent.keyDown(tree, { key: 'Enter' });
    await waitFor(() => {
      expect(useSession.getState().openFiles.some((f) => f.path === 'a.sol')).toBe(true);
    });
  });

  test('error state surfaces a retry affordance', async () => {
    let calls = 0;
    mockRpc({
      edb_getTrace: () => [
        { id: 0, kind: 'Call', code_address: ADDR_A, target_address: ADDR_A, children: [] },
      ],
      edb_getCodeByAddress: () => {
        calls += 1;
        // Always fail — we just want the error state to surface.
        const err: { code: number; message: string } = { code: -1, message: 'boom' };
        throw err;
      },
    });
    const { wrapper } = makeWrapper();
    render(<FileExplorer />, { wrapper });
    await waitFor(() => expect(screen.getByTestId(`explorer-error-${ADDR_A}`)).toBeTruthy());
    const before = calls;
    await userEvent.click(screen.getByTestId(`explorer-retry-${ADDR_A}`));
    await waitFor(() => expect(calls).toBeGreaterThan(before));
  });
});
