# `edb test` Cheatcode Coverage

This document tracks which Foundry cheatcodes are supported in `edb test` v1.

EDB ships its own hand-rolled cheatcode inspector
(`crates/edb/src/cmd/test/cheats.rs`) rather than embedding
`foundry-cheatcodes`, because foundry's `Cheatcodes<...>` Inspector trait
bound is structurally incompatible with EDB's `EdbContext<DB>`. The
hand-rolled inspector intercepts every `CALL` to the cheatcode precompile
address (`0x7109709ECfa91a80626fF3989D68f67F5b1DD12D`) and dispatches by
4-byte selector.

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
| `vm.expectEmit()` / `vm.expectEmit(bool,bool,bool,bool)` / `vm.expectEmit(bool,bool,bool,bool,address)` / `vm.expectEmit(address)` | Soft-match v1 — see Limitations below. Registers a pending log expectation; verified when the registering frame ends. |
| `vm.expectCall(address,bytes)` / `vm.expectCall(address,bytes,uint64)` | Asserts at least one (or `uint64` count) external call to the given target with the given calldata is observed before the registering frame ends. |
| `vm.assume(bool)` | `true` → no-op. `false` → revert with an EDB violation message. In real Foundry fuzz mode this would silently skip the iteration; EDB surfaces it as a revert in unit-test-only mode so the author sees what happened. |
| `vm.envBool(string)` | Reads the named process env var and parses `"true"`/`"1"` or `"false"`/`"0"` (case-insensitive). Reverts with a clear message on parse failure or if the var is not set. |
| `vm.envBytes(string)` | Reads the named process env var and hex-decodes the value (must start with `0x`). Reverts on invalid hex or if the var is not set. |
| `vm.envString(string)` | Reads the named process env var and returns it as a UTF-8 ABI-encoded `string`. Reverts if the var is not set. |
| `vm.envOr(string,bool)` / `vm.envOr(string,bytes)` / `vm.envOr(string,string)` | Same as the corresponding `envXxx` but returns the second argument when the var is not set instead of reverting. |
| `vm.pauseGasMetering()` / `vm.resumeGasMetering()` | **Stub.** Tracks a `gas_metering_paused` flag but does NOT actually pause REVM's gas accounting. Tests that call these for flow control (don't crash) work fine; tests that assert different gas behavior between paused and running may see unexpected values. |
| `vm.lastCallGas() returns (Gas memory)` | **Stub.** Returns an all-zero `Gas` struct (`gasLimit`, `gasTotalUsed`, `gasMemoryUsed`, `gasRefunded`, `gasRemaining` all 0). EDB runs the same transaction in multiple instrumented passes; real REVM gas values differ between passes and would cause non-determinism, so zero is the correct deterministic stub. Tests that assert specific gas values will fail; tests that only flow through (don't assert gas) will work. |

All selectors are verified at test time against `keccak256(canonical_signature)[..4]`;
see the unit tests in `crates/edb/src/cmd/test/cheats.rs`.

### `vm.envOr` variants deferred to v2

The following `vm.envOr` overloads (int/uint/address types) are not yet implemented and will fall through to the "unknown selector" revert:

- `vm.envOr(string,int256)` (and other int/uint widths)
- `vm.envOr(string,uint256)` (and other uint widths)
- `vm.envOr(string,address)`

These will be added in v2 alongside `vm.envInt`, `vm.envUint`, `vm.envAddress`.

### Limitations: `vm.expectEmit` soft-match semantics (v1)

Foundry's `vm.expectEmit` infers an expected log template from the test
contract's own `emit Foo(...)` statement between the cheatcode call and the
next external call, then byte-compares each captured log against that
template (honoring the `(bool t1, bool t2, bool t3, bool checkData)` mask).

EDB's v1 ships a **soft-match** approximation:

- The expectation accepts the first log whose emitter matches (when an
  emitter is supplied) and whose topic slot count is at least the highest
  index marked `true` in the bool mask. When `checkData` is `true` we
  additionally require the log's data to be non-empty.
- We do NOT compare topic values or data bytes against any template — there
  is no template captured.

This covers the common "did the contract emit any qualifying event?" smoke
pattern at the cost of false positives for byte-equality checks. Faithful
template matching is tracked for v2.

## Explicitly rejected (revert with EDB error)

These cheatcodes return a clear revert message — `EDB: cheatcode vm.<name> not supported in v1` — so calling-code surfaces a clean failure instead of silently no-op'ing.

| Cheatcode | Why |
|---|---|
| `vm.createFork`, `vm.createSelectFork`, `vm.selectFork`, `vm.rollFork`, `vm.activeFork`, `vm.makePersistent` | No multi-fork backend in v1. (Same-fork `rollFork(block)` will land later — see Task 8.3.) |
| `vm.transact(bytes32)` | Requires the multi-fork backend and a separate-tx execution model. |
| `vm.snapshotState`, `vm.revertToState`, plus legacy `vm.snapshot` / `vm.revertTo` | Mid-tx journal rewind is not exposed via EDB's CacheDB journal in v1. |
| `vm.broadcast`, `vm.startBroadcast`, `vm.stopBroadcast` | Script-only; not applicable to `forge test`. |
| `vm.ffi`, `vm.readFile`, `vm.writeFile`, `vm.removeFile` | Security: external process / fs access disabled in v1. |
| `vm.expectCallMinGas(address,uint256,uint64,bytes)` | Gas accounting under EDB's instrumented bytecode needs separate design work; deferred to v2. |

## Not yet implemented (unknown-selector revert)

Anything not in the supported or rejected lists returns:

```
EDB: unknown cheatcode selector 0x<hex> (likely not implemented in v1)
```

so authors can file an issue or PR. Common candidates for v2:
`vm.envInt`, `vm.envUint`, `vm.envAddress` (and their `envOr` overloads),
`vm.parseJson*`, `vm.parseToml*`.

## Want a cheatcode added?

Open an issue at <https://github.com/edb-rs/edb/issues> with the cheatcode
name and a real-world test that uses it.
