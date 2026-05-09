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

/* ── ExecutionFrameId ──────────────────────────────────────
 * `ExecutionFrameId(usize, usize)` is a tuple struct in Rust which serde
 * serialises as a 2-element JSON array: `[trace_entry_id, re_entry_count]`.
 * We keep the wire format as a tuple but expose helpers for callsites
 * that want named fields.
 */
export const ExecutionFrameId = z.tuple([
  z.number().int().nonnegative(),
  z.number().int().nonnegative(),
]);
export type ExecutionFrameId = z.infer<typeof ExecutionFrameId>;
export const traceEntryIdOf = (f: ExecutionFrameId): number => f[0];
export const reEntryCountOf = (f: ExecutionFrameId): number => f[1];

/* ── SolValue ─────────────────────────────────────────────
 * Mirrors `SerializedDynSolValue` in `crates/common/src/types/sol_value.rs`.
 * The Rust enum uses `#[serde(tag = "type", content = "value")]` so the
 * wire format is `{ type, value }` where the shape of `value` depends
 * on the variant. We use a discriminated union on `type`.
 *
 * Recursive variants (Array / FixedArray / Tuple / CustomStruct.tuple)
 * require `z.lazy` plus a manual `z.ZodType<SolValue>` annotation.
 */
export type SolValue =
  | { type: 'Bool'; value: boolean }
  | { type: 'Int'; value: { value: string; bits: number } }
  | { type: 'Uint'; value: { value: string; bits: number } }
  | { type: 'FixedBytes'; value: { value: string; size: number } }
  | { type: 'Address'; value: string }
  | { type: 'Function'; value: string }
  | { type: 'Bytes'; value: number[] }
  | { type: 'String'; value: string }
  | { type: 'Array'; value: SolValue[] }
  | { type: 'FixedArray'; value: SolValue[] }
  | { type: 'Tuple'; value: SolValue[] }
  | { type: 'CustomStruct'; value: { name: string; prop_names: string[]; tuple: SolValue[] } };

export const SolValue: z.ZodType<SolValue> = z.lazy(() =>
  z.discriminatedUnion('type', [
    z.object({ type: z.literal('Bool'), value: z.boolean() }),
    z.object({
      type: z.literal('Int'),
      value: z.object({ value: z.string(), bits: z.number().int().nonnegative() }),
    }),
    z.object({
      type: z.literal('Uint'),
      value: z.object({ value: z.string(), bits: z.number().int().nonnegative() }),
    }),
    z.object({
      type: z.literal('FixedBytes'),
      value: z.object({ value: z.string(), size: z.number().int().nonnegative() }),
    }),
    z.object({ type: z.literal('Address'), value: Address }),
    z.object({ type: z.literal('Function'), value: z.string() }),
    z.object({ type: z.literal('Bytes'), value: z.array(z.number()) }),
    z.object({ type: z.literal('String'), value: z.string() }),
    z.object({ type: z.literal('Array'), value: z.array(SolValue) }),
    z.object({ type: z.literal('FixedArray'), value: z.array(SolValue) }),
    z.object({ type: z.literal('Tuple'), value: z.array(SolValue) }),
    z.object({
      type: z.literal('CustomStruct'),
      value: z.object({
        name: z.string(),
        prop_names: z.array(z.string()),
        tuple: z.array(SolValue),
      }),
    }),
  ]),
);

/**
 * Render a `SolValue` to a short, human-readable string. Best-effort —
 * complex nested structures truncate. Used by the terminal panel and the
 * vars view, which previously dumped the entire JSON envelope.
 */
export function formatSolValue(v: SolValue | null | undefined): string {
  if (v === null || v === undefined) return '∅';
  switch (v.type) {
    case 'Bool': return v.value ? 'true' : 'false';
    case 'Int':
    case 'Uint': return v.value.value;
    case 'FixedBytes': return v.value.value;
    case 'Address': return v.value;
    case 'Function': return v.value;
    case 'Bytes': {
      const hex = v.value.map((b) => b.toString(16).padStart(2, '0')).join('');
      return `0x${hex}`;
    }
    case 'String': return JSON.stringify(v.value);
    case 'Array':
    case 'FixedArray':
    case 'Tuple': {
      const items = v.value.slice(0, 8).map(formatSolValue);
      const more = v.value.length > 8 ? `, …${v.value.length - 8} more` : '';
      const open = v.type === 'Tuple' ? '(' : '[';
      const close = v.type === 'Tuple' ? ')' : ']';
      return `${open}${items.join(', ')}${more}${close}`;
    }
    case 'CustomStruct': {
      const s = v.value;
      const fields = s.tuple
        .slice(0, 6)
        .map((f, i) => `${s.prop_names[i] ?? i}: ${formatSolValue(f)}`)
        .join(', ');
      const more = s.tuple.length > 6 ? `, …${s.tuple.length - 6} more` : '';
      return `${s.name}{${fields}${more}}`;
    }
  }
}

