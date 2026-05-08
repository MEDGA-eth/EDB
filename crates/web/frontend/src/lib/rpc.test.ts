import { afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { z } from 'zod';
import { RpcError, SchemaError, TransportError, rpc, rpcRaw } from './rpc';

const okSchema = z.object({ snapshot: z.number() });

function mockFetch(impl: (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>) {
  globalThis.fetch = mock(impl as never) as unknown as typeof fetch;
}

describe('rpc', () => {
  let calls: Array<{ url: string; body: unknown }>;
  beforeEach(() => { calls = []; });
  afterEach(() => { (globalThis.fetch as { mockReset?: () => void }).mockReset?.(); });

  test('sends a JSON-RPC 2.0 envelope with method and params', async () => {
    mockFetch(async (url, init) => {
      calls.push({ url: String(url), body: JSON.parse(init!.body as string) });
      return new Response(JSON.stringify({ jsonrpc: '2.0', id: 1, result: { snapshot: 42 } }), {
        status: 200, headers: { 'content-type': 'application/json' },
      });
    });

    const out = await rpc('edb_getSnapshotInfo', okSchema, [42]);
    expect(out).toEqual({ snapshot: 42 });
    expect(calls).toHaveLength(1);
    const body = calls[0].body as { jsonrpc: string; method: string; params: unknown[] };
    expect(body.jsonrpc).toBe('2.0');
    expect(body.method).toBe('edb_getSnapshotInfo');
    expect(body.params).toEqual([42]);
  });

  test('throws RpcError when server returns error', async () => {
    mockFetch(async () =>
      new Response(JSON.stringify({ jsonrpc: '2.0', id: 1, error: { code: -33001, message: 'oob' } })),
    );
    let err: unknown;
    try { await rpcRaw('edb_x'); } catch (e) { err = e; }
    expect(err).toBeInstanceOf(RpcError);
    expect((err as RpcError).code).toBe(-33001);
  });

  test('throws TransportError when fetch throws', async () => {
    mockFetch(async () => { throw new Error('connection refused'); });
    let err: unknown;
    try { await rpcRaw('edb_x'); } catch (e) { err = e; }
    expect(err).toBeInstanceOf(TransportError);
  });

  test('throws TransportError on non-200 status', async () => {
    mockFetch(async () => new Response('boom', { status: 502 }));
    let err: unknown;
    try { await rpcRaw('edb_x'); } catch (e) { err = e; }
    expect(err).toBeInstanceOf(TransportError);
  });

  test('throws SchemaError when payload does not match schema', async () => {
    mockFetch(async () =>
      new Response(JSON.stringify({ jsonrpc: '2.0', id: 1, result: { wrong: true } })),
    );
    let err: unknown;
    try { await rpc('edb_getSnapshotInfo', okSchema); } catch (e) { err = e; }
    expect(err).toBeInstanceOf(SchemaError);
  });
});
