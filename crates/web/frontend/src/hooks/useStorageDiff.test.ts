import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { makeWrapper, mockRpc } from './_test-utils';
import { useStorageDiff } from './useStorageDiff';

describe('useStorageDiff', () => {
  afterEach(() => (globalThis.fetch as { mockReset?: () => void }).mockReset?.());

  test('passes id and returns array', async () => {
    let received: unknown[] = [];
    mockRpc({ edb_getStorageDiff: (p) => { received = p!; return []; } });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useStorageDiff(7), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(received).toEqual([7]);
    expect(result.current.data).toEqual([]);
  });

  test('schema error on bad shape', async () => {
    mockRpc({ edb_getStorageDiff: () => 'not-an-array' });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useStorageDiff(0), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
  });

  test('does not fetch when id is negative', async () => {
    let calls = 0;
    mockRpc({ edb_getStorageDiff: () => { calls++; return []; } });
    const { wrapper } = makeWrapper();
    renderHook(() => useStorageDiff(-1), { wrapper });
    await new Promise((r) => setTimeout(r, 30));
    expect(calls).toBe(0);
  });
});
