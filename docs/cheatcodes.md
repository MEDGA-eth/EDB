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
| `vm.deal(address, uint256)` | Sets the account's balance. |
| `vm.etch(address, bytes)` | Sets the account's deployed code via `journal.set_code`. |
| `vm.store(address, bytes32, bytes32)` | Writes a storage slot via `journal.sstore`. |
| `vm.load(address, bytes32)` | Reads a storage slot via `journal.sload` and returns it as a `bytes32`. |
| `vm.setNonce(address, uint64)` | Sets the account's nonce. |
| `vm.prank(address)` | The next out-of-test call uses the given `msg.sender`. |
| `vm.startPrank(address)` | All subsequent calls at this depth use the given `msg.sender` until `stopPrank`. |
| `vm.stopPrank()` | Clears the active prank at the current depth. |
| `vm.mockCall(address, bytes, bytes)` | When the target is called with matching calldata, return the given bytes instead of executing the call. |
| `vm.mockCallRevert(address, bytes, bytes)` | Same as `mockCall` but the synthetic call reverts with the given bytes. |
| `vm.clearMockedCalls()` | Clears all mocks. |
| `vm.expectRevert()` | Asserts the next sub-call reverts (any reason). If it does, the matched revert is rewritten to a success; otherwise an EDB error is raised. |
| `vm.expectRevert(bytes)` | Like `expectRevert()` but matches the revert payload exactly. |
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

## Partial support (warns at runtime)

These cheatcodes are SUPPORTED but with limitations. EDB emits a one-time
`tracing::warn!` + `eprintln!` line to stderr the first time each is
called per `edb test` run. Re-invocations stay silent.

| Cheatcode | Limitation | Workaround |
|---|---|---|
| `vm.rollFork(uint256)` | Updates `block.number` only; `block.timestamp` / `basefee` unchanged; CacheDB not invalidated. | Pair with `vm.warp(t)` for the timestamp you need; for state at a specific block, restart with `--fork-block-number` at the CLI. |
| `vm.pauseGasMetering()` / `vm.resumeGasMetering()` | Stub. Flag is tracked but REVM gas accounting is NOT paused. | If your test asserts specific gas behavior between paused/running phases, EDB will give different results than `forge test`. Re-run under `forge` for gas-precise assertions. |
| `vm.lastCallGas()` | Stub. Returns all-zero `Gas{}` for determinism across multi-pass instrumentation. | Same caveat as the gas-metering stubs — don't assert specific gas values under EDB. |
| `vm.expectEmit*` (all 4 overloads) | Soft-match v1: checks emitter + topic-count + non-empty data; NOT byte-equality against a template. False positives possible. | Combine with explicit checks on the recorded log via `vm.recordLogs()` / `vm.getRecordedLogs()` if you need precise event verification. |

### `vm.expectEmit` soft-match — detail

Foundry's `vm.expectEmit` infers a template log from the test contract's
own `emit Foo(...)` statement between the cheatcode call and the next
external call, then byte-compares each captured log against the template
(honoring the `(bool t1, bool t2, bool t3, bool checkData)` mask).

EDB's v1 soft-match is approximate:

- The expectation matches the first log whose emitter is correct (when
  supplied) and whose topic slot count is at least the highest index marked
  `true` in the bool mask. When `checkData` is `true` we additionally
  require non-empty data.
- We do NOT compare topic values or data bytes against any template — EDB
  doesn't capture a template.

This covers the "did the contract emit any qualifying event?" smoke
pattern at the cost of false positives on byte-equality checks. Faithful
template matching is tracked for v2.

## Not supported

EDB **aborts the test before launching the UI** if any unsupported
cheatcode is called during prepare.

### Rejected (need infrastructure EDB doesn't ship in v1)

| Cheatcode | Why |
|---|---|
| `vm.createFork`, `vm.createSelectFork`, `vm.selectFork`, `vm.activeFork`, `vm.makePersistent` | No multi-fork backend in v1. |
| `vm.rollFork(bytes32)`, `vm.rollFork(uint256,uint256)`, `vm.rollFork(uint256,bytes32)` | Cross-fork variants require the multi-fork backend. (Single-arg `vm.rollFork(uint256)` is partially supported — see above.) |
| `vm.transact(bytes32)` | Requires the multi-fork backend and a separate-tx execution model. |
| `vm.broadcast`, `vm.startBroadcast`, `vm.stopBroadcast` | Script-only; not applicable to `forge test`. |
| `vm.ffi`, `vm.readFile`, `vm.writeFile`, `vm.removeFile` | External-process and filesystem access disabled in v1 for safety. |
| `vm.expectCallMinGas(address,uint256,uint64,bytes)` | Gas accounting under EDB's instrumented bytecode needs separate design work; deferred to v2. |

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