/* ── Snapshots ────────────────────────────────────────────── */
/**
 * Mirrors `OpcodeSnapshotInfoDetail` in `crates/common/src/types/snapshot.rs`.
 *
 * Memory is `Vec<u8>` → JSON array of numbers; stack is `Vec<U256>` → array
 * of u256 hex strings; calldata is `Bytes` → hex string; transient_storage
 * is keyed by `addr:slot` strings via a custom serde module.
 */
const OpcodeSnapshotDetailRaw = z.object({
  id: z.number().int().nonnegative(),
  frame_id: ExecutionFrameId,
  pc: z.number().int().nonnegative(),
  opcode: z.number().int().nonnegative(),
  memory: z.array(z.number()),
  stack: z.array(U256),
  calldata: Hex,
  transient_storage: z.record(z.string(), U256),
});

/**
 * Mirrors `HookSnapshotInfoDetail`. Locals and state_variables are
 * `HashMap<String, Option<Arc<EdbSolValue>>>` so each value is either
 * a `SolValue` or `null`.
 */
const HookSnapshotDetailRaw = z.object({
  id: z.number().int().nonnegative(),
  frame_id: ExecutionFrameId,
  path: z.string(),
  offset: z.number().int().nonnegative(),
  length: z.number().int().nonnegative(),
  locals: z.record(z.string(), SolValue.nullable()),
  state_variables: z.record(z.string(), SolValue.nullable()),
});

export type OpcodeSnapshotDetail = z.infer<typeof OpcodeSnapshotDetailRaw> & { kind: 'Opcode' };
export type HookSnapshotDetail = z.infer<typeof HookSnapshotDetailRaw> & { kind: 'Hook' };
export type SnapshotInfoDetail = OpcodeSnapshotDetail | HookSnapshotDetail;

/**
 * Engine emits the externally-tagged form `{ Opcode: {...} }` or
 * `{ Hook: {...} }`. Transform at parse-time into `{ kind: 'Opcode', ... }`
 * so consumers can write `if (detail.kind === 'Hook') {…}`.
 */
export const SnapshotInfoDetail = z.union([
  z.object({ Opcode: OpcodeSnapshotDetailRaw }).transform(
    (d): OpcodeSnapshotDetail => ({ kind: 'Opcode', ...d.Opcode }),
  ),
  z.object({ Hook: HookSnapshotDetailRaw }).transform(
    (d): HookSnapshotDetail => ({ kind: 'Hook', ...d.Hook }),
  ),
]);

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
/**
 * `CallType` is `Call(CallScheme) | Create(CreateScheme)`. CallScheme is a
 * unit enum (Call/CallCode/DelegateCall/StaticCall) so it serialises to a
 * bare string. CreateScheme has variants `Create`, `Create2 { salt }`,
 * `Custom { … }` so it can be a string OR an object.
 */
const CallSchemeWire = z.enum(['Call', 'CallCode', 'DelegateCall', 'StaticCall']);
const CreateSchemeWire = z.union([
  z.literal('Create'),
  z.object({ Create2: z.object({ salt: U256 }) }).passthrough(),
  z.object({ Custom: z.unknown() }).passthrough(),
]);

export type CallType =
  | { kind: 'Call'; scheme: 'Call' | 'CallCode' | 'DelegateCall' | 'StaticCall' }
  | { kind: 'Create'; scheme: unknown };

export const CallType = z.union([
  z.object({ Call: CallSchemeWire }).transform((d): CallType => ({ kind: 'Call', scheme: d.Call })),
  z.object({ Create: CreateSchemeWire }).transform(
    (d): CallType => ({ kind: 'Create', scheme: d.Create }),
  ),
]);

const InstructionResult = z.string();

/**
 * `CallResult = Success | Revert | Error`, externally tagged. Each variant
 * carries `output: Bytes` (hex) and `result: InstructionResult` (string).
 */
export type CallResult =
  | { kind: 'Success'; output: string; result: string }
  | { kind: 'Revert'; output: string; result: string }
  | { kind: 'Error'; output: string; result: string };

