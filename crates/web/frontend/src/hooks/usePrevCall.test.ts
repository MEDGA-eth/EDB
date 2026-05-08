import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { makeWrapper, mockRpc } from './_test-utils';
import { usePrevCall } from './usePrevCall';

describe('usePrevCall', () => {
  afterEach(() => (globalThis.fetch as { mockReset?: () => void }).mockReset?.());

  test('passes id', async () => {
    let received: unknown[] = [];
    mockRpc({ edb_getPrevCall: (p) => { received = p!; return 5; } });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => usePrevCall(2), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(received).toEqual([2]);
  });

  test('returns numeric prev id', async () => {
    mockRpc({ edb_getPrevCall: () => 5 });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => usePrevCall(0), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toBe(5);
  });
});
