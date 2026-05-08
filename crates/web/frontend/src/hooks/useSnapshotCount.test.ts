import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { makeWrapper, mockRpc } from './_test-utils';
import { useSnapshotCount } from './useSnapshotCount';

describe('useSnapshotCount', () => {
  afterEach(() => (globalThis.fetch as { mockReset?: () => void }).mockReset?.());

  test('returns count on success', async () => {
    mockRpc({ edb_getSnapshotCount: () => 42 });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useSnapshotCount(), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toBe(42);
  });

  test('throws SchemaError on shape mismatch', async () => {
    mockRpc({ edb_getSnapshotCount: () => 'oops' });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useSnapshotCount(), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
  });

  test('caches across rerenders', async () => {
    let calls = 0;
    mockRpc({
      edb_getSnapshotCount: () => {
        calls++;
        return 1;
      },
    });
    const { wrapper } = makeWrapper();
    const { result, rerender } = renderHook(() => useSnapshotCount(), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    rerender();
    expect(calls).toBe(1);
  });
});