export const CallResult = z.union([
  z.object({ Success: z.object({ output: Hex, result: InstructionResult }) }).transform(
    (d): CallResult => ({ kind: 'Success', ...d.Success }),
  ),
  z.object({ Revert: z.object({ output: Hex, result: InstructionResult }) }).transform(
    (d): CallResult => ({ kind: 'Revert', ...d.Revert }),
  ),
  z.object({ Error: z.object({ output: Hex, result: InstructionResult }) }).transform(
    (d): CallResult => ({ kind: 'Error', ...d.Error }),
  ),
]);

/** Ethereum log event as emitted by alloy `LogData`. */
export const LogData = z.object({
  topics: z.array(Hex),
  data: Hex,
});

/**
 * Mirrors `TraceEntry`. Note: the trace is FLAT (`{ inner: TraceEntry[] }`)
 * with `parent_id`/`depth` for relationships. Consumers must rebuild the
 * tree at render time.
 */
export const TraceEntry = z.object({
  id: z.number().int().nonnegative(),
  parent_id: z.number().int().nonnegative().nullable(),
  depth: z.number().int().nonnegative(),
  call_type: CallType,
  caller: Address,
  target: Address,
  code_address: Address,
  input: Hex,
  value: U256,
  result: CallResult.nullable(),
  created_contract: z.boolean(),
  create_scheme: CreateSchemeWire.nullable(),
  bytecode: Hex.nullable(),
  target_label: z.string().nullable(),
  self_destruct: z.tuple([Address, U256]).nullable(),
  events: z.array(LogData),
  first_snapshot_id: z.number().int().nonnegative().nullable(),
});
export type TraceEntry = z.infer<typeof TraceEntry>;

/** Top-level trace shape: `{ inner: TraceEntry[] }`. */
export const Trace = z.object({
  inner: z.array(TraceEntry),
});
export type Trace = z.infer<typeof Trace>;

/* ── Code ─────────────────────────────────────────────────── */
/** A single source file as `{ path, content }` for view-side ergonomics. */
export const SourceFile = z.object({
  path: z.string(),
  content: z.string(),
});
export type SourceFile = z.infer<typeof SourceFile>;

/**
 * Mirrors `Code = Opcode(OpcodeInfo) | Source(SourceInfo)`.
 *
 * - `OpcodeInfo` has `bytecode_address` and `codes: HashMap<usize, String>`
 *   (pc → mnemonic). We transform that map into a sorted disasm string
 *   (`PC: opcode\n…`) plus the raw map for callsites that need it.
 * - `SourceInfo` has `sources: HashMap<PathBuf, String>` (path → content).
 *   We transform into `files: SourceFile[]` and pick the longest-path file
 *   as a default `entry`.
 */
const OpcodeInfoRaw = z.object({
  bytecode_address: Address,
  codes: z.record(z.string(), z.string()),
});

const SourceInfoRaw = z.object({
  bytecode_address: Address,
  sources: z.record(z.string(), z.string()),
});

export type OpcodeCode = {
  kind: 'Opcodes';
  bytecode_address: string;
  /** Map of pc-string → mnemonic. */
  codes: Record<string, string>;
  /** Pre-rendered, line-sorted disasm string. */
  disasm: string;
};

export type SourceCode = {
  kind: 'Source';
  bytecode_address: string;
  files: SourceFile[];
  /** Best-guess entry file (largest content) — used by editors as default tab. */
  entry: string;
};

export type CodeKind = OpcodeCode | SourceCode;

function disasmFromCodes(codes: Record<string, string>): string {
  const rows = Object.entries(codes)
    .map(([pc, op]) => [Number(pc), op] as const)
    .filter(([n]) => Number.isFinite(n))
    .sort((a, b) => a[0] - b[0]);
  return rows
    .map(([pc, op]) => `${pc.toString(16).padStart(4, '0')}: ${op}`)
    .join('\n');
}

function pickEntry(files: SourceFile[]): string {
  if (files.length === 0) return '';
  // Pick the file with the largest content — usually the entry contract.
  let best = files[0];
  for (const f of files) {
    if (f.content.length > best.content.length) best = f;
  }
  return best.path;
}

export const CodeKind = z.union([
  z.object({ Opcode: OpcodeInfoRaw }).transform((d): OpcodeCode => ({
    kind: 'Opcodes',
    bytecode_address: d.Opcode.bytecode_address,
    codes: d.Opcode.codes,
    disasm: disasmFromCodes(d.Opcode.codes),
  })),
  z.object({ Source: SourceInfoRaw }).transform((d): SourceCode => {
    const files: SourceFile[] = Object.entries(d.Source.sources).map(([path, content]) => ({
      path,
      content,
    }));
    return {
      kind: 'Source',
      bytecode_address: d.Source.bytecode_address,
      files,
      entry: pickEntry(files),
    };
  }),
]);

