import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { makeWrapper, mockRpc } from './_test-utils';
import { useCode } from './useCode';

const opcodeFixture = { kind: 'Opcodes', disasm: '0000: PUSH1 0x60' };

describe('useCode', () => {
  afterEach(() => (globalThis.fetch as { mockReset?: () => void }).mockReset?.());

  test('passes snapshot id', async () => {
    let received: unknown[] = [];
    mockRpc({ edb_getCode: (p) => { received = p!; return opcodeFixture; } });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useCode(3), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(received).toEqual([3]);
    expect(result.current.data?.kind).toBe('Opcodes');
  });

  test('schema error on bad shape', async () => {
    mockRpc({ edb_getCode: () => ({ kind: 'Bogus' }) });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useCode(0), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
  });
});
