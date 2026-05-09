import { z } from 'zod';

export class RpcError extends Error {
  constructor(public code: number, message: string, public data?: unknown) {
    super(message);
    this.name = 'RpcError';
  }
}

export class TransportError extends Error {
  constructor(message: string, public cause?: unknown) {
    super(message);
    this.name = 'TransportError';
  }
}

export class SchemaError extends Error {
  constructor(public method: string, public issues: z.ZodError) {
    super(`schema mismatch in ${method}: ${issues.message}`);
    this.name = 'SchemaError';
  }
}

let nextId = 1;
const ENDPOINT = '/';

/** Optional per-call options. */
export interface RpcOptions {
  /**
   * Caller-owned AbortSignal forwarded to `fetch`. When the signal aborts,
   * the call rejects with the underlying `AbortError`. Special-case it on
   * the call-site (e.g., terminal "Cancelled" entry) — don't surface it as
   * a regular RPC error.
   */
  signal?: AbortSignal;
}

/**
 * Raw JSON-RPC call — the response body is **not** validated against any
 * schema; the caller gets back whatever the server returned, typed as `T`.
 *
 * Prefer {@link rpc} for typed flows, which runs a Zod schema on the
 * response and throws {@link SchemaError} on mismatches. Callers that
 * intentionally want unvalidated bytes (e.g. the eval terminal, where
 * the server returns rich SolValues we render opaquely; or the
 * healthcheck probe, which inspects the raw status field directly)
 * may use this entrypoint, but they take on the risk of malformed data.
 */
export async function rpcRaw<T = unknown>(
  method: string,
  params?: unknown[],
  opts?: RpcOptions,
): Promise<T> {
  let res: Response;
  try {
    res = await fetch(ENDPOINT, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', method, params, id: nextId++ }),
      signal: opts?.signal,
    });
  } catch (e) {
    // Preserve AbortError so callers can special-case it.
    if (e instanceof DOMException && e.name === 'AbortError') throw e;
    if (e instanceof Error && e.name === 'AbortError') throw e;
    throw new TransportError(`fetch failed for ${method}`, e);
  }
  if (!res.ok) {
    throw new TransportError(`HTTP ${res.status} for ${method}`);
  }
  let json: { result?: T; error?: { code: number; message: string; data?: unknown } };
  try {
    json = await res.json();
  } catch (e) {
    throw new TransportError(`invalid JSON from ${method}`, e);
  }
  if (json.error) throw new RpcError(json.error.code, json.error.message, json.error.data);
  return json.result as T;
}

/**
 * Typed JSON-RPC call. The schema may be a plain `z.ZodType<T>` or a
 * transforming schema (e.g. `z.object(...).transform(...)`) whose input
 * shape differs from its output — that's why the schema parameter accepts
 * any Zod schema with `output = T` rather than `z.ZodType<T>`, which
 * implicitly requires `input == output`.
 */
export async function rpc<T>(
  method: string,
  schema: z.ZodSchema<T, z.ZodTypeDef, unknown>,
  params?: unknown[],
  opts?: RpcOptions,
): Promise<T> {
  const raw = await rpcRaw<unknown>(method, params, opts);
  const parsed = schema.safeParse(raw);
  if (!parsed.success) throw new SchemaError(method, parsed.error);
  return parsed.data;
}