/* ── ABI ──────────────────────────────────────────────────── */
export const AbiEntry = z.object({
  type: z.string(),
  name: z.string().optional(),
  inputs: z.array(z.unknown()).optional(),
  outputs: z.array(z.unknown()).optional(),
  stateMutability: z.string().optional(),
}).passthrough();
export const Abi = z.array(AbiEntry);

/**
 * `AbiEntryTy = Function | StateVariable(u64)` — externally tagged. The
 * `StateVariable` variant carries a u64 payload (`{ StateVariable: 42 }`).
 */
const AbiEntryTy = z.union([
  z.literal('Function'),
  z.object({ StateVariable: z.number() }).passthrough(),
]);

export const CallableAbiEntry = z.object({
  name: z.string(),
  ty: AbiEntryTy,
  inputs: z.array(z.string()),
  outputs: z.array(z.string()),
  abi: AbiEntry,
});

export const ContractTy = z.enum(['Normal', 'Proxy', 'Implementation']);
export type ContractTy = z.infer<typeof ContractTy>;

/**
 * `getCallableABI` returns `Vec<CallableAbiInfo>` — one entry per related
 * contract. For proxies you get the proxy + its implementations together.
 */
export const CallableAbiInfo = z.object({
  address: Address,
  contract_ty: ContractTy,
  entries: z.array(CallableAbiEntry),
});
export const CallableAbi = z.array(CallableAbiInfo);
export type CallableAbi = z.infer<typeof CallableAbi>;

/* ── Storage ──────────────────────────────────────────────── */
/**
 * `getStorageDiff` returns `HashMap<U256, (U256, U256)>` — slot → (before, after)
 * encoded as map keyed by stringified U256 with tuple values.
 */
export const StorageDiff = z.record(
  z.string(),
  z.tuple([U256, U256]),
);
export type StorageDiff = z.infer<typeof StorageDiff>;

/** Convenience row for view consumers — flattens the map into an array. */
export interface StorageDiffRow {
  slot: string;
  before: string;
  after: string;
}

export function storageDiffRows(diff: StorageDiff): StorageDiffRow[] {
  return Object.entries(diff).map(([slot, [before, after]]) => ({ slot, before, after }));
}

/* ── Constructor args ─────────────────────────────────────── */
/** `getConstructorArgs` returns `Option<Bytes>` — a hex string or null. */
export const ConstructorArgs = Hex.nullable();
export type ConstructorArgs = z.infer<typeof ConstructorArgs>;

/* ── Breakpoints ──────────────────────────────────────────── */
/**
 * Wire format of `BreakpointLocation` is externally tagged:
 *   `{ Source: { bytecode_address, file_path, line_number } }`
 *   `{ Opcode: { bytecode_address, pc } }`
 * For ergonomics we keep our internal representation (used by the session
 * store + UI) discriminated on `kind`. We provide both the wire schema for
 * decoding engine responses (currently unused — engines never *return*
 * breakpoints) and helpers for serialising our internal form to the wire.
 */
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

/** Convert our `kind`-tagged breakpoint to the engine's externally-tagged
 * shape for over-the-wire transmission. */
export function breakpointToWire(bp: Breakpoint): unknown {
  let loc: unknown = null;
  if (bp.loc) {
    if (bp.loc.kind === 'Opcode') {
      loc = { Opcode: { bytecode_address: bp.loc.bytecode_address, pc: bp.loc.pc } };
    } else {
      loc = {
        Source: {
          bytecode_address: bp.loc.bytecode_address,
          file_path: bp.loc.file_path,
          line_number: bp.loc.line_number,
        },
      };
    }
  }
  return { loc, condition: bp.condition };
}

/* ── Eval ─────────────────────────────────────────────────── */
/**
 * `edb_evalOnSnapshot` returns `Result<EdbSolValue, String>` which serde
 * encodes as `{ Ok: SolValue } | { Err: string }`. Transform into a
 * narrower union so consumers can write `if (r.kind === 'Ok')`.
 */
export type EvalResult = { kind: 'Ok'; value: SolValue } | { kind: 'Err'; error: string };
export const EvalResult = z.union([
  z.object({ Ok: SolValue }).transform((d): EvalResult => ({ kind: 'Ok', value: d.Ok })),
  z.object({ Err: z.string() }).transform((d): EvalResult => ({ kind: 'Err', error: d.Err })),
]);

/* ── Health ───────────────────────────────────────────────── */
export const Health = z.object({
  status: z.string(),
  service: z.string().optional(),
  version: z.string().optional(),
});
