import { afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { renderHook, waitFor } from '@testing-library/react';
import { useHealthcheck } from './useHealthcheck';
import { useSession } from '../store/session';

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
    renderHook(() => useHealthcheck());
    await waitFor(() => expect(useSession.getState().connection).toBe('connected'));
  });

  test('marks offline and ends session after 3 consecutive misses', async () => {
    mockFetch([{ ok: false }, { ok: false }, { ok: false }, { ok: false }]);
    renderHook(() => useHealthcheck());
    await waitFor(() => expect(useSession.getState().sessionEnded).toBe(true), { timeout: 8000 });
  });
});
