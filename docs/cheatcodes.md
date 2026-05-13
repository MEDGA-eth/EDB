# `edb test` Cheatcodes

This page is for users debugging Foundry tests with `edb test`. It tells you:

- which cheatcodes EDB supports,
- which it ships as best-effort partials (and how to work around the gaps),
- which it explicitly rejects (and why), and
- how to read the error messages EDB produces.

For the architectural deep-dive on EDB's hand-rolled cheatcode design (why
we didn't embed `foundry-cheatcodes`, the multi-pass instrumentation
constraints, etc.), see
`docs/superpowers/specs/2026-05-10-edb-test-design.md`.

## Real-world coverage at a glance

The script `scripts/edb-static-cheatcode-coverage.py` walks the vendored
Foundry projects in `testdata/foundry-e2e/` and reports, for each test
function, whether every `vm.*` cheatcode it (or anything else in its
file) invokes is in EDB's supported catalog. A test is "eligible" when
all cheatcodes it touches are supported — meaning `edb test` will NOT
abort with an unsupported-cheatcode error before launching the UI
(execution-time behavior, e.g. instrumented-bytecode divergence, is a
separate concern).

| Project | Total tests | Eligible | % |
|---|---:|---:|---:|
| forge-template | 1 | 1 | 100% |
| solady | 781 | 781 | 100% |
| uniswap-v4-core | 442 | 442 | 100% |
| solmate | 151 | 151 | 100% |
| prb-math | 157 | 157 | 100% |
| **overall** | **1,532** | **1,532** | **100%** |

`testFail*` legacy names are excluded (forge dropped support; EDB
follows). Re-run with `./scripts/edb-static-cheatcode-coverage.py`
after adding cheatcodes to the catalog to see the updated number.

Round-4 closed the last gap: the seven outstanding cheatcodes (the
`signP256` / `publicKeyP256` pair, `readLine`, `getNonce`,
`getBlockNumber`, `setBlockhash`, and the deferred `getRawBlockHeader`)
landed in this round. With all 1,532 tests now statically eligible
across the five vendored projects, the static blocker list is empty
under the current coverage script — the only remaining caveat is
`vm.getRawBlockHeader`, which is catalogued as `Supported` for static
coverage purposes but reverts at runtime with a clear "requires an
upstream RPC channel" message in non-fork mode (see Rejected section).

(Historical blockers: `vm.toString` (29), `vm.parseJson*` (24 combined),
`vm.txGasPrice` (22), `vm.fee` (16), `vm.snapshotValue`, and
`vm.getDeployedCode` topped this list before the round-3 batch landed.)

## Quick start

```bash
edb test MyTest::testFoo
```

EDB intercepts every `CALL` to the cheatcode precompile address
(`0x7109709ECfa91a80626fF3989D68f67F5b1DD12D`) and dispatches by 4-byte
selector. If your test calls a cheatcode EDB doesn't support, you'll see an
error like this **before the UI launches**:

```
EDB: this test uses cheatcodes not supported in v1. Aborting before UI launch.

  - vm.selectFork (rejected, called 1x)
  - vm.transact (rejected, called 1x)

See docs/cheatcodes.md for the full support matrix and workarounds.
```

The abort happens during EDB's prepare phase — your test isn't actually run
in the debugger. Fix the test (or wait for EDB to ship the missing
cheatcode) before retrying.

## Supported

