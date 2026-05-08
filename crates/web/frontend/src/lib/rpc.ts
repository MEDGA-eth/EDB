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

export async function rpcRaw<T = unknown>(method: string, params?: unknown[]): Promise<T> {
  let res: Response;
  try {
    res = await fetch(ENDPOINT, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', method, params, id: nextId++ }),
    });
  } catch (e) {
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

export async function rpc<T>(method: string, schema: z.ZodType<T>, params?: unknown[]): Promise<T> {
  const raw = await rpcRaw<unknown>(method, params);
  const parsed = schema.safeParse(raw);
  if (!parsed.success) throw new SchemaError(method, parsed.error);
  return parsed.data;
}
