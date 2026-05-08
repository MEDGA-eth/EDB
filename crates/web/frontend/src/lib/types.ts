import { z } from 'zod';

/* ── Primitives ───────────────────────────────────────────── */
export const Address = z.string().regex(/^0x[0-9a-fA-F]{40}$/, 'invalid address');
export const Hex = z.string().regex(/^0x[0-9a-fA-F]*$/, 'invalid hex');
export const U256 = z.string().regex(/^(0x[0-9a-fA-F]+|\d+)$/, 'invalid u256');

/* ── Errors ───────────────────────────────────────────────── */
export const ErrorCode = z.union([
  z.literal(-32600),
  z.literal(-32601),
  z.literal(-32602),
  z.literal(-32603),
  z.literal(-33001), // SNAPSHOT_OUT_OF_BOUNDS
  z.literal(-33002), // INVALID_ADDRESS
  z.literal(-33003), // TRACE_ENTRY_NOT_FOUND
  z.literal(-33004), // CODE_NOT_FOUND
  z.literal(-33005), // USID_NOT_FOUND
  z.literal(-33006), // EVAL_FAILED
  z.number(), // future codes
]);

/* ── Snapshots ────────────────────────────────────────────── */
export const ExecutionFrameId = z.object({
  trace_entry_id: z.number().int().nonnegative(),
  step_id: z.number().int().nonnegative(),
});
export type ExecutionFrameId = z.infer<typeof ExecutionFrameId>;

/**
 * Mirrors `OpcodeSnapshotInfoDetail` in `crates/common/src/types/snapshot.rs`.
 * The engine emits this for low-level (opcode) snapshots.
 */
export const OpcodeSnapshotDetail = z.object({
  kind: z.literal('Opcode'),
  id: z.number().int().nonnegative(),
  frame_id: ExecutionFrameId,
  pc: z.number().int().nonnegative(),
  opcode: z.number().int().nonnegative(),
  memory: z.array(z.number()), // bytes
  stack: z.array(U256),
  calldata: z.array(z.number()),
  transient_storage: z.record(z.string(), z.string()),
});
export type OpcodeSnapshotDetail = z.infer<typeof OpcodeSnapshotDetail>;

/**
 * Mirrors `HookSnapshotInfoDetail` in `crates/common/src/types/snapshot.rs`.
 * The engine emits this for source-level (hook) snapshots. Note that
 * `bytecode_address` lives at the top-level `SnapshotInfo`, NOT here, and
 * `usid` is not part of the wire format.
 */
export const HookSnapshotDetail = z.object({
  kind: z.literal('Hook'),
  id: z.number().int().nonnegative(),
  frame_id: ExecutionFrameId,
  /** Source file path where execution is currently positioned. */
  path: z.string(),
  /** Character offset within the source file. */
  offset: z.number().int().nonnegative(),
  /** Length of the current source code segment. */
  length: z.number().int().nonnegative(),
  /** Local variables; engine returns `Option<EdbSolValue>` per name. */
  locals: z.record(z.string(), z.unknown()),
  /** State variables at the bytecode address. */
  state_variables: z.record(z.string(), z.unknown()),
});
export type HookSnapshotDetail = z.infer<typeof HookSnapshotDetail>;

/** Discriminated union over `kind`. */
export const SnapshotInfoDetail = z.discriminatedUnion('kind', [
  OpcodeSnapshotDetail,
  HookSnapshotDetail,
]);
export type SnapshotInfoDetail = z.infer<typeof SnapshotInfoDetail>;

export const SnapshotInfo = z.object({
  id: z.number().int().nonnegative(),
  frame_id: ExecutionFrameId,
  next_id: z.number().int().nonnegative(),
  prev_id: z.number().int().nonnegative(),
  target_address: Address,
  bytecode_address: Address,
  detail: SnapshotInfoDetail,
});
export type SnapshotInfo = z.infer<typeof SnapshotInfo>;

/* ── Trace ────────────────────────────────────────────────── */
export const TraceEntry: z.ZodType<unknown> = z.lazy(() =>
  z.object({
    id: z.number().int().nonnegative(),
    kind: z.string(),
    code_address: Address,
    target_address: Address,
    children: z.array(TraceEntry).default([]),
  }),
);
export const Trace = z.array(TraceEntry);

/* ── Code ─────────────────────────────────────────────────── */
export const SourceFile = z.object({
  path: z.string(),
  content: z.string(),
});

export const CodeKind = z.union([
  z.object({ kind: z.literal('Opcodes'), disasm: z.string() }),
  z.object({ kind: z.literal('Source'), files: z.array(SourceFile), entry: z.string() }),
]);
export type CodeKind = z.infer<typeof CodeKind>;

/* ── ABI ──────────────────────────────────────────────────── */
export const AbiEntry = z.object({
  type: z.string(),
  name: z.string().optional(),
  inputs: z.array(z.unknown()).optional(),
  outputs: z.array(z.unknown()).optional(),
  stateMutability: z.string().optional(),
}).passthrough();
export const Abi = z.array(AbiEntry);

export const CallableAbi = z.object({
  is_proxy: z.boolean().optional(),
  normal: Abi.optional(),
  proxy: Abi.optional(),
  implementation: Abi.optional(),
}).passthrough();

/* ── Storage ──────────────────────────────────────────────── */
export const StorageDiffEntry = z.object({
  slot: U256,
  before: U256.nullable(),
  after: U256.nullable(),
});
export const StorageDiff = z.array(StorageDiffEntry);

/* ── Breakpoints ──────────────────────────────────────────── */
export const BreakpointLocation = z.union([
  z.object({ kind: z.literal('Opcode'), bytecode_address: Address, pc: z.number().int().nonnegative() }),
  z.object({
    kind: z.literal('Source'),
    bytecode_address: Address,
    file_path: z.string(),
    line_number: z.number().int().nonnegative(),
  }),
]);
export type BreakpointLocation = z.infer<typeof BreakpointLocation>;

export const Breakpoint = z.object({
  loc: BreakpointLocation.nullable(),
  condition: z.string().nullable(),
});
export type Breakpoint = z.infer<typeof Breakpoint>;

/* ── Eval ─────────────────────────────────────────────────── */
export const EvalResult = z.unknown(); // engine returns rich SolValue; treat opaque for now

/* ── Health ───────────────────────────────────────────────── */
export const Health = z.object({
  status: z.string(),
  service: z.string().optional(),
  version: z.string().optional(),
});
