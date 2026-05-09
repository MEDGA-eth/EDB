import { afterEach, describe, expect, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { makeWrapper, mockRpc } from './_test-utils';
import { useEvalExpr } from './useEvalExpr';

describe('useEvalExpr', () => {
  afterEach(() => (globalThis.fetch as { mockReset?: () => void }).mockReset?.());

  test('mutates and caches result under [eval, id, expr]', async () => {
    let receivedParams: unknown[] = [];
    mockRpc({ edb_evalOnSnapshot: (p) => { receivedParams = p!; return { Ok: { type: 'Bool', value: true } }; } });
    const { wrapper, client } = makeWrapper();
    const { result } = renderHook(() => useEvalExpr(), { wrapper });
    const out = await result.current.mutateAsync({ id: 1, expr: 'foo' });
    expect(out.kind).toBe('Ok');
    if (out.kind === 'Ok') expect(out.value).toEqual({ type: 'Bool', value: true });
    expect(receivedParams).toEqual([1, 'foo']);
    await waitFor(() =>
      expect(client.getQueryData(['eval', 1, 'foo']) as unknown).toEqual({
        kind: 'Ok',
        value: { type: 'Bool', value: true },
      }),
    );
  });

  test('throws RpcError on engine error', async () => {
    mockRpc({ edb_evalOnSnapshot: () => { const e: Error & { code?: number } = new Error('boom'); e.code = -33006; throw e; } });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useEvalExpr(), { wrapper });
    let err: unknown;
    try { await result.current.mutateAsync({ id: 1, expr: 'foo' }); } catch (e) { err = e; }
    expect(err).toBeTruthy();
  });
});
