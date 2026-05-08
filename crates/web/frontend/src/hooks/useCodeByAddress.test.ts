import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { makeWrapper, mockRpc } from './_test-utils';
import { useCodeByAddress } from './useCodeByAddress';

const opcodeFixture = { kind: 'Opcodes', disasm: '0000: PUSH1 0x60' };

describe('useCodeByAddress', () => {
  afterEach(() => (globalThis.fetch as { mockReset?: () => void }).mockReset?.());

  test('passes address as param', async () => {
    let received: unknown[] = [];
    mockRpc({ edb_getCodeByAddress: (p) => { received = p!; return opcodeFixture; } });
    const { wrapper } = makeWrapper();
    const addr = '0x' + '0'.repeat(40);
    const { result } = renderHook(() => useCodeByAddress(addr), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(received).toEqual([addr]);
    expect(result.current.data?.kind).toBe('Opcodes');
  });

  test('schema error on bad shape', async () => {
    mockRpc({ edb_getCodeByAddress: () => ({ kind: 'Bogus' }) });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useCodeByAddress('0x' + '0'.repeat(40)), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
  });

  test('does not fetch when addr is undefined', async () => {
    let calls = 0;
    mockRpc({ edb_getCodeByAddress: () => { calls++; return opcodeFixture; } });
    const { wrapper } = makeWrapper();
    renderHook(() => useCodeByAddress(undefined), { wrapper });
    await new Promise((r) => setTimeout(r, 30));
    expect(calls).toBe(0);
  });
});
