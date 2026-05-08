import snapshotCount from './snapshotCount.json';
import trace from './trace.json';
import snapshotInfo0 from './snapshotInfo-0.json';
import code0 from './code-0.json';
import storageDiff0 from './storageDiff-0.json';

export const MOCKS: Record<string, (params?: unknown[]) => unknown> = {
  edb_getSnapshotCount: () => snapshotCount,
  edb_getTrace: () => trace,
  edb_getSnapshotInfo: (params) => {
    const [id] = (params ?? []) as [number];
    return id === 0 ? snapshotInfo0 : null;
  },
  edb_getCode: (params) => {
    const [id] = (params ?? []) as [number];
    return id === 0 ? code0 : null;
  },
  edb_getStorageDiff: () => storageDiff0,
  edb_evalOnSnapshot: (params) => {
    const [id, expr] = (params ?? []) as [number, string];
    return { kind: 'Mock', id, expr };
  },
  edb_getNextCall: () => 1,
  edb_getPrevCall: () => 0,
  edb_getBreakpointHits: () => [],
  edb_getContractABI: () => [],
  edb_getCallableABI: () => ({ is_proxy: false, normal: [] }),
  edb_getConstructorArgs: () => [],
  edb_getCodeByAddress: () => code0,
  edb_getStorage: () => '0x0',
};
