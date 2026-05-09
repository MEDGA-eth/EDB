import { afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { useHealthcheck } from './useHealthcheck';
import { useSession } from '../store/session';
import { makeWrapper } from './_test-utils';

function mockFetch(seq: Array<{ ok: boolean }>) {
  let i = 0;
  globalThis.fetch = mock(async () => {
    const r = seq[Math.min(i, seq.length - 1)]; i++;
    return r.ok
      ? new Response(JSON.stringify({ status: 'healthy' }))
      : new Response('boom', { status: 503 });
  }) as unknown as typeof fetch;
}

describe('useHealthcheck', () => {
  beforeEach(() => useSession.getState().setSessionEnded(false));
  afterEach(() => (globalThis.fetch as { mockReset?: () => void }).mockReset?.());

  test('marks connected on first success', async () => {
    mockFetch([{ ok: true }]);
    const { wrapper } = makeWrapper();
    renderHook(() => useHealthcheck(), { wrapper });
    await waitFor(() => expect(useSession.getState().connection).toBe('connected'));
  });

  test('marks offline and ends session after 3 consecutive misses', async () => {
    mockFetch([{ ok: false }, { ok: false }, { ok: false }, { ok: false }]);
    const { wrapper } = makeWrapper();
    renderHook(() => useHealthcheck(), { wrapper });
    await waitFor(() => expect(useSession.getState().sessionEnded).toBe(true), { timeout: 8000 });
  });

  test('cancels in-flight queries but preserves cache on false→true sessionEnded edge', async () => {
    mockFetch([{ ok: true }]);
    const { wrapper, client } = makeWrapper();
    // Pre-seed cache: this is intentionally NOT evicted on a transient
    // flap so a reconnect doesn't have to refetch everything from scratch.
    client.setQueryData(['snapshot', 0], 'cached-value');
    expect(client.getQueryData<string>(['snapshot', 0])).toBe('cached-value');
    const cancelSpy = mock(() => Promise.resolve());
    const origCancel = client.cancelQueries.bind(client);
    client.cancelQueries = ((...args: unknown[]) => {
      cancelSpy();
      return origCancel(...(args as Parameters<typeof origCancel>));
    }) as typeof client.cancelQueries;
    renderHook(() => useHealthcheck(), { wrapper });
    await waitFor(() => expect(useSession.getState().connection).toBe('connected'));
    useSession.getState().setSessionEnded(true);
    await waitFor(() => expect(cancelSpy).toHaveBeenCalled());
    // Cache value survives the flap.
    expect(client.getQueryData<string>(['snapshot', 0])).toBe('cached-value');
  });
});