| Cheatcode | Behavior |
|---|---|
| `vm.warp(uint256)` | Mutates `block.timestamp`. Subsequent `TIMESTAMP` opcodes read the new value. |
| `vm.roll(uint256)` | Mutates `block.number`. |
| `vm.chainId(uint256)` | Mutates `cfg.chain_id`; the `CHAINID` opcode reads the new value. |
| `vm.fee(uint256)` | Mutates `block.basefee` (saturating to `u64::MAX` since REVM's `BlockEnv.basefee` is `u64`). Subsequent `BASEFEE` opcodes read the new value. |
| `vm.txGasPrice(uint256)` | Mutates `tx.gas_price` (saturating to `u128::MAX` since REVM's `TxEnv.gas_price` is `u128`). Subsequent `GASPRICE` opcodes read the new value. |
| `vm.toString(address)` / `(bool)` / `(bytes)` / `(bytes32)` / `(int256)` / `(uint256)` | Format the value as the Solidity-canonical string. `address` -> EIP-55 checksum; `bool` -> `"true"` / `"false"`; `bytes` / `bytes32` -> `"0x"` + lowercase hex; `uint256` / `int256` -> decimal (signed for `int256`). |
| `vm.parseJson(string)` / `(string,string)` | Parse JSON; if a path is given, walk to the leaf. Returns an ABI-encoded primitive guess (bool / number / string / 20-byte address / 32-byte hex). Complex (object / mixed array) leaves are not supported in v1 — use the typed `vm.parseJson<Type>` overload instead. |
| `vm.parseJsonBool(json, path)` / `parseJsonString` / `parseJsonBytes32` / `parseJsonUint` / `parseJsonInt` / `parseJsonAddress` | Walk a foundry-style JSONPath subset (`.foo.bar[0]`, leading `$` optional) to a primitive leaf and return the ABI-encoded typed value. The walker supports `.key` and `[index]` tokens; complex JSONPath filters (`?(@...)`, wildcards) are NOT supported. |
| `vm.deal(address, uint256)` | Sets the account's balance. |
| `vm.etch(address, bytes)` | Sets the account's deployed code via `journal.set_code`. |
| `vm.store(address, bytes32, bytes32)` | Writes a storage slot via `journal.sstore`. |
| `vm.load(address, bytes32)` | Reads a storage slot via `journal.sload` and returns it as a `bytes32`. |
| `vm.setNonce(address, uint64)` | Sets the account's nonce. |
| `vm.getNonce(address) returns (uint64)` | Reads the account's nonce via the journal. Mirror of `vm.setNonce`. |
| `vm.getBlockNumber() returns (uint256)` | Returns the current `block.number`. Useful after `vm.roll` since solc caches `block.number` as a constant within a single transaction body. |
| `vm.setBlockhash(uint256 blockNumber, bytes32 blockHash)` | Installs a `BLOCKHASH` override at `blockNumber` by writing into CacheDB's `block_hashes` cache. Rejects `blockNumber > u64::MAX` and `blockNumber > block.number` (matching foundry's `<=` semantics). |
| `vm.readLine(string path) returns (string)` | Opens the file (lazily, cached per-path), advances by one line per call, strips the trailing `\n` / `\r\n`, and returns `""` at EOF. Subsequent calls to the same path continue from the previous cursor. Paths are canonicalized against `project_root`; absolute paths and `..` escapes are rejected. |
| `vm.signP256(uint256 privateKey, bytes32 digest) returns (bytes32 r, bytes32 s)` | NIST P-256 (secp256r1) ECDSA over the 32-byte pre-hashed digest (signed AS-IS via `sign_prehash`). The returned `s` is low-half normalized (matches foundry's `normalize_s().unwrap_or(signature)`). Rejects `sk == 0` and `sk >= n` with a clear message. |
| `vm.publicKeyP256(uint256 privateKey) returns (uint256 x, uint256 y)` | Derives the uncompressed P-256 public point. Returns ABI-encoded `(x, y)` as two 32-byte big-endian uint256 words. Same private-key validation as `vm.signP256`. |
| `vm.prank(address)` | The next out-of-test call uses the given `msg.sender`. |
| `vm.startPrank(address)` | All subsequent calls at this depth use the given `msg.sender` until `stopPrank`. |
| `vm.startPrank(address, address)` | Like the 1-arg form, but also overrides `tx.origin` for the prank scope. The original `tx.origin` is restored on `stopPrank`. |
| `vm.stopPrank()` | Clears the active prank at the current depth (and restores any `tx.origin` overridden by `startPrank(msgSender, txOrigin)`). |
| `vm.mockCall(address, bytes, bytes)` | When the target is called with matching calldata, return the given bytes instead of executing the call. |
| `vm.mockCallRevert(address, bytes, bytes)` | Same as `mockCall` but the synthetic call reverts with the given bytes. |
| `vm.clearMockedCalls()` | Clears all mocks. |
| `vm.expectRevert()` | Asserts the next sub-call reverts (any reason). If it does, the matched revert is rewritten to a success; otherwise an EDB error is raised. |
| `vm.expectRevert(bytes)` | Like `expectRevert()` but matches the revert payload exactly. |
| `vm.expectRevert(bytes4)` | Like `expectRevert()` but matches the leading 4 bytes (selector) of the revert payload — convenient for custom-error reverts where the trailing ABI args may vary. |
| `vm.label(address, string)` | Records a human-readable label for an address (queryable on the inspector). |
| `vm.recordLogs()` | Starts capturing logs into the inspector's recorder. |
| `vm.getRecordedLogs()` | Returns the captured logs as `Log[]` (foundry's `Vm.Log` shape: `{ bytes32[] topics; bytes data; address emitter; }`), then resets the recorder. |
| `vm.expectCall(address,bytes)` / `vm.expectCall(address,bytes,uint64)` | Asserts at least one (or `uint64` count) external call to the given target with the given calldata is observed before the registering frame ends. |
| `vm.assume(bool)` | `true` -> no-op. `false` -> revert with an EDB violation message. In real Foundry fuzz mode this would silently skip the iteration; EDB surfaces it as a revert in unit-test-only mode so the author sees what happened. |
| `vm.envBool(string)` | Reads the named process env var and parses `"true"`/`"1"` or `"false"`/`"0"` (case-insensitive). Reverts with a clear message on parse failure or if the var is not set. |
| `vm.envBytes(string)` | Reads the named process env var and hex-decodes the value (must start with `0x`). Reverts on invalid hex or if the var is not set. |
| `vm.envString(string)` | Reads the named process env var and returns it as a UTF-8 ABI-encoded `string`. Reverts if the var is not set. |
| `vm.envOr(string,bool)` / `vm.envOr(string,bytes)` / `vm.envOr(string,string)` | Same as the corresponding `envXxx` but returns the second argument when the var is not set instead of reverting. |
| `vm.snapshotState() returns (uint256)` | Capture journaled state + DB; return fresh monotonic id (starts at 1). |
| `vm.snapshot() returns (uint256)` | Deprecated alias of `snapshotState`. |
| `vm.revertToState(uint256) returns (bool)` | Restore from snapshot; **also delete** the snapshot (foundry one-shot semantics). Returns `true` if id existed, `false` otherwise (no revert). |
| `vm.revertTo(uint256) returns (bool)` | Deprecated alias of `revertToState`. |
| `vm.revertToStateAndDelete(uint256) returns (bool)` | Same behavior as `revertToState` — delete-on-revert is already the default. |
| `vm.deleteStateSnapshot(uint256) returns (bool)` | Drop a snapshot without restoring. Returns `true` if id existed. |
| `vm.deleteStateSnapshots()` | Drop all snapshots. |
| `vm.addr(uint256) returns (address)` | Derive an Ethereum address from a secp256k1 secret key (k256-backed via `alloy-signer-local`). Reverts on a zero / out-of-range key. |
| `vm.sign(uint256, bytes32) returns (uint8 v, bytes32 r, bytes32 s)` | ECDSA-sign the pre-hashed digest with the secret key (no EIP-191 prefix added). `v` is normalized to the legacy 27/28 encoding so the output is directly usable as `ecrecover` input. |
| `vm.getDeployedCode(string artifact) returns (bytes runtimeBytecode)` | Return the deployed bytecode of a project contract by artifact name. Accepts `"Foo"`, `"Foo.sol"`, or `"path/Foo.sol:Foo[:version]"`. Resolved against EDB's in-memory `LocalArtifactSet` (built from the project's solc output before prepare). Reverts with `"vm.getDeployedCode: artifact {name:?} not found in project"` when nothing matches. |

### Assertions

40 assertion overloads forwarded by forge-std's `StdAssertions` for the
fixed-width primitive types. Each compares two 32-byte ABI words and either
returns successfully (passing assertion) or reverts with
`Error(string)` carrying a descriptive message. The `_MSG` siblings
embed the caller-supplied error string after `(...)`.

| Family | Overloads |
|---|---|
| `vm.assertEq` | `(uint256, uint256)`, `(int256, int256)`, `(address, address)`, `(bool, bool)`, `(bytes32, bytes32)` (+ `_MSG` siblings) |
| `vm.assertNotEq` | same type set as `assertEq` (+ `_MSG` siblings) |
| `vm.assertGt` / `assertGe` / `assertLt` / `assertLe` | `(uint256, uint256)`, `(int256, int256)` (+ `_MSG` siblings). Signed comparisons handle the cross-sign case explicitly. |
| `vm.assertTrue` / `assertFalse` | `(bool)` and `(bool, string)`. |

Failure messages currently print the two operands as raw 32-byte hex
(`left=0x...`, `right=0x...`) — `forge test` formats them type-aware
(decimal for uints, `true`/`false` for bools). Tracked for v2.

### Gas snapshot stubs

Six selectors recorded as **no-op stubs** so tests that wrap calls in
gas-profiling cheatcodes don't hard-abort. EDB is not a gas profiler
in v1; these calls succeed silently and do NOT produce any
gas-snapshot output. A first-time `tracing::warn!` fires per cheatcode
name per `edb test` run.

- `vm.startSnapshotGas(string)`
- `vm.stopSnapshotGas()` / `vm.stopSnapshotGas(string)` / `vm.stopSnapshotGas(string, string)`
- `vm.snapshotGasLastCall(string)` / `vm.snapshotGasLastCall(string, string)`

All selectors are verified at test time against
`keccak256(canonical_signature)[..4]`; see the unit tests in
`crates/edb/src/cmd/test/cheats.rs`
(`all_assertion_selectors_match_canonical`,
`all_gas_snapshot_selectors_match_canonical`).

### Benchmark-value snapshot stubs

Two selectors recorded as **no-op stubs** so tests that record benchmark
values (e.g. tracked metrics, expected post-conditions in solady's
benchmark suite) don't hard-abort. EDB is not a benchmark recorder in
v1; these calls succeed silently. A first-time `tracing::warn!` fires
once per `edb test` run under the `snapshotValue` name.

- `vm.snapshotValue(string,uint256)`
- `vm.snapshotValue(string,string,uint256)`

Selectors verified against `keccak256(canonical_signature)[..4]` in
`all_snapshot_value_selectors_match_canonical` (see
`crates/edb/src/cmd/test/cheats.rs`).

## Partial support (warns at runtime)

These cheatcodes are SUPPORTED but with limitations. EDB emits a one-time
`tracing::warn!` + `eprintln!` line to stderr the first time each is
called per `edb test` run. Re-invocations stay silent.

| Cheatcode | Limitation | Workaround |
|---|---|---|
| `vm.rollFork(uint256)` | Updates `block.number` only; `block.timestamp` / `basefee` unchanged; CacheDB not invalidated. | Pair with `vm.warp(t)` for the timestamp you need; for state at a specific block, restart with `--fork-block-number` at the CLI. |
| `vm.pauseGasMetering()` / `vm.resumeGasMetering()` | Stub. Flag is tracked but REVM gas accounting is NOT paused. | If your test asserts specific gas behavior between paused/running phases, EDB will give different results than `forge test`. Re-run under `forge` for gas-precise assertions. |
| `vm.lastCallGas()` | Stub. Returns all-zero `Gas{}` for determinism across multi-pass instrumentation. | Same caveat as the gas-metering stubs — don't assert specific gas values under EDB. |
| `vm.expectEmit*` (all 4 overloads) | Soft-match v1: matches any log from the (optionally constrained) emitter with at least a signature topic; NOT byte-equality against a template. False positives possible. | Combine with explicit checks on the recorded log via `vm.recordLogs()` / `vm.getRecordedLogs()` if you need precise event verification. |

### `vm.expectEmit` soft-match — detail

Foundry's `vm.expectEmit` infers a template log from the test contract's
own `emit Foo(...)` statement between the cheatcode call and the next
external call, then byte-compares each captured log against the template
(honoring the `(bool t1, bool t2, bool t3, bool checkData)` mask).

EDB's v1 soft-match is approximate:

- The expectation matches the first log whose emitter is correct (when
  supplied) and whose topic vector is non-empty (every Solidity `emit`
  contributes at least the event-signature topic).
- We do NOT compare topic values or data bytes against any template — EDB
  doesn't capture a template, and we deliberately ignore the
  `(bool t1, bool t2, bool t3, bool checkData)` mask because:
  - Topic-count enforcement (`tN=true ⇒ topics.len() ≥ N+1`) false-fails
    events with fewer indexed args than the user happens to mask, where
    forge would pass because the template carries the same shape.
  - `checkData=true ⇒ data.len() > 0` false-fails events whose only args
    are indexed (empty data payload), which forge accepts for the same
    template-shape reason.
- The mask is still recorded on the expectation for future use when v2
  brings template capture.

This covers "did the contract emit any qualifying event?" at the cost
of false positives on byte-equality checks. Faithful template matching
is tracked for v2.

## Not supported

EDB **aborts the test before launching the UI** if any unsupported
cheatcode is called during prepare.

If you hit one and want to find it fast, ⌘-F / Ctrl-F the selector
listed in the abort message (`selector 0x...`) against this page —
every catalog entry below lists its 4-byte selector explicitly.

### Rejected (need infrastructure EDB doesn't ship in v1)

These are explicitly catalogued — EDB recognises the call and aborts
with a "rejected" category. The full set lives in `KNOWN_CHEATCODES`
in `crates/edb/src/cmd/test/cheats.rs` under
`--- Explicitly rejected in EDB v1 ---`; the table below is sorted by
selector and reproduced verbatim from that catalog.

| Selector | Cheatcode | Why |
|---|---|---|
| `0x0f29772b` | `vm.rollFork(bytes32)` | Cross-fork variant. (Single-arg `vm.rollFork(uint256)` is partial — see Partial support above.) |
| `0x08e4e116` | `vm.expectCallMinGas(address,uint256,uint64,bytes)` | Gas accounting under EDB's instrumented bytecode needs separate design. |
| `0x2c667606` | `vm.getRawBlockHeader(uint256)` | Deferred to v2: requires an upstream RPC channel and synchronous-from-async dispatch in the cheatcode handler. |
| `0x1d9e269e` | `vm.makePersistent(address[])` | No multi-fork backend in v1. |
| `0x2f103f22` | `vm.activeFork()` | Same. |
| `0x31ba3498` | `vm.createFork(string)` | Same. |
| `0x4074e0a8` | `vm.makePersistent(address,address)` | Same. |
| `0x4d8abc4b` | `vm.transact(uint256,bytes32)` | Multi-fork + separate-tx execution. |
| `0x57e22dde` | `vm.makePersistent(address)` | No multi-fork backend in v1. |
| `0x60f9bb11` | `vm.readFile(string)` | Filesystem access; disabled for safety. |
| `0x6ba3ba2b` | `vm.createFork(string,uint256)` | No multi-fork backend in v1. |
| `0x71ee464d` | `vm.createSelectFork(string,uint256)` | Same. |
| `0x76eadd36` | `vm.stopBroadcast()` | Script-only; not applicable to `forge test`. |
| `0x7ca29682` | `vm.createFork(string,bytes32)` | No multi-fork backend in v1. |
| `0x7fb5297f` | `vm.startBroadcast()` | Script-only. |
| `0x7fec2a8d` | `vm.startBroadcast(address)` | Script-only. |
| `0x84d52b7a` | `vm.createSelectFork(string,bytes32)` | No multi-fork backend in v1. |
| `0x89160467` | `vm.ffi(string[])` | External-process; disabled for safety. |
| `0x897e0a97` | `vm.writeFile(string,string)` | Filesystem access; disabled. |
| `0x98680034` | `vm.createSelectFork(string)` | No multi-fork backend in v1. |
| `0x9ebf6827` | `vm.selectFork(uint256)` | Same. |
| `0xafc98040` | `vm.broadcast()` | Script-only. |
| `0xbe646da1` | `vm.transact(bytes32)` | Multi-fork + separate-tx execution. |
| `0xd74c83a4` | `vm.rollFork(uint256,uint256)` | Cross-fork variant. |
| `0xefb77a75` | `vm.makePersistent(address,address,address)` | No multi-fork backend in v1. |
| `0xf1afe04d` | `vm.removeFile(string)` | Filesystem access; disabled. |
| `0xf2830f7b` | `vm.rollFork(uint256,bytes32)` | Cross-fork variant. |

### Not yet implemented

Cheatcodes EDB knows about (catalog entries) but doesn't yet implement.
Calling one aborts the test with the message in the next section.

Common v2 candidates:

- `vm.envInt`, `vm.envUint`, `vm.envAddress` (and their `envOr` overloads
  for int / uint / address)
- `vm.parseJson*`, `vm.parseToml*`
- `vm.toString*`, `vm.serialize*`
- mapping introspection: `vm.getMappingKeyOf`, `vm.getMappingLength`,
  `vm.getMappingSlotAt`, `vm.startMappingRecording` /
  `vm.stopMappingRecording`
- state-diff recording: `vm.startStateDiffRecording`,
  `vm.stopAndReturnStateDiff`

#### Dynamic / array / decimal assertion overloads

Modern forge-std's `StdAssertions` forwards `assertEq(string,string)`,
`assertEq(bytes,bytes)`, the array variants, the `*Decimal` variants,
and `assertApproxEqAbs` / `assertApproxEqRel` to dedicated cheatcode
selectors. EDB v1 catalogs these but does not implement them yet —
calls produce
"`EDB: cheatcode vm.<name> not yet implemented in v1 (selector 0x...)`":

- `vm.assertEq(string,string)` / `(bytes,bytes)` / `(bool[],bool[])` /
  `(uint256[],uint256[])` / `(int256[],int256[])` /
  `(address[],address[])` / `(bytes32[],bytes32[])` /
  `(string[],string[])` / `(bytes[],bytes[])` (+ `_MSG` siblings)
- `vm.assertNotEq` — same dynamic / array set (+ `_MSG` siblings)
- `vm.assertEqDecimal` / `assertNotEqDecimal` /
  `assertGtDecimal` / `assertGeDecimal` /
  `assertLtDecimal` / `assertLeDecimal`
  (uint and int variants, + `_MSG` siblings)
- `vm.assertApproxEqAbs` / `assertApproxEqRel`
  (uint and int variants, + `_MSG` siblings)

The full not-yet-implemented set is enumerated in `KNOWN_CHEATCODES` in
`crates/edb/src/cmd/test/cheats.rs`.

## Known caveats (not cheatcode-specific)

These aren't cheatcode-level limits but they affect what EDB can debug.

### EDB explicitly does not model gas

EDB is a **source-level debugger**, not a gas profiler. The multi-pass
engine runs each transaction with hook-instrumented bytecode in the
hook-snapshot pass; each statement boundary inserts a small `require(
keccak256(abi.encode(MAGIC, USID)) != 0x2333)` check whose gas cost is
on the order of ~30 gas. Across hundreds of statements per call this
adds up.

We make **no attempt** to keep gas usage faithful between EDB and
forge. Cheatcodes in this family are silent stubs by design:

| Cheatcode | What EDB does |
|---|---|
| `vm.pauseGasMetering` / `vm.resumeGasMetering` | Records the flag but does NOT pause REVM gas accounting |
| `vm.lastCallGas` | Returns all-zero `Gas{}` |
| `vm.startSnapshotGas` / `vm.stopSnapshotGas` / `vm.snapshotGasLastCall` / `vm.snapshotValue` | Silent no-op stubs |

If your test asserts specific gas values, EDB will give different
numbers than forge — **this is intentional** and not a bug. Run under
`forge test` for gas-precise assertions.

### Forked tests + unverified contracts

In `--fork-url` mode, any address that misses the local artifact set
falls back to the Etherscan API (`EdbCachePath::etherscan_chain_cache_dir`).
If the contract is not verified on Etherscan, EDB has no source for it —
the trace will show the call but its body cannot be stepped through in
the debugger UI. The call/return semantics are still faithful (we
execute the actual runtime bytecode); only the source-level view is
missing.

### Instrumentation-side test-reverts (a small handful of known cases)

The engine runs the transaction in three passes (tracer → opcode
snapshots → hook snapshots), with the third pass executing
**instrumented runtime bytecode** that mirrors the original Solidity
source but with snapshot-hook `require(keccak256(...))` calls inserted
at every statement boundary. Hooks never SSTORE — only read-side
keccak — so:

- **Storage layout is preserved.** Solc lays out state-variable slots
  based on declaration order alone, which the instrumenter doesn't
  touch.
- **`address(this)` / CREATE / CREATE2-derived addresses are
  preserved.** The hook-snapshot inspector intercepts `CREATE` at
  `crates/engine/src/inspector/hook_snapshot_inspector.rs:652-678`,
  substitutes the instrumented initcode but pins the deployment
  address to whatever the original-initcode CREATE would have produced
  (`CreateScheme::Custom { address: predicted_address }`). So:
  - For CREATE: `address(this)` matches Pass 1 (deployer+nonce
    sequence is preserved).
  - For CREATE2: the natural
    `keccak(0xff || deployer || salt || keccak(initcode))[12:]`
    derivation would yield a different address for the instrumented
    initcode, but `CreateScheme::Custom` overrides that with the
    original-initcode address. Tests that hard-code expected
    CREATE2 addresses continue to work.
- **Immutables work transparently.** Solc emits the instrumented
  constructor with MSTORE patch instructions at the correct (shifted)
  byte positions for the instrumented runtime; the patched values
  themselves are computed by the same Solidity logic in both
  compilations.

Despite all of this, a small number of real-world tests pass under
`forge test` but revert under EDB. The remaining divergences trace
mostly to:

1. **`vm.etch(addr, address(thing).code)` patterns** — `.code`
   evaluated in Pass 3 returns the *instrumented* runtime, which
   carries hooks but also embedded references (immutables, library
   links) to its original deployment context. Re-installing this code
   at a different address can break those embedded references.
2. **Inline assembly with memory-layout assumptions** — the
   instrumenter operates on the Solidity AST, but assembly blocks can
   make assumptions about scratch-memory state that no longer hold
   after a hook just wrote there.
3. **Self-hash checks** — code that does
   `keccak256(type(C).runtimeCode)` or checks its own deployed code
   against a recorded hash necessarily diverges (the instrumented
   runtime hashes differently).

Confirmed-divergent cases (1 per category):

- `solady` `ERC4337Test::testDepositFunctions` — case (1): the test
  does `vm.etch(account.entryPoint(), address(new MockEntryPoint()).code)`
  to install MockEntryPoint at the canonical ERC-4337 entrypoint address.
  The `.code` it copies is instrumented (Pass 3), and re-installing
  there triggers a revert mid-execution.
- `uniswap-v4-core` `CustomAccountingTest::test_swap_afterSwapFeeOnUnspecified_exactOutput`
  — diagnostic at `crates/engine/src/inspector/hook_snapshot_inspector.rs:861`:
  `Create outcome mismatch at frame N: expected Some(Success ...), got CreateOutcome { result: Revert ... }`.
  One of ~14 CREATEs in `_setUpFeeTakingPool()` has a Pass 3
  constructor that reverts where Pass 1 succeeded. Likely case (2)
  (inline assembly in one of the pool-manager contracts), but the
  exact contract still needs to be bisected.
- `solmate` `WETHTest::*` (the whole suite reverts) — root cause
  unclear; given the address-pinning + storage-preservation
  invariants above, the WETH-specific issue is neither
  `address(this)` nor storage layout. Worth a focused
  per-test investigation.

These are tracked as v1.x engine bugs. Run `forge test` against the
same fixtures when you hit a `test-revert` status on EDB and want to
verify whether the test is supposed to pass.

## Error messages

When `edb test` encounters a cheatcode it does not implement, you can see
the failure surface in three places:

- **Aborted before UI launch** (the primary surface):

  ```
  EDB: this test uses cheatcodes not supported in v1. Aborting before UI launch.

    - vm.<name1> (<category>, called Nx)
    - vm.<name2> (<category>, called Mx)

  See docs/cheatcodes.md for the full support matrix and workarounds.
  ```

  Category is one of `rejected`, `not yet implemented`, or `unknown
  selector` — see sections above for the difference.

- **Inline in the trace** (only relevant if you're inspecting the trace
  directly — the abort fires before UI launch):

  ```
  EDB: cheatcode vm.<name> not yet implemented in v1 (selector 0x<hex>). See docs/cheatcodes.md
  ```

- **Unknown selector** (cheatcode foundry has but EDB's catalog doesn't
  yet know — usually means a very new foundry cheatcode, or a non-vm
  call that accidentally hit the cheatcode address):

  ```
  EDB: unknown cheatcode selector 0x<hex> (not in foundry's known cheatcode catalog — check spelling or open an issue)
  ```

The catalog and dispatch live in `crates/edb/src/cmd/test/cheats.rs`
(`KNOWN_CHEATCODES` constant + `dispatch` method). Each selector in the
catalog is verified against `keccak256(canonical_signature)[..4]` by the
`known_cheatcode_catalog_*` unit tests in the same file.

## Want a cheatcode added?

Open an issue at <https://github.com/edb-rs/edb/issues> with the cheatcode
name and a real-world test that uses it.
