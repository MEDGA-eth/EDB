import { afterEach, describe, expect, test } from 'bun:test';
import { act, cleanup, renderHook } from '@testing-library/react';
import { useDebounced } from './useDebounced';

function sleep(ms: number) {
  return new Promise((res) => setTimeout(res, ms));
}

describe('useDebounced', () => {
  afterEach(cleanup);

  test('returns the input on first render', () => {
    const { result } = renderHook(() => useDebounced('hello', 100));
    expect(result.current).toBe('hello');
  });

  test('lags behind rapid changes by the delay', async () => {
    const { result, rerender } = renderHook(({ v }: { v: string }) => useDebounced(v, 60), {
      initialProps: { v: 'a' },
    });
    expect(result.current).toBe('a');
    rerender({ v: 'ab' });
    rerender({ v: 'abc' });
    rerender({ v: 'abcd' });
    // Still old value before the delay elapses
    expect(result.current).toBe('a');
    await act(async () => {
      await sleep(120);
    });
    expect(result.current).toBe('abcd');
  });

  test('delayMs <= 0 updates immediately', () => {
    const { result, rerender } = renderHook(({ v }: { v: string }) => useDebounced(v, 0), {
      initialProps: { v: 'a' },
    });
    rerender({ v: 'b' });
    // Effect runs after render flush; trigger it.
    act(() => {});
    expect(result.current).toBe('b');
  });
});
