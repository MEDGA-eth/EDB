import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { makeWrapper, mockRpc } from './_test-utils';
import { useTrace } from './useTrace';

describe('useTrace', () => {
  afterEach(() => (globalThis.fetch as { mockReset?: () => void }).mockReset?.());

  test('returns trace array', async () => {
    mockRpc({ edb_getTrace: () => [] });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useTrace(), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(Array.isArray(result.current.data)).toBe(true);
  });
});
