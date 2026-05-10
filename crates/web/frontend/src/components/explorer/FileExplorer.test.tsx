import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { FileExplorer } from './FileExplorer';
import { makeWrapper, mockRpc } from '../../hooks/_test-utils';
import { useSession } from '../../store/session';

const ADDR_A = '0x' + 'a'.repeat(40);
const ADDR_B = '0x' + 'b'.repeat(40);

function entry(id: number, parent: number | null, addr: string) {
  return {
    id,
    parent_id: parent,
    depth: parent === null ? 0 : 1,
    call_type: { Call: 'Call' },
    caller: '0x' + '1'.repeat(40),
    target: addr,
    code_address: addr,
    input: '0x',
    value: '0x0',
    result: { Success: { output: '0x', result: 'Stop' } },
    created_contract: false,
    create_scheme: null,
    bytecode: '0x6080',
    target_label: null,
    self_destruct: null,
    events: [],
    first_snapshot_id: id,
  };
}

describe('<FileExplorer />', () => {
  afterEach(() => {
    cleanup();
    useSession.setState({ openFiles: [], activeFileId: null });
  });

  test('shows empty state when trace has no entries', async () => {
    mockRpc({ edb_getTrace: () => ({ inner: [] }) });
    const { wrapper } = makeWrapper();
    render(<FileExplorer />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('explorer-empty')).toBeTruthy());
  });

  test('lists unique code addresses from the trace', async () => {
    mockRpc({
      edb_getTrace: () => ({
        inner: [
          entry(0, null, ADDR_A),
          entry(1, 0, ADDR_B),
          entry(2, 0, ADDR_A),
        ],
      }),
      edb_getCodeByAddress: (params) => ({
        Opcode: { bytecode_address: (params as string[])[0], codes: { '0': 'STOP' } },
      }),
    });
    const { wrapper } = makeWrapper();
    render(<FileExplorer />, { wrapper });
    await waitFor(() => expect(screen.getByTestId(`explorer-addr-${ADDR_A}`)).toBeTruthy());
    await waitFor(() => expect(screen.getByTestId(`explorer-addr-${ADDR_B}`)).toBeTruthy());
  });

  test('clicking a file calls openFile in the session store', async () => {
    mockRpc({
      edb_getTrace: () => ({ inner: [entry(0, null, ADDR_A)] }),
      edb_getCodeByAddress: () => ({
        Source: {
          bytecode_address: ADDR_A,
          sources: { 'a.sol': 'contract X{}' },
        },
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
      edb_getTrace: () => ({ inner: [entry(0, null, ADDR_A), entry(1, 0, ADDR_B)] }),
      edb_getCodeByAddress: (params) => ({
        Source: {
          bytecode_address: (params as string[])[0],
          sources: { 'a.sol': '' },
        },
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
    // Press ArrowDown a few times then Enter, should land on a file and open it.
    fireEvent.keyDown(tree, { key: 'ArrowDown' });
    fireEvent.keyDown(tree, { key: 'Enter' });
    await waitFor(() => {
      expect(useSession.getState().openFiles.some((f) => f.path === 'a.sol')).toBe(true);
    });
  });

  test('error state surfaces a retry affordance', async () => {
    let calls = 0;
    mockRpc({
      edb_getTrace: () => ({ inner: [entry(0, null, ADDR_A)] }),
      edb_getCodeByAddress: () => {
        calls += 1;
        // Always fail, we just want the error state to surface.
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
