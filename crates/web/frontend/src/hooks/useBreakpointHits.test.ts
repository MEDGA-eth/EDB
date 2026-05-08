import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { makeWrapper, mockRpc } from './_test-utils';
import { useBreakpointHits } from './useBreakpointHits';
import type { Breakpoint } from '../lib/types';

const addr = '0x' + '0'.repeat(40);
const bpA: Breakpoint = {
  loc: { kind: 'Opcode', bytecode_address: addr, pc: 12 },
  condition: null,
};
const bpB: Breakpoint = {
  loc: { kind: 'Opcode', bytecode_address: addr, pc: 99 },
  condition: 'x > 0',
};

describe('useBreakpointHits', () => {
  afterEach(() => (globalThis.fetch as { mockReset?: () => void }).mockReset?.());

  test('returns hits array', async () => {
    mockRpc({ edb_getBreakpointHits: () => [1, 5, 9] });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useBreakpointHits(bpA), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual([1, 5, 9]);
  });

  test('disabled when bp is null', async () => {
    let calls = 0;
    mockRpc({ edb_getBreakpointHits: () => { calls++; return []; } });
    const { wrapper } = makeWrapper();
    renderHook(() => useBreakpointHits(null), { wrapper });
    await new Promise((r) => setTimeout(r, 30));
    expect(calls).toBe(0);
  });

  test('reruns when bp shape changes', async () => {
    let calls = 0;
    mockRpc({ edb_getBreakpointHits: () => { calls++; return [calls]; } });
    const { wrapper } = makeWrapper();
    const { result, rerender } = renderHook(
      ({ bp }: { bp: Breakpoint }) => useBreakpointHits(bp),
      { wrapper, initialProps: { bp: bpA } },
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls).toBe(1);
    rerender({ bp: bpB });
    await waitFor(() => expect(calls).toBe(2));
  });
});
