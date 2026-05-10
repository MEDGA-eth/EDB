import snapshotCount from './captures/snapshotCount.json';
import snapshotInfo0 from './captures/snapshotInfo-0.json';
import snapshotInfo150 from './captures/snapshotInfo-150.json';
import trace from './captures/trace.json';
import code0 from './captures/code-0.json';
import code150 from './captures/code-150.json';
import codeByRouter from './captures/code-by-router.json';
import storage from './captures/storage-100-slot0.json';
import storageDiff from './captures/storageDiff-100.json';
import contractABI from './captures/contractABI-router.json';
import callableABI from './captures/callableABI-router.json';
import constructorArgs from './captures/constructorArgs-router.json';
import evalResult from './captures/eval-1plus1.json';
import breakpointHits from './captures/breakpointHits-empty.json';

/**
 * Mock fixtures used by the dev server (`bun run dev --mode=mock`) and the
 * Playwright e2e suite. These are real captures, extracted via
 * `edb --ui=web replay` against a Uniswap V2 swap (tx 0x608d…e4c2), minus
 * the JSON-RPC envelope. The vite mock middleware in `vite.config.ts` wraps
 * each handler return value in `{ jsonrpc, id, result }`.
 *
 * Handlers that vary on params do simple parameter-aware dispatch and fall
 * back to a representative capture otherwise; handlers that don't (ABIs,
 * eval, breakpoint hits) just return their single capture.
 */

/* Router address present in the captures. */
const ROUTER_ADDR = '0x7a250d5630b4cf539739df2c5dacb4c659f2488d';
/* Pair address present in the captures (snapshot 150's bytecode_address). */
const PAIR_ADDR = '0x6b4207a321319d5740d2375471953ac95d803598';

type Address = string;
type SnapshotInfo = typeof snapshotInfo0 & {
  detail: { Hook: typeof snapshotInfo0.detail.Hook };
};

/**
 * Synthesize a SnapshotInfo for an arbitrary id by cloning snapshot 0 and
 * patching the navigation fields. Used for ids without a dedicated capture.
 */
function fallbackSnapshotInfo(id: number, total: number): SnapshotInfo {
  const base = snapshotInfo0 as SnapshotInfo;
  return {
    ...base,
    id,
    next_id: Math.min(total - 1, id + 1),
    prev_id: Math.max(0, id - 1),
    detail: { Hook: { ...base.detail.Hook, id } },
  };
}

/**
 * Tiny Opcodes-shaped stub for any address that doesn't have a captured
 * `edb_getCodeByAddress` response. Keeps the file explorer functional for
 * the 3 trace-internal addresses (WETH, pair, intermediate) without a
 * 35KB source dump per address.
 */
function opcodeStub(addr: Address): unknown {
  return {
    Opcode: {
      bytecode_address: addr,
      codes: {
        0: 'PUSH1 0x80',
        2: 'PUSH1 0x40',
        4: 'MSTORE',
        5: 'CALLVALUE',
        6: 'DUP1',
        7: 'ISZERO',
      },
    },
  };
}

export const MOCKS: Record<string, (params?: unknown[]) => unknown> = {
  edb_getSnapshotCount: () => snapshotCount,
  edb_getTrace: () => trace,

  edb_getSnapshotInfo: (params) => {
    const [id] = (params ?? []) as [number];
    const requested = typeof id === 'number' ? id : 0;
    if (requested === 150) return snapshotInfo150;
    if (requested === 0) return snapshotInfo0;
    return fallbackSnapshotInfo(requested, snapshotCount as unknown as number);
  },

  edb_getCode: (params) => {
    const [id] = (params ?? []) as [number];
    // Snapshot 150 lives in the pair contract (different source); for any
    // id >= 1 default to its source so the editor picks up something more
    // interesting than just the router on snapshot 0. Snapshot 0 stays on
    // the router source.
    if (typeof id === 'number' && id === 0) return code0;
    if (typeof id === 'number' && id >= 1) return code150;
    return code0;
  },

  edb_getCodeByAddress: (params) => {
    const [addr] = (params ?? []) as [string];
    const a = (addr ?? '').toLowerCase();
    if (a === ROUTER_ADDR) return codeByRouter;
    if (a === PAIR_ADDR) return code150;
    // WETH / intermediate addresses don't have a dedicated capture; serve a
    // tiny Opcodes stub so the explorer can still expand them as <disasm>.
    return opcodeStub(a);
  },

  edb_getStorage: () => storage,
  edb_getStorageDiff: () => storageDiff,

  edb_getNextCall: (params) => {
    const [id] = (params ?? []) as [number];
    const cur = typeof id === 'number' ? id : 0;
    // Real engine returned 5 for snapshot 0; keep that shape, every 5
    // snapshots advance one "call", capped at the last id (195).
    return cur <= 190 ? cur + 5 : cur;
  },
  edb_getPrevCall: (params) => {
    const [id] = (params ?? []) as [number];
    const cur = typeof id === 'number' ? id : 0;
    return Math.max(0, cur - 5);
  },

  edb_getContractABI: () => contractABI,
  edb_getCallableABI: () => callableABI,
  edb_getConstructorArgs: () => constructorArgs,

  edb_evalOnSnapshot: () => evalResult,
  edb_getBreakpointHits: () => breakpointHits,
};
