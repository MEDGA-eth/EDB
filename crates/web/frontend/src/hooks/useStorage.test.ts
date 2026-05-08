import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { makeWrapper, mockRpc } from './_test-utils';
import { useStorage } from './useStorage';

describe('useStorage', () => {
  afterEach(() => (globalThis.fetch as { mockReset?: () => void }).mockReset?.());

  test('passes [id, slot] params and returns U256', async () => {
    let received: unknown[] = [];
    mockRpc({ edb_getStorage: (p) => { received = p!; return '0x0'; } });
    const { wrapper } = makeWrapper();
    const slot = '0x' + '0'.repeat(64);
    const { result } = renderHook(() => useStorage(2, slot), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(received).toEqual([2, slot]);
    expect(result.current.data).toBe('0x0');
  });

  test('does not fetch when slot is undefined', async () => {
    let calls = 0;
    mockRpc({ edb_getStorage: () => { calls++; return '0x0'; } });
    const { wrapper } = makeWrapper();
    renderHook(() => useStorage(0, undefined), { wrapper });
    await new Promise((r) => setTimeout(r, 30));
    expect(calls).toBe(0);
  });

  test('does not fetch when id is negative', async () => {
    let calls = 0;
    mockRpc({ edb_getStorage: () => { calls++; return '0x0'; } });
    const { wrapper } = makeWrapper();
    renderHook(() => useStorage(-1, '0x' + '0'.repeat(64)), { wrapper });
    await new Promise((r) => setTimeout(r, 30));
    expect(calls).toBe(0);
  });
});
