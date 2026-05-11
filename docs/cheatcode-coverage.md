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

All selectors are verified at test time against `keccak256(canonical_signature)[..4]`;
see the unit tests in `crates/edb/src/cmd/test/cheats.rs`.

## Explicitly rejected (revert with EDB error)

These cheatcodes return a clear revert message — `EDB: cheatcode vm.<name> not supported in v1` — so calling-code surfaces a clean failure instead of silently no-op'ing.

| Cheatcode | Why |
|---|---|
| `vm.createFork`, `vm.createSelectFork`, `vm.selectFork`, `vm.rollFork`, `vm.activeFork`, `vm.makePersistent` | No multi-fork backend in v1. (Same-fork `rollFork(block)` will land later — see Task 8.3.) |
| `vm.transact(bytes32)` | Requires the multi-fork backend and a separate-tx execution model. |
| `vm.snapshotState`, `vm.revertToState`, plus legacy `vm.snapshot` / `vm.revertTo` | Mid-tx journal rewind is not exposed via EDB's CacheDB journal in v1. |
| `vm.broadcast`, `vm.startBroadcast`, `vm.stopBroadcast` | Script-only; not applicable to `forge test`. |
| `vm.ffi`, `vm.readFile`, `vm.writeFile`, `vm.removeFile` | Security: external process / fs access disabled in v1. |

## Not yet implemented (unknown-selector revert)

Anything not in the supported or rejected lists returns:

```
EDB: unknown cheatcode selector 0x<hex> (likely not implemented in v1)
```

so authors can file an issue or PR. Common candidates for v2:
`vm.expectEmit`, `vm.expectCall`, `vm.env*`, `vm.assume`,
`vm.pauseGasMetering`, `vm.lastCallGas`, `vm.parseJson*`, `vm.parseToml*`.

## Want a cheatcode added?

Open an issue at <https://github.com/edb-rs/edb/issues> with the cheatcode
name and a real-world test that uses it.
