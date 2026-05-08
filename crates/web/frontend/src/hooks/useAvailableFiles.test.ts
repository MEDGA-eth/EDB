import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { useAvailableFiles } from './useAvailableFiles';
import { makeWrapper, mockRpc } from './_test-utils';

const ADDR_A = '0x' + 'a'.repeat(40);
const ADDR_B = '0x' + 'b'.repeat(40);

describe('useAvailableFiles', () => {
  afterEach(() => {
    // Reset fetch
  });

  test('returns Source files for addresses with source code', async () => {
    mockRpc({
      edb_getTrace: () => [
        { id: 0, kind: 'CALL', code_address: ADDR_A, target_address: ADDR_A, children: [] },
        { id: 1, kind: 'CALL', code_address: ADDR_B, target_address: ADDR_B, children: [] },
      ],
      edb_getCodeByAddress: (params) => {
        const addr = (params as string[])[0];
        if (addr === ADDR_A) {
          return {
            kind: 'Source',
            entry: 'X.sol',
            files: [
              { path: 'X.sol', content: 'contract X{}' },
              { path: 'Y.sol', content: 'contract Y{}' },
            ],
          };
        }
        return { kind: 'Opcodes', disasm: 'PUSH1' };
      },
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useAvailableFiles(), { wrapper });
    await waitFor(() => expect(result.current.files.length).toBe(3));
    const paths = result.current.files.map((f) => f.path).sort();
    expect(paths).toEqual(['<disasm>', 'X.sol', 'Y.sol']);
  });

  test('returns empty when trace is empty', async () => {
    mockRpc({ edb_getTrace: () => [] });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useAvailableFiles(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.files).toEqual([]);
    expect(result.current.addresses).toEqual([]);
  });
});
