import { afterEach, describe, expect, test } from 'bun:test';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { FileTabPanel } from './FileTabPanel';
import { makeWrapper, mockRpc } from '../hooks/_test-utils';
import { useSession } from '../store/session';

const ADDR = '0x' + 'a'.repeat(40);

/** Build a SnapshotInfo with an Opcode detail at a given pc. The disasm
 *  uses 4-hex padded prefixes so we use even, well-spaced PCs to keep
 *  pcLineIndex deterministic. */
function makeOpcodeSnap(id: number, pc: number) {
  return {
    id,
    frame_id: [0, 0],
    next_id: id + 1,
    prev_id: Math.max(0, id - 1),
    target_address: ADDR,
    bytecode_address: ADDR,
    detail: {
      Opcode: {
        id,
        frame_id: [0, 0],
        pc,
        opcode: 0x60,
        memory: [],
        stack: [],
        calldata: '0x',
        transient_storage: {},
      },
    },
  };
}

describe('<FileTabPanel />', () => {
  afterEach(cleanup);

  test('renders opcodes view for Opcode payload', async () => {
    mockRpc({
      edb_getCodeByAddress: () => ({
        Opcode: { bytecode_address: ADDR, codes: { '0': 'PUSH1 0x60' } },
      }),
    });
    const { wrapper } = makeWrapper();
    render(<FileTabPanel addr={ADDR} path="<disasm>" />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('opcodes-view')).toBeTruthy());
  });

  test('renders solidity view for Source payload', async () => {
    mockRpc({
      edb_getCodeByAddress: () => ({
        Source: {
          bytecode_address: ADDR,
          sources: { 'a.sol': 'contract X{}' },
        },
      }),
    });
    const { wrapper } = makeWrapper();
    render(<FileTabPanel addr={ADDR} path="a.sol" />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('solidity-view')).toBeTruthy());
  });

  test('accepts dockview-style params prop', async () => {
    mockRpc({
      edb_getCodeByAddress: () => ({
        Opcode: { bytecode_address: ADDR, codes: { '0': 'STOP' } },
      }),
    });
    const { wrapper } = makeWrapper();
    render(<FileTabPanel params={{ addr: ADDR, path: '<disasm>' }} />, { wrapper });
    await waitFor(() => expect(screen.getByTestId('opcodes-view')).toBeTruthy());
  });

  test('opcode highlight follows snapshot forwards AND backwards', async () => {
    // 100 opcodes at PCs 0, 2, 4, … 198. Each line in the resulting disasm
    // is `<pc-hex-4>: OP_<i>`. We'll watch which line carries the
    // `data-edb-current="true"` attribute as the active snapshot moves.
    const codes: Record<string, string> = {};
    for (let i = 0; i < 100; i += 1) codes[(i * 2).toString()] = `OP${i}`;
    let pcForId = (id: number) => id * 2;
    mockRpc({
      edb_getCodeByAddress: () => ({
        Opcode: { bytecode_address: ADDR, codes },
      }),
      edb_getSnapshotInfo: (params) => {
        const [id] = (params ?? []) as [number];
        return makeOpcodeSnap(id, pcForId(id));
      },
    });
    useSession.setState({ currentSnapshotId: 50, navHistory: [] });
    const { wrapper } = makeWrapper();
    render(<FileTabPanel addr={ADDR} path="<disasm>" />, { wrapper });

    // Wait for the disasm to render and the highlight to land on line 50.
    await waitFor(() => {
      const cur = document.querySelector('[data-edb-current="true"]');
      expect(cur?.textContent).toContain('OP50');
    });

    // Step forward to id 60. setSnapshotId pushes 50 onto navHistory.
    useSession.getState().setSnapshotId(60);
    await waitFor(() => {
      const cur = document.querySelector('[data-edb-current="true"]');
      expect(cur?.textContent).toContain('OP60');
    });

    // Reverse Step (history pop) → back to id 50. This is the case that
    // broke before the data-attribute fix: a single ref swapped between
    // siblings ended up null because the new (DOM-earlier) row was
    // written first, then the old (DOM-later) row cleared the ref.
    useSession.getState().goBack();
    await waitFor(() => {
      const cur = document.querySelector('[data-edb-current="true"]');
      expect(cur?.textContent).toContain('OP50');
    });
    // Exactly one row is current at any given time.
    expect(document.querySelectorAll('[data-edb-current="true"]').length).toBe(1);
  });
});
