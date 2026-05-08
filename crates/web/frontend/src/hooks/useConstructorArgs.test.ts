import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { makeWrapper, mockRpc } from './_test-utils';
import { useConstructorArgs } from './useConstructorArgs';

describe('useConstructorArgs', () => {
  afterEach(() => (globalThis.fetch as { mockReset?: () => void }).mockReset?.());

  test('passes address as param', async () => {
    let received: unknown[] = [];
    mockRpc({ edb_getConstructorArgs: (p) => { received = p!; return []; } });
    const { wrapper } = makeWrapper();
    const addr = '0x' + '0'.repeat(40);
    const { result } = renderHook(() => useConstructorArgs(addr), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(received).toEqual([addr]);
    expect(result.current.data).toEqual([]);
  });

  test('schema error on bad shape', async () => {
    mockRpc({ edb_getConstructorArgs: () => 'not-an-array' });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useConstructorArgs('0x' + '0'.repeat(40)), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
  });

  test('does not fetch when addr is undefined', async () => {
    let calls = 0;
    mockRpc({ edb_getConstructorArgs: () => { calls++; return []; } });
    const { wrapper } = makeWrapper();
    renderHook(() => useConstructorArgs(undefined), { wrapper });
    await new Promise((r) => setTimeout(r, 30));
    expect(calls).toBe(0);
  });
});
