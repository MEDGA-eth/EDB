import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { useAvailableFiles } from './useAvailableFiles';
import { makeWrapper, mockRpc } from './_test-utils';

const ADDR_A = '0x' + 'a'.repeat(40);
const ADDR_B = '0x' + 'b'.repeat(40);

function traceEntry(id: number, parent: number | null, addr: string) {
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

describe('useAvailableFiles', () => {
  afterEach(() => {
    // Reset fetch
  });

  test('returns Source files for addresses with source code', async () => {
    mockRpc({
      edb_getTrace: () => ({
        inner: [traceEntry(0, null, ADDR_A), traceEntry(1, 0, ADDR_B)],
      }),
      edb_getCodeByAddress: (params) => {
        const addr = (params as string[])[0];
        if (addr === ADDR_A) {
          return {
            Source: {
              bytecode_address: ADDR_A,
              sources: { 'X.sol': 'contract X{}', 'Y.sol': 'contract Y{}' },
            },
          };
        }
        return { Opcode: { bytecode_address: ADDR_B, codes: { '0': 'PUSH1 0x60' } } };
      },
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useAvailableFiles(), { wrapper });
    await waitFor(() => expect(result.current.files.length).toBe(3));
    const paths = result.current.files.map((f) => f.path).sort();
    expect(paths).toEqual(['<disasm>', 'X.sol', 'Y.sol']);
  });

  test('returns empty when trace is empty', async () => {
    mockRpc({ edb_getTrace: () => ({ inner: [] }) });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useAvailableFiles(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.files).toEqual([]);
    expect(result.current.addresses).toEqual([]);
  });
});
