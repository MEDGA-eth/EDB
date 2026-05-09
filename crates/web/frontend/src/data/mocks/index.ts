import snapshotCount from './snapshotCount.json';
import trace from './trace.json';
import snapshotInfo0 from './snapshotInfo-0.json';
import code0 from './code-0.json';
import storageDiff0 from './storageDiff-0.json';

/**
 * Mock fixtures used by the dev server and offline tests. All shapes here
 * mirror the engine's actual JSON wire format (externally-tagged enums,
 * `{ inner: TraceEntry[] }`, etc.) so they parse through the same Zod
 * schemas as a live response.
 */
export const MOCKS: Record<string, (params?: unknown[]) => unknown> = {
  edb_getSnapshotCount: () => snapshotCount,
  edb_getTrace: () => trace,
  edb_getSnapshotInfo: (params) => {
    const [id] = (params ?? []) as [number];
    // Only snapshot 0 has a fixture; for any other id, return a clone with
    // the requested id so navigation still works in the dev mock server.
    const base = snapshotInfo0 as { id: number; next_id: number; prev_id: number; detail: { Hook: { id: number } } };
    const requested = typeof id === 'number' ? id : 0;
    return {
      ...base,
      id: requested,
      next_id: requested + 1,
      prev_id: Math.max(0, requested - 1),
      detail: { Hook: { ...base.detail.Hook, id: requested } },
    };
  },
  edb_getCode: () => code0,
  edb_getStorageDiff: () => storageDiff0,
  edb_evalOnSnapshot: (params) => {
    const [, expr] = (params ?? []) as [number, string];
    return { Ok: { type: 'String', value: `mock(${expr})` } };
  },
  edb_getNextCall: () => 1,
  edb_getPrevCall: () => 0,
  edb_getBreakpointHits: () => [],
  edb_getContractABI: () => [],
  edb_getCallableABI: () => [],
  edb_getConstructorArgs: () => null,
  edb_getCodeByAddress: () => code0,
  edb_getStorage: () =>
    '0x0000000000000000000000000000000000000000000000000000000000000000',
};
