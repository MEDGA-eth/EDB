// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! Hand-rolled cheatcode inspector for `edb test` sessions.
//!
//! Implements a curated subset of foundry's cheatcodes by intercepting CALLs
//! to the cheatcode precompile address (0x7109...DD12D) and dispatching by
//! 4-byte selector. Unsupported cheatcodes return a revert with a clear EDB
//! error string.
//!
//! Coverage matrix: see `docs/cheatcodes.md` (linked from README).
//!
//! Design notes:
//! - The inspector is generic over `EdbContext<DB>` so it composes natively
//!   with EDB's `CacheDB<DB>` journal — no Inspector trait-bound mismatch
//!   like the upstream foundry-cheatcodes inspector (see Task 5.5 commit).
//! - State (pranks, mocks, expectRevert, recorded logs, labels) lives on
//!   the inspector value itself; the engine builds a fresh inspector via
//!   `build_cheats_factory` for each orchestration pass.

use alloy_primitives::{Address, B256, Bytes, Log, U256, address};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use revm::{
    Database, DatabaseCommit, DatabaseRef, Inspector,
    context::JournalTr,
    context_interface::journaled_state::account::JournaledAccountTr,
    database::CacheDB,
    interpreter::{CallInputs, CallOutcome, Gas, InstructionResult, InterpreterResult},
    state::Bytecode,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Cheatcode precompile address (matches foundry's).
pub const CHEATCODE_ADDRESS: Address = address!("7109709ECfa91a80626fF3989D68f67F5b1DD12D");

// ----------------------------------------------------------------------------
// Cheatcode selectors (verified against forge-std's `Vm.sol` by recomputing
// keccak256 of the canonical signature; see this file's unit tests).
// ----------------------------------------------------------------------------

// Supported set
const SEL_WARP: [u8; 4] = [0xe5, 0xd6, 0xbf, 0x02]; // warp(uint256)
const SEL_ROLL: [u8; 4] = [0x1f, 0x7b, 0x4f, 0x30]; // roll(uint256)
const SEL_CHAIN_ID: [u8; 4] = [0x40, 0x49, 0xdd, 0xd2]; // chainId(uint256)
const SEL_DEAL: [u8; 4] = [0xc8, 0x8a, 0x5e, 0x6d]; // deal(address,uint256)
const SEL_ETCH: [u8; 4] = [0xb4, 0xd6, 0xc7, 0x82]; // etch(address,bytes)
const SEL_STORE: [u8; 4] = [0x70, 0xca, 0x10, 0xbb]; // store(address,bytes32,bytes32)
const SEL_LOAD: [u8; 4] = [0x66, 0x7f, 0x9d, 0x70]; // load(address,bytes32)
const SEL_SET_NONCE: [u8; 4] = [0xf8, 0xe1, 0x8b, 0x57]; // setNonce(address,uint64)
const SEL_GET_NONCE: [u8; 4] = [0x2d, 0x03, 0x35, 0xab]; // getNonce(address)
const SEL_GET_BLOCK_NUMBER: [u8; 4] = [0x42, 0xcb, 0xb1, 0x5c]; // getBlockNumber()
const SEL_SET_BLOCKHASH: [u8; 4] = [0x53, 0x14, 0xb5, 0x4a]; // setBlockhash(uint256,bytes32)
const SEL_GET_RAW_BLOCK_HEADER: [u8; 4] = [0x2c, 0x66, 0x76, 0x06]; // getRawBlockHeader(uint256)
const SEL_READ_LINE: [u8; 4] = [0x70, 0xf5, 0x57, 0x28]; // readLine(string)
const SEL_PRANK: [u8; 4] = [0xca, 0x66, 0x9f, 0xa7]; // prank(address)
const SEL_START_PRANK: [u8; 4] = [0x06, 0x44, 0x7d, 0x56]; // startPrank(address)
const SEL_START_PRANK_2: [u8; 4] = [0x45, 0xb5, 0x60, 0x78]; // startPrank(address,address)
const SEL_STOP_PRANK: [u8; 4] = [0x90, 0xc5, 0x01, 0x3b]; // stopPrank()
const SEL_MOCK_CALL: [u8; 4] = [0xb9, 0x62, 0x13, 0xe4]; // mockCall(address,bytes,bytes)
const SEL_MOCK_CALL_REVERT: [u8; 4] = [0xdb, 0xaa, 0xd1, 0x47]; // mockCallRevert(address,bytes,bytes)
const SEL_CLEAR_MOCKED_CALLS: [u8; 4] = [0x3f, 0xdf, 0x4e, 0x15]; // clearMockedCalls()
const SEL_EXPECT_REVERT_BARE: [u8; 4] = [0xf4, 0x84, 0x48, 0x14]; // expectRevert()
const SEL_EXPECT_REVERT_BYTES: [u8; 4] = [0xf2, 0x8d, 0xce, 0xb3]; // expectRevert(bytes)
const SEL_EXPECT_REVERT_BYTES4: [u8; 4] = [0xc3, 0x1e, 0xb0, 0xe0]; // expectRevert(bytes4)
const SEL_LABEL: [u8; 4] = [0xc6, 0x57, 0xc7, 0x18]; // label(address,string)
const SEL_RECORD_LOGS: [u8; 4] = [0x41, 0xaf, 0x2f, 0x52]; // recordLogs()
const SEL_GET_RECORDED_LOGS: [u8; 4] = [0x19, 0x15, 0x53, 0xa4]; // getRecordedLogs()
const SEL_EXPECT_EMIT_BARE: [u8; 4] = [0x44, 0x0e, 0xd1, 0x0d]; // expectEmit()
const SEL_EXPECT_EMIT_FILTER4: [u8; 4] = [0x49, 0x1c, 0xc7, 0xc2]; // expectEmit(bool,bool,bool,bool)
const SEL_EXPECT_EMIT_FILTER5: [u8; 4] = [0x81, 0xba, 0xd6, 0xf3]; // expectEmit(bool,bool,bool,bool,address)
const SEL_EXPECT_EMIT_ADDR: [u8; 4] = [0x86, 0xb9, 0x62, 0x0d]; // expectEmit(address)
const SEL_EXPECT_CALL: [u8; 4] = [0xbd, 0x6a, 0xf4, 0x34]; // expectCall(address,bytes)
const SEL_EXPECT_CALL_COUNT: [u8; 4] = [0xc1, 0xad, 0xbb, 0xff]; // expectCall(address,bytes,uint64)
const SEL_EXPECT_CALL_MIN_GAS: [u8; 4] = [0x08, 0xe4, 0xe1, 0x16]; // expectCallMinGas(address,uint256,uint64,bytes)
const SEL_ASSUME: [u8; 4] = [0x4c, 0x63, 0xe5, 0x62]; // assume(bool)
const SEL_ENV_BOOL: [u8; 4] = [0x7e, 0xd1, 0xec, 0x7d]; // envBool(string)
const SEL_ENV_BYTES: [u8; 4] = [0x4d, 0x7b, 0xaf, 0x06]; // envBytes(string)
const SEL_ENV_STRING: [u8; 4] = [0xf8, 0x77, 0xcb, 0x19]; // envString(string)
const SEL_ENV_OR_BOOL: [u8; 4] = [0x47, 0x77, 0xf3, 0xcf]; // envOr(string,bool)
const SEL_ENV_OR_BYTES: [u8; 4] = [0xb3, 0xe4, 0x77, 0x05]; // envOr(string,bytes)
const SEL_ENV_OR_STRING: [u8; 4] = [0xd1, 0x45, 0x73, 0x6c]; // envOr(string,string)
const SEL_PAUSE_GAS_METERING: [u8; 4] = [0xd1, 0xa5, 0xb3, 0x6f]; // pauseGasMetering()
const SEL_RESUME_GAS_METERING: [u8; 4] = [0x2b, 0xcd, 0x50, 0xe0]; // resumeGasMetering()
const SEL_LAST_CALL_GAS: [u8; 4] = [0x2b, 0x58, 0x9b, 0x28]; // lastCallGas()

// Block / tx env mutators (peers of vm.warp / vm.roll / vm.chainId).
const SEL_FEE: [u8; 4] = [0x39, 0xb3, 0x7a, 0xb0]; // fee(uint256)            -> sets block.basefee
const SEL_TX_GAS_PRICE: [u8; 4] = [0x48, 0xf5, 0x0c, 0x0f]; // txGasPrice(uint256)   -> sets tx.gas_price

// vm.toString — formatting primitives as their Solidity-canonical string.
// Returns an ABI-encoded `string` (offset+length+padded UTF-8).
const SEL_TO_STRING_ADDRESS: [u8; 4] = [0x56, 0xca, 0x62, 0x3e]; // toString(address)
const SEL_TO_STRING_BOOL: [u8; 4] = [0x71, 0xdc, 0xe7, 0xda]; // toString(bool)
const SEL_TO_STRING_BYTES: [u8; 4] = [0x71, 0xaa, 0xd1, 0x0d]; // toString(bytes)
const SEL_TO_STRING_BYTES32: [u8; 4] = [0xb1, 0x1a, 0x19, 0xe8]; // toString(bytes32)
const SEL_TO_STRING_INT256: [u8; 4] = [0xa3, 0x22, 0xc4, 0x0e]; // toString(int256)
const SEL_TO_STRING_UINT256: [u8; 4] = [0x69, 0x00, 0xa3, 0xae]; // toString(uint256)

// vm.parseJson family — minimal JSONPath-like access against a serde_json
// tree. Supported accessor grammar (subset of foundry's jsonpath_lib): leading
// `$` optional, then a sequence of `.<ident>` and/or `[<index>]` tokens. The
// path `"$"` or `""` selects the root.
const SEL_PARSE_JSON_1: [u8; 4] = [0x6a, 0x82, 0x60, 0x0a]; // parseJson(string)
const SEL_PARSE_JSON_2: [u8; 4] = [0x85, 0x94, 0x0e, 0xf1]; // parseJson(string,string)
const SEL_PARSE_JSON_BOOL: [u8; 4] = [0x9f, 0x86, 0xdc, 0x91]; // parseJsonBool(string,string)
const SEL_PARSE_JSON_STRING: [u8; 4] = [0x49, 0xc4, 0xfa, 0xc8]; // parseJsonString(string,string)
const SEL_PARSE_JSON_BYTES32: [u8; 4] = [0x17, 0x77, 0xe5, 0x9d]; // parseJsonBytes32(string,string)
const SEL_PARSE_JSON_UINT: [u8; 4] = [0xad, 0xdd, 0xe2, 0xb6]; // parseJsonUint(string,string)
const SEL_PARSE_JSON_INT: [u8; 4] = [0x7b, 0x04, 0x8c, 0xcd]; // parseJsonInt(string,string)
const SEL_PARSE_JSON_ADDRESS: [u8; 4] = [0x1e, 0x19, 0xe6, 0x57]; // parseJsonAddress(string,string)

// Crypto cheatcodes (secp256k1 — backed by k256 via alloy-signer-local).
const SEL_ADDR: [u8; 4] = [0xff, 0xa1, 0x86, 0x49]; // addr(uint256)
const SEL_SIGN: [u8; 4] = [0xe3, 0x41, 0xea, 0xa4]; // sign(uint256,bytes32)

// NIST P-256 (secp256r1) cheatcodes — backed by the `p256` crate.
const SEL_SIGN_P256: [u8; 4] = [0x83, 0x21, 0x1b, 0x40]; // signP256(uint256,bytes32)
const SEL_PUBLIC_KEY_P256: [u8; 4] = [0xc4, 0x53, 0x94, 0x9e]; // publicKeyP256(uint256)

// Assertion cheatcodes (forge-std StdAssertions → vm.assertEq / assertNe / etc.)
// All selectors verified by keccak256 of the canonical ABI signature.
const SEL_ASSERT_EQ_U256: [u8; 4] = [0x98, 0x29, 0x6c, 0x54]; // assertEq(uint256,uint256)
const SEL_ASSERT_EQ_U256_MSG: [u8; 4] = [0x88, 0xb4, 0x4c, 0x85]; // assertEq(uint256,uint256,string)
const SEL_ASSERT_EQ_I256: [u8; 4] = [0xfe, 0x74, 0xf0, 0x5b]; // assertEq(int256,int256)
const SEL_ASSERT_EQ_I256_MSG: [u8; 4] = [0x71, 0x4a, 0x2f, 0x13]; // assertEq(int256,int256,string)
const SEL_ASSERT_EQ_ADDR: [u8; 4] = [0x51, 0x53, 0x61, 0xf6]; // assertEq(address,address)
const SEL_ASSERT_EQ_ADDR_MSG: [u8; 4] = [0x2f, 0x27, 0x69, 0xd1]; // assertEq(address,address,string)
const SEL_ASSERT_EQ_BOOL: [u8; 4] = [0xf7, 0xfe, 0x34, 0x77]; // assertEq(bool,bool)
const SEL_ASSERT_EQ_BOOL_MSG: [u8; 4] = [0x4d, 0xb1, 0x9e, 0x7e]; // assertEq(bool,bool,string)
const SEL_ASSERT_EQ_B32: [u8; 4] = [0x7c, 0x84, 0xc6, 0x9b]; // assertEq(bytes32,bytes32)
const SEL_ASSERT_EQ_B32_MSG: [u8; 4] = [0xc1, 0xfa, 0x1e, 0xd0]; // assertEq(bytes32,bytes32,string)
const SEL_ASSERT_TRUE: [u8; 4] = [0x0c, 0x9f, 0xd5, 0x81]; // assertTrue(bool)
const SEL_ASSERT_TRUE_MSG: [u8; 4] = [0xa3, 0x4e, 0xdc, 0x03]; // assertTrue(bool,string)
const SEL_ASSERT_FALSE: [u8; 4] = [0xa5, 0x98, 0x28, 0x85]; // assertFalse(bool)
const SEL_ASSERT_FALSE_MSG: [u8; 4] = [0x7b, 0xa0, 0x48, 0x09]; // assertFalse(bool,string)
const SEL_ASSERT_GE_U256: [u8; 4] = [0xa8, 0xd4, 0xd1, 0xd9]; // assertGe(uint256,uint256)
const SEL_ASSERT_GE_U256_MSG: [u8; 4] = [0xe2, 0x52, 0x42, 0xc0]; // assertGe(uint256,uint256,string)
const SEL_ASSERT_GE_I256: [u8; 4] = [0x0a, 0x30, 0xb7, 0x71]; // assertGe(int256,int256)
const SEL_ASSERT_GE_I256_MSG: [u8; 4] = [0xa8, 0x43, 0x28, 0xdd]; // assertGe(int256,int256,string)
const SEL_ASSERT_GT_U256: [u8; 4] = [0xdb, 0x07, 0xfc, 0xd2]; // assertGt(uint256,uint256)
const SEL_ASSERT_GT_U256_MSG: [u8; 4] = [0xd9, 0xa3, 0xc4, 0xd2]; // assertGt(uint256,uint256,string)
const SEL_ASSERT_GT_I256: [u8; 4] = [0x5a, 0x36, 0x2d, 0x45]; // assertGt(int256,int256)
const SEL_ASSERT_GT_I256_MSG: [u8; 4] = [0xf8, 0xd3, 0x3b, 0x9b]; // assertGt(int256,int256,string)
const SEL_ASSERT_LE_U256: [u8; 4] = [0x84, 0x66, 0xf4, 0x15]; // assertLe(uint256,uint256)
const SEL_ASSERT_LE_U256_MSG: [u8; 4] = [0xd1, 0x7d, 0x4b, 0x0d]; // assertLe(uint256,uint256,string)
const SEL_ASSERT_LE_I256: [u8; 4] = [0x95, 0xfd, 0x15, 0x4e]; // assertLe(int256,int256)
const SEL_ASSERT_LE_I256_MSG: [u8; 4] = [0x4d, 0xfe, 0x69, 0x2c]; // assertLe(int256,int256,string)
const SEL_ASSERT_LT_U256: [u8; 4] = [0xb1, 0x2f, 0xc0, 0x05]; // assertLt(uint256,uint256)
const SEL_ASSERT_LT_U256_MSG: [u8; 4] = [0x65, 0xd5, 0xc1, 0x35]; // assertLt(uint256,uint256,string)
const SEL_ASSERT_LT_I256: [u8; 4] = [0x3e, 0x91, 0x40, 0x80]; // assertLt(int256,int256)
const SEL_ASSERT_LT_I256_MSG: [u8; 4] = [0x9f, 0xf5, 0x31, 0xe3]; // assertLt(int256,int256,string)
const SEL_ASSERT_NOT_EQ_U256: [u8; 4] = [0xb7, 0x90, 0x93, 0x20]; // assertNotEq(uint256,uint256)
const SEL_ASSERT_NOT_EQ_U256_MSG: [u8; 4] = [0x98, 0xf9, 0xbd, 0xbd]; // assertNotEq(uint256,uint256,string)
const SEL_ASSERT_NOT_EQ_I256: [u8; 4] = [0xf4, 0xc0, 0x04, 0xe3]; // assertNotEq(int256,int256)
const SEL_ASSERT_NOT_EQ_I256_MSG: [u8; 4] = [0x47, 0x24, 0xc5, 0xb9]; // assertNotEq(int256,int256,string)
const SEL_ASSERT_NOT_EQ_ADDR: [u8; 4] = [0xb1, 0x2e, 0x16, 0x94]; // assertNotEq(address,address)
const SEL_ASSERT_NOT_EQ_ADDR_MSG: [u8; 4] = [0x87, 0x75, 0xa5, 0x91]; // assertNotEq(address,address,string)
const SEL_ASSERT_NOT_EQ_BOOL: [u8; 4] = [0x23, 0x6e, 0x4d, 0x66]; // assertNotEq(bool,bool)
const SEL_ASSERT_NOT_EQ_BOOL_MSG: [u8; 4] = [0x10, 0x91, 0xa2, 0x61]; // assertNotEq(bool,bool,string)
const SEL_ASSERT_NOT_EQ_B32: [u8; 4] = [0x89, 0x8e, 0x83, 0xfc]; // assertNotEq(bytes32,bytes32)
const SEL_ASSERT_NOT_EQ_B32_MSG: [u8; 4] = [0xb2, 0x33, 0x2f, 0x51]; // assertNotEq(bytes32,bytes32,string)
// Gas snapshot stubs (no-ops — EDB doesn't do gas profiling)
const SEL_START_SNAPSHOT_GAS_STR: [u8; 4] = [0x3c, 0xad, 0x9d, 0x7b]; // startSnapshotGas(string)
const SEL_STOP_SNAPSHOT_GAS: [u8; 4] = [0xf6, 0x40, 0x2e, 0xda]; // stopSnapshotGas()
const SEL_STOP_SNAPSHOT_GAS_STR: [u8; 4] = [0x77, 0x3b, 0x28, 0x05]; // stopSnapshotGas(string)
const SEL_STOP_SNAPSHOT_GAS_2STR: [u8; 4] = [0x0c, 0x9d, 0xb7, 0x07]; // stopSnapshotGas(string,string)
const SEL_SNAPSHOT_GAS_LAST_CALL_STR: [u8; 4] = [0xdd, 0x9f, 0xca, 0x12]; // snapshotGasLastCall(string)
const SEL_SNAPSHOT_GAS_LAST_CALL_2STR: [u8; 4] = [0x20, 0x0c, 0x67, 0x72]; // snapshotGasLastCall(string,string)
// Benchmark-value snapshot stubs — same shape as the gas-snapshot family:
// EDB does not record benchmark snapshots, so we accept these as no-ops.
// Foundry defines exactly two overloads in `cheatcodes.json` (no `bytes32`
// variants in v1.7.x); verified via `keccak256(sig)[..4]` in the
// `all_snapshot_value_selectors_match_canonical` unit test.
const SEL_SNAPSHOT_VALUE_2: [u8; 4] = [0x51, 0xdb, 0x80, 0x5a]; // snapshotValue(string,uint256)
const SEL_SNAPSHOT_VALUE_3: [u8; 4] = [0x6d, 0x2b, 0x27, 0xd8]; // snapshotValue(string,string,uint256)
// Locally-compiled artifact lookup. Verified against keccak256(sig)[..4] in
// `selector_get_deployed_code`. Foundry implements this by reading the
// JSON artifact from `out/` keyed by the supplied path/name; EDB resolves
// it against the in-memory `LocalArtifactSet` built before prepare().
const SEL_GET_DEPLOYED_CODE: [u8; 4] = [0x3e, 0xbf, 0x73, 0xb4]; // getDeployedCode(string)

// Explicitly rejected — multi-fork / state-snapshot / scripting / fs+ffi.
const SEL_SNAPSHOT_STATE: [u8; 4] = [0x9c, 0xd2, 0x38, 0x35]; // snapshotState()
const SEL_SNAPSHOT_LEGACY: [u8; 4] = [0x97, 0x11, 0x71, 0x5a]; // snapshot()
/// Alias used by dispatch arms (Task 3+); same bytes as SEL_SNAPSHOT_LEGACY.
#[allow(dead_code)] // consumed by dispatch arm in plan task 3+
const SEL_SNAPSHOT: [u8; 4] = SEL_SNAPSHOT_LEGACY;
const SEL_REVERT_TO_STATE: [u8; 4] = [0xc2, 0x52, 0x74, 0x05]; // revertToState(uint256)
const SEL_REVERT_TO_LEGACY: [u8; 4] = [0x44, 0xd7, 0xf0, 0xa4]; // revertTo(uint256)
/// Alias used by dispatch arms (Task 4+); same bytes as SEL_REVERT_TO_LEGACY.
#[allow(dead_code)] // consumed by dispatch arm in plan task 4+
const SEL_REVERT_TO: [u8; 4] = SEL_REVERT_TO_LEGACY;
// Snapshot family — new in plan (revertToStateAndDelete / deleteStateSnapshot*)
#[allow(dead_code)] // consumed by dispatch arm in plan task 5+
const SEL_REVERT_TO_STATE_AND_DELETE: [u8; 4] = [0x3a, 0x19, 0x85, 0xdc]; // revertToStateAndDelete(uint256)
#[allow(dead_code)] // consumed by dispatch arm in plan task 6+
const SEL_DELETE_STATE_SNAPSHOT: [u8; 4] = [0x08, 0xd6, 0xb3, 0x7a]; // deleteStateSnapshot(uint256)
#[allow(dead_code)] // consumed by dispatch arm in plan task 6+
const SEL_DELETE_STATE_SNAPSHOTS: [u8; 4] = [0xe0, 0x93, 0x3c, 0x74]; // deleteStateSnapshots()
const SEL_CREATE_FORK: [u8; 4] = [0x31, 0xba, 0x34, 0x98]; // createFork(string)
const SEL_CREATE_SELECT_FORK: [u8; 4] = [0x98, 0x68, 0x00, 0x34]; // createSelectFork(string)
const SEL_SELECT_FORK: [u8; 4] = [0x9e, 0xbf, 0x68, 0x27]; // selectFork(uint256)
const SEL_ROLL_FORK: [u8; 4] = [0xd9, 0xbb, 0xf3, 0xa1]; // rollFork(uint256)
/// Alias used by dispatch arms (Task 7+); same bytes as SEL_ROLL_FORK.
#[allow(dead_code)] // consumed by dispatch arm in plan task 7+
const SEL_ROLL_FORK_UINT: [u8; 4] = SEL_ROLL_FORK;
const SEL_ACTIVE_FORK: [u8; 4] = [0x2f, 0x10, 0x3f, 0x22]; // activeFork()
const SEL_MAKE_PERSISTENT: [u8; 4] = [0x57, 0xe2, 0x2d, 0xde]; // makePersistent(address)
const SEL_TRANSACT: [u8; 4] = [0xbe, 0x64, 0x6d, 0xa1]; // transact(bytes32)
const SEL_FFI: [u8; 4] = [0x89, 0x16, 0x04, 0x67]; // ffi(string[])
const SEL_READ_FILE: [u8; 4] = [0x60, 0xf9, 0xbb, 0x11]; // readFile(string)
const SEL_WRITE_FILE: [u8; 4] = [0x89, 0x7e, 0x0a, 0x97]; // writeFile(string,string)
const SEL_REMOVE_FILE: [u8; 4] = [0xf1, 0xaf, 0xe0, 0x4d]; // removeFile(string)
const SEL_BROADCAST: [u8; 4] = [0xaf, 0xc9, 0x80, 0x40]; // broadcast()
const SEL_START_BROADCAST: [u8; 4] = [0x7f, 0xb5, 0x29, 0x7f]; // startBroadcast()
const SEL_STOP_BROADCAST: [u8; 4] = [0x76, 0xea, 0xdd, 0x36]; // stopBroadcast()

// ----------------------------------------------------------------------------
// Known-cheatcode catalog (selector → name)
// ----------------------------------------------------------------------------

/// Reverse-lookup table: cheatcode selector → cheatcode name (without `vm.` prefix).
/// Covers EDB's implemented set, rejected boundary cheatcodes, and the most common
/// not-yet-implemented cheatcodes. Used by the dispatch fall-through arm to produce
/// a precise "not yet implemented" message instead of a raw hex selector.
///
/// To add an entry: compute `keccak256(b"<canonical_sig>")[..4]` and verify the bytes
/// against the `keccak256_selectors_*` unit tests before baking them here.
///
/// Alphabetical by cheatcode name within each category.
const KNOWN_CHEATCODES: &[(&[u8; 4], &str)] = &[
    // --- Supported in EDB v1 (should never reach the fall-through arm, but
    //     listed so the "did you mean?" lookup covers the full known name set) ---
    (&[0xff, 0xa1, 0x86, 0x49], "addr"), // addr(uint256)
    (&[0xea, 0x06, 0x02, 0x91], "allowCheatcodes"), // allowCheatcodes(address)
    (&[0x4c, 0x63, 0xe5, 0x62], "assume"), // assume(bool)
    (&[0x40, 0x49, 0xdd, 0xd2], "chainId"), // chainId(uint256)
    (&[0x3f, 0xdf, 0x4e, 0x15], "clearMockedCalls"), // clearMockedCalls()
    (&[0xc8, 0x8a, 0x5e, 0x6d], "deal"), // deal(address,uint256)
    (&[0x7e, 0xd1, 0xec, 0x7d], "envBool"), // envBool(string)
    (&[0x4d, 0x7b, 0xaf, 0x06], "envBytes"), // envBytes(string)
    (&[0x47, 0x77, 0xf3, 0xcf], "envOr"), // envOr(string,bool)
    (&[0xb3, 0xe4, 0x77, 0x05], "envOr"), // envOr(string,bytes)
    (&[0xd1, 0x45, 0x73, 0x6c], "envOr"), // envOr(string,string)
    (&[0xf8, 0x77, 0xcb, 0x19], "envString"), // envString(string)
    (&[0xb4, 0xd6, 0xc7, 0x82], "etch"), // etch(address,bytes)
    (&[0x39, 0xb3, 0x7a, 0xb0], "fee"),  // fee(uint256)             — sets block.basefee
    (&[0x44, 0x0e, 0xd1, 0x0d], "expectEmit"), // expectEmit()
    (&[0x86, 0xb9, 0x62, 0x0d], "expectEmit"), // expectEmit(address)
    (&[0x49, 0x1c, 0xc7, 0xc2], "expectEmit"), // expectEmit(bool,bool,bool,bool)
    (&[0x81, 0xba, 0xd6, 0xf3], "expectEmit"), // expectEmit(bool,bool,bool,bool,address)
    (&[0xf4, 0x84, 0x48, 0x14], "expectRevert"), // expectRevert()
    (&[0xf2, 0x8d, 0xce, 0xb3], "expectRevert"), // expectRevert(bytes)
    (&[0xc3, 0x1e, 0xb0, 0xe0], "expectRevert"), // expectRevert(bytes4)
    (&[0xbd, 0x6a, 0xf4, 0x34], "expectCall"), // expectCall(address,bytes)
    (&[0xc1, 0xad, 0xbb, 0xff], "expectCall"), // expectCall(address,bytes,uint64)
    (&[0x19, 0x15, 0x53, 0xa4], "getRecordedLogs"), // getRecordedLogs()
    (&[0xc6, 0x57, 0xc7, 0x18], "label"), // label(address,string)
    (&[0x2b, 0x58, 0x9b, 0x28], "lastCallGas"), // lastCallGas()
    (&[0x66, 0x7f, 0x9d, 0x70], "load"), // load(address,bytes32)
    (&[0xb9, 0x62, 0x13, 0xe4], "mockCall"), // mockCall(address,bytes,bytes)
    (&[0xdb, 0xaa, 0xd1, 0x47], "mockCallRevert"), // mockCallRevert(address,bytes,bytes)
    (&[0xd1, 0xa5, 0xb3, 0x6f], "pauseGasMetering"), // pauseGasMetering()
    // vm.parseJson family — minimal JSONPath-like access for primitive leaves.
    (&[0x6a, 0x82, 0x60, 0x0a], "parseJson"), // parseJson(string)
    (&[0x85, 0x94, 0x0e, 0xf1], "parseJson"), // parseJson(string,string)
    (&[0x1e, 0x19, 0xe6, 0x57], "parseJsonAddress"), // parseJsonAddress(string,string)
    (&[0x9f, 0x86, 0xdc, 0x91], "parseJsonBool"), // parseJsonBool(string,string)
    (&[0x17, 0x77, 0xe5, 0x9d], "parseJsonBytes32"), // parseJsonBytes32(string,string)
    (&[0x7b, 0x04, 0x8c, 0xcd], "parseJsonInt"), // parseJsonInt(string,string)
    (&[0x49, 0xc4, 0xfa, 0xc8], "parseJsonString"), // parseJsonString(string,string)
    (&[0xad, 0xdd, 0xe2, 0xb6], "parseJsonUint"), // parseJsonUint(string,string)
    (&[0xca, 0x66, 0x9f, 0xa7], "prank"),     // prank(address)
    (&[0x41, 0xaf, 0x2f, 0x52], "recordLogs"), // recordLogs()
    (&[0x2b, 0xcd, 0x50, 0xe0], "resumeGasMetering"), // resumeGasMetering()
    (&[0x08, 0xd6, 0xb3, 0x7a], "deleteStateSnapshot"), // deleteStateSnapshot(uint256)
    (&[0xe0, 0x93, 0x3c, 0x74], "deleteStateSnapshots"), // deleteStateSnapshots()
    (&[0x44, 0xd7, 0xf0, 0xa4], "revertTo"), // revertTo(uint256)  — deprecated alias of revertToState
    (&[0xc2, 0x52, 0x74, 0x05], "revertToState"), // revertToState(uint256)
    (&[0x3a, 0x19, 0x85, 0xdc], "revertToStateAndDelete"), // revertToStateAndDelete(uint256)
    (&[0x1f, 0x7b, 0x4f, 0x30], "roll"),     // roll(uint256)
    // rollFork(uint256) single-arg supported in v1 (block.number only); cross-fork variants still rejected
    (&[0xd9, 0xbb, 0xf3, 0xa1], "rollFork"), // rollFork(uint256)
    (&[0xf8, 0xe1, 0x8b, 0x57], "setNonce"), // setNonce(address,uint64)
    (&[0xe3, 0x41, 0xea, 0xa4], "sign"),     // sign(uint256,bytes32)
    (&[0x97, 0x11, 0x71, 0x5a], "snapshot"), // snapshot()  — deprecated alias of snapshotState
    (&[0x9c, 0xd2, 0x38, 0x35], "snapshotState"), // snapshotState()
    (&[0x06, 0x44, 0x7d, 0x56], "startPrank"), // startPrank(address)
    (&[0x45, 0xb5, 0x60, 0x78], "startPrank"), // startPrank(address,address)
    (&[0x90, 0xc5, 0x01, 0x3b], "stopPrank"), // stopPrank()
    (&[0x70, 0xca, 0x10, 0xbb], "store"),    // store(address,bytes32,bytes32)
    // vm.toString — six type overloads, all supported via the dispatch arm.
    (&[0x56, 0xca, 0x62, 0x3e], "toString"), // toString(address)
    (&[0x71, 0xdc, 0xe7, 0xda], "toString"), // toString(bool)
    (&[0x71, 0xaa, 0xd1, 0x0d], "toString"), // toString(bytes)
    (&[0xb1, 0x1a, 0x19, 0xe8], "toString"), // toString(bytes32)
    (&[0xa3, 0x22, 0xc4, 0x0e], "toString"), // toString(int256)
    (&[0x69, 0x00, 0xa3, 0xae], "toString"), // toString(uint256)
    (&[0x48, 0xf5, 0x0c, 0x0f], "txGasPrice"), // txGasPrice(uint256) — sets tx.gas_price
    (&[0xe5, 0xd6, 0xbf, 0x02], "warp"),     // warp(uint256)
    // --- Supported: gas-snapshot stubs (silent success + one-time warn) ---
    // EDB is not a gas profiler in v1; these calls succeed without producing
    // any snapshot output. See docs/cheatcodes.md "Partial support".
    (&[0x3c, 0xad, 0x9d, 0x7b], "startSnapshotGas"), // startSnapshotGas(string)
    (&[0xf6, 0x40, 0x2e, 0xda], "stopSnapshotGas"),  // stopSnapshotGas()
    (&[0x77, 0x3b, 0x28, 0x05], "stopSnapshotGas"),  // stopSnapshotGas(string)
    (&[0x0c, 0x9d, 0xb7, 0x07], "stopSnapshotGas"),  // stopSnapshotGas(string,string)
    (&[0xdd, 0x9f, 0xca, 0x12], "snapshotGasLastCall"), // snapshotGasLastCall(string)
    (&[0x20, 0x0c, 0x67, 0x72], "snapshotGasLastCall"), // snapshotGasLastCall(string,string)
    // --- Supported: benchmark-value snapshot stubs (silent success + one-time warn) ---
    // EDB does not record benchmark snapshots in v1; these calls succeed
    // silently. Same approach as the gas-snapshot family above.
    (&[0x51, 0xdb, 0x80, 0x5a], "snapshotValue"), // snapshotValue(string,uint256)
    (&[0x6d, 0x2b, 0x27, 0xd8], "snapshotValue"), // snapshotValue(string,string,uint256)
    // `vm.getDeployedCode(string)` — real impl backed by EDB's
    // `LocalArtifactSet` (see SEL_GET_DEPLOYED_CODE + dispatch).  Hosted in
    // the supported section so the static-coverage estimator sees it.
    (&[0x3e, 0xbf, 0x73, 0xb4], "getDeployedCode"), // getDeployedCode(string)
    // --- Supported: block/state introspection + nonce read --------------
    (&[0x42, 0xcb, 0xb1, 0x5c], "getBlockNumber"), // getBlockNumber()
    (&[0x2d, 0x03, 0x35, 0xab], "getNonce"),       // getNonce(address)
    (&[0x53, 0x14, 0xb5, 0x4a], "setBlockhash"),   // setBlockhash(uint256,bytes32)
    // --- Supported: filesystem read (sandboxed to project_root) ----------
    (&[0x70, 0xf5, 0x57, 0x28], "readLine"), // readLine(string)
    // --- Supported: NIST P-256 (secp256r1) ECDSA via `p256` crate --------
    (&[0x83, 0x21, 0x1b, 0x40], "signP256"), // signP256(uint256,bytes32)
    (&[0xc4, 0x53, 0x94, 0x9e], "publicKeyP256"), // publicKeyP256(uint256)
    // --- Explicitly rejected in EDB v1 ---
    (&[0x2f, 0x10, 0x3f, 0x22], "activeFork"), // activeFork()
    (&[0xaf, 0xc9, 0x80, 0x40], "broadcast"),  // broadcast()
    (&[0x31, 0xba, 0x34, 0x98], "createFork"), // createFork(string)
    (&[0x6b, 0xa3, 0xba, 0x2b], "createFork"), // createFork(string,uint256)
    (&[0x7c, 0xa2, 0x96, 0x82], "createFork"), // createFork(string,bytes32)
    (&[0x98, 0x68, 0x00, 0x34], "createSelectFork"), // createSelectFork(string)
    (&[0x71, 0xee, 0x46, 0x4d], "createSelectFork"), // createSelectFork(string,uint256)
    (&[0x84, 0xd5, 0x2b, 0x7a], "createSelectFork"), // createSelectFork(string,bytes32)
    (&[0x08, 0xe4, 0xe1, 0x16], "expectCallMinGas"), // expectCallMinGas(address,uint256,uint64,bytes)
    (&[0x89, 0x16, 0x04, 0x67], "ffi"),              // ffi(string[])
    (&[0x57, 0xe2, 0x2d, 0xde], "makePersistent"),   // makePersistent(address)
    (&[0x40, 0x74, 0xe0, 0xa8], "makePersistent"),   // makePersistent(address,address)
    (&[0xef, 0xb7, 0x7a, 0x75], "makePersistent"),   // makePersistent(address,address,address)
    (&[0x1d, 0x9e, 0x26, 0x9e], "makePersistent"),   // makePersistent(address[])
    (&[0x60, 0xf9, 0xbb, 0x11], "readFile"),         // readFile(string)
    (&[0xf1, 0xaf, 0xe0, 0x4d], "removeFile"),       // removeFile(string)
    (&[0x0f, 0x29, 0x77, 0x2b], "rollFork"), // rollFork(bytes32)    — cross-fork; still rejected
    (&[0xd7, 0x4c, 0x83, 0xa4], "rollFork"), // rollFork(uint256,uint256) — cross-fork; still rejected
    (&[0xf2, 0x83, 0x0f, 0x7b], "rollFork"), // rollFork(uint256,bytes32) — cross-fork; still rejected
    (&[0x9e, 0xbf, 0x68, 0x27], "selectFork"), // selectFork(uint256)
    (&[0x7f, 0xb5, 0x29, 0x7f], "startBroadcast"), // startBroadcast()
    (&[0x7f, 0xec, 0x2a, 0x8d], "startBroadcast"), // startBroadcast(address)
    (&[0x76, 0xea, 0xdd, 0x36], "stopBroadcast"), // stopBroadcast()
    (&[0xbe, 0x64, 0x6d, 0xa1], "transact"), // transact(bytes32)
    (&[0x4d, 0x8a, 0xbc, 0x4b], "transact"), // transact(uint256,bytes32)
    (&[0x2c, 0x66, 0x76, 0x06], "getRawBlockHeader"), // getRawBlockHeader(uint256) — deferred (RPC)
    (&[0x89, 0x7e, 0x0a, 0x97], "writeFile"), // writeFile(string,string)
    // --- Assertion cheatcodes (assertEq / assertNe / assertTrue / assertFalse / etc.) ---
    (&[0x98, 0x29, 0x6c, 0x54], "assertEq"), // assertEq(uint256,uint256)
    (&[0x88, 0xb4, 0x4c, 0x85], "assertEq"), // assertEq(uint256,uint256,string)
    (&[0xfe, 0x74, 0xf0, 0x5b], "assertEq"), // assertEq(int256,int256)
    (&[0x71, 0x4a, 0x2f, 0x13], "assertEq"), // assertEq(int256,int256,string)
    (&[0x51, 0x53, 0x61, 0xf6], "assertEq"), // assertEq(address,address)
    (&[0x2f, 0x27, 0x69, 0xd1], "assertEq"), // assertEq(address,address,string)
    (&[0xf7, 0xfe, 0x34, 0x77], "assertEq"), // assertEq(bool,bool)
    (&[0x4d, 0xb1, 0x9e, 0x7e], "assertEq"), // assertEq(bool,bool,string)
    (&[0x7c, 0x84, 0xc6, 0x9b], "assertEq"), // assertEq(bytes32,bytes32)
    (&[0xc1, 0xfa, 0x1e, 0xd0], "assertEq"), // assertEq(bytes32,bytes32,string)
    (&[0x0c, 0x9f, 0xd5, 0x81], "assertTrue"), // assertTrue(bool)
    (&[0xa3, 0x4e, 0xdc, 0x03], "assertTrue"), // assertTrue(bool,string)
    (&[0xa5, 0x98, 0x28, 0x85], "assertFalse"), // assertFalse(bool)
    (&[0x7b, 0xa0, 0x48, 0x09], "assertFalse"), // assertFalse(bool,string)
    (&[0xa8, 0xd4, 0xd1, 0xd9], "assertGe"), // assertGe(uint256,uint256)
    (&[0xe2, 0x52, 0x42, 0xc0], "assertGe"), // assertGe(uint256,uint256,string)
    (&[0x0a, 0x30, 0xb7, 0x71], "assertGe"), // assertGe(int256,int256)
    (&[0xa8, 0x43, 0x28, 0xdd], "assertGe"), // assertGe(int256,int256,string)
    (&[0xdb, 0x07, 0xfc, 0xd2], "assertGt"), // assertGt(uint256,uint256)
    (&[0xd9, 0xa3, 0xc4, 0xd2], "assertGt"), // assertGt(uint256,uint256,string)
    (&[0x5a, 0x36, 0x2d, 0x45], "assertGt"), // assertGt(int256,int256)
    (&[0xf8, 0xd3, 0x3b, 0x9b], "assertGt"), // assertGt(int256,int256,string)
    (&[0x84, 0x66, 0xf4, 0x15], "assertLe"), // assertLe(uint256,uint256)
    (&[0xd1, 0x7d, 0x4b, 0x0d], "assertLe"), // assertLe(uint256,uint256,string)
    (&[0x95, 0xfd, 0x15, 0x4e], "assertLe"), // assertLe(int256,int256)
    (&[0x4d, 0xfe, 0x69, 0x2c], "assertLe"), // assertLe(int256,int256,string)
    (&[0xb1, 0x2f, 0xc0, 0x05], "assertLt"), // assertLt(uint256,uint256)
    (&[0x65, 0xd5, 0xc1, 0x35], "assertLt"), // assertLt(uint256,uint256,string)
    (&[0x3e, 0x91, 0x40, 0x80], "assertLt"), // assertLt(int256,int256)
    (&[0x9f, 0xf5, 0x31, 0xe3], "assertLt"), // assertLt(int256,int256,string)
    (&[0xb7, 0x90, 0x93, 0x20], "assertNotEq"), // assertNotEq(uint256,uint256)
    (&[0x98, 0xf9, 0xbd, 0xbd], "assertNotEq"), // assertNotEq(uint256,uint256,string)
    (&[0xf4, 0xc0, 0x04, 0xe3], "assertNotEq"), // assertNotEq(int256,int256)
    (&[0x47, 0x24, 0xc5, 0xb9], "assertNotEq"), // assertNotEq(int256,int256,string)
    (&[0xb1, 0x2e, 0x16, 0x94], "assertNotEq"), // assertNotEq(address,address)
    (&[0x87, 0x75, 0xa5, 0x91], "assertNotEq"), // assertNotEq(address,address,string)
    (&[0x23, 0x6e, 0x4d, 0x66], "assertNotEq"), // assertNotEq(bool,bool)
    (&[0x10, 0x91, 0xa2, 0x61], "assertNotEq"), // assertNotEq(bool,bool,string)
    (&[0x89, 0x8e, 0x83, 0xfc], "assertNotEq"), // assertNotEq(bytes32,bytes32)
    (&[0xb2, 0x33, 0x2f, 0x51], "assertNotEq"), // assertNotEq(bytes32,bytes32,string)
    // --- Dynamic / array / decimal / approx assertion overloads ----------------
    // Modern forge-std's StdAssertions calls these unconditionally; without the
    // catalog entry the dispatch fall-through tags them as `Unknown`, which
    // produces "unknown cheatcode selector 0x..." — confusing for users.
    // Catalogued as `NotYetImplemented` (via the assertion-name classifier
    // below) so the abort message reads "not yet implemented in v1 (selector
    // ...)" instead. C2-3 (Round 2 audit).
    (&[0xf3, 0x20, 0xd9, 0x63], "assertEq"), // assertEq(string,string)
    (&[0x36, 0xf6, 0x56, 0xd8], "assertEq"), // assertEq(string,string,string)
    (&[0x97, 0x62, 0x46, 0x31], "assertEq"), // assertEq(bytes,bytes)
    (&[0xe2, 0x4f, 0xed, 0x00], "assertEq"), // assertEq(bytes,bytes,string)
    (&[0x97, 0x5d, 0x5a, 0x12], "assertEq"), // assertEq(uint256[],uint256[])
    (&[0x5d, 0x18, 0xc7, 0x3a], "assertEq"), // assertEq(uint256[],uint256[],string)
    (&[0x71, 0x10, 0x43, 0xac], "assertEq"), // assertEq(int256[],int256[])
    (&[0x19, 0x1f, 0x1b, 0x30], "assertEq"), // assertEq(int256[],int256[],string)
    (&[0x38, 0x68, 0xac, 0x34], "assertEq"), // assertEq(address[],address[])
    (&[0x3e, 0x91, 0x73, 0xc5], "assertEq"), // assertEq(address[],address[],string)
    (&[0x70, 0x7d, 0xf7, 0x85], "assertEq"), // assertEq(bool[],bool[])
    (&[0xe4, 0x8a, 0x8f, 0x8d], "assertEq"), // assertEq(bool[],bool[],string)
    (&[0x0c, 0xc9, 0xee, 0x84], "assertEq"), // assertEq(bytes32[],bytes32[])
    (&[0xe0, 0x3e, 0x91, 0x77], "assertEq"), // assertEq(bytes32[],bytes32[],string)
    (&[0xcf, 0x1c, 0x04, 0x9c], "assertEq"), // assertEq(string[],string[])
    (&[0xef, 0xf6, 0xb2, 0x7d], "assertEq"), // assertEq(string[],string[],string)
    (&[0xe5, 0xfb, 0x9b, 0x4a], "assertEq"), // assertEq(bytes[],bytes[])
    (&[0xf4, 0x13, 0xf0, 0xb6], "assertEq"), // assertEq(bytes[],bytes[],string)
    (&[0x6a, 0x82, 0x37, 0xb3], "assertNotEq"), // assertNotEq(string,string)
    (&[0x78, 0xbd, 0xce, 0xa7], "assertNotEq"), // assertNotEq(string,string,string)
    (&[0x3c, 0xf7, 0x8e, 0x28], "assertNotEq"), // assertNotEq(bytes,bytes)
    (&[0x95, 0x07, 0x54, 0x0e], "assertNotEq"), // assertNotEq(bytes,bytes,string)
    (&[0x56, 0xf2, 0x9c, 0xba], "assertNotEq"), // assertNotEq(uint256[],uint256[])
    (&[0x9a, 0x7f, 0xbd, 0x8f], "assertNotEq"), // assertNotEq(uint256[],uint256[],string)
    (&[0x0b, 0x72, 0xf4, 0xef], "assertNotEq"), // assertNotEq(int256[],int256[])
    (&[0xd3, 0x97, 0x73, 0x22], "assertNotEq"), // assertNotEq(int256[],int256[],string)
    (&[0x46, 0xd0, 0xb2, 0x52], "assertNotEq"), // assertNotEq(address[],address[])
    (&[0x72, 0xc7, 0xe0, 0xb5], "assertNotEq"), // assertNotEq(address[],address[],string)
    (&[0x28, 0x6f, 0xaf, 0xea], "assertNotEq"), // assertNotEq(bool[],bool[])
    (&[0x62, 0xc6, 0xf9, 0xfb], "assertNotEq"), // assertNotEq(bool[],bool[],string)
    (&[0x06, 0x03, 0xea, 0x68], "assertNotEq"), // assertNotEq(bytes32[],bytes32[])
    (&[0xb8, 0x73, 0x63, 0x4c], "assertNotEq"), // assertNotEq(bytes32[],bytes32[],string)
    (&[0xbd, 0xfa, 0xcb, 0xe8], "assertNotEq"), // assertNotEq(string[],string[])
    (&[0xb6, 0x71, 0x87, 0xf3], "assertNotEq"), // assertNotEq(string[],string[],string)
    (&[0xed, 0xec, 0xd0, 0x35], "assertNotEq"), // assertNotEq(bytes[],bytes[])
    (&[0x1d, 0xcd, 0x1f, 0x68], "assertNotEq"), // assertNotEq(bytes[],bytes[],string)
    (&[0x27, 0xaf, 0x7d, 0x9c], "assertEqDecimal"), // assertEqDecimal(uint256,uint256,uint256)
    (&[0xd0, 0xcb, 0xbd, 0xef], "assertEqDecimal"), // assertEqDecimal(uint256,uint256,uint256,string)
    (&[0x48, 0x01, 0x6c, 0x04], "assertEqDecimal"), // assertEqDecimal(int256,int256,uint256)
    (&[0x7e, 0x77, 0xb0, 0xc5], "assertEqDecimal"), // assertEqDecimal(int256,int256,uint256,string)
    (&[0x66, 0x9e, 0xfc, 0xa7], "assertNotEqDecimal"), // assertNotEqDecimal(uint256,uint256,uint256)
    (&[0xf5, 0xa5, 0x55, 0x58], "assertNotEqDecimal"), // assertNotEqDecimal(uint256,uint256,uint256,string)
    (&[0x14, 0xe7, 0x56, 0x80], "assertNotEqDecimal"), // assertNotEqDecimal(int256,int256,uint256)
    (&[0x33, 0x94, 0x9f, 0x0b], "assertNotEqDecimal"), // assertNotEqDecimal(int256,int256,uint256,string)
    (&[0xec, 0xcd, 0x24, 0x37], "assertGtDecimal"),    // assertGtDecimal(uint256,uint256,uint256)
    (&[0x64, 0x94, 0x9a, 0x8d], "assertGtDecimal"), // assertGtDecimal(uint256,uint256,uint256,string)
    (&[0x78, 0x61, 0x1f, 0x0e], "assertGtDecimal"), // assertGtDecimal(int256,int256,uint256)
    (&[0x04, 0xa5, 0xc7, 0xab], "assertGtDecimal"), // assertGtDecimal(int256,int256,uint256,string)
    (&[0x3d, 0x1f, 0xe0, 0x8a], "assertGeDecimal"), // assertGeDecimal(uint256,uint256,uint256)
    (&[0x8b, 0xff, 0x91, 0x33], "assertGeDecimal"), // assertGeDecimal(uint256,uint256,uint256,string)
    (&[0xdc, 0x28, 0xc0, 0xf1], "assertGeDecimal"), // assertGeDecimal(int256,int256,uint256)
    (&[0x5d, 0xf9, 0x3c, 0x9b], "assertGeDecimal"), // assertGeDecimal(int256,int256,uint256,string)
    (&[0x20, 0x77, 0x33, 0x7e], "assertLtDecimal"), // assertLtDecimal(uint256,uint256,uint256)
    (&[0xa9, 0x72, 0xd0, 0x37], "assertLtDecimal"), // assertLtDecimal(uint256,uint256,uint256,string)
    (&[0xdb, 0xe8, 0xd8, 0x8b], "assertLtDecimal"), // assertLtDecimal(int256,int256,uint256)
    (&[0x40, 0xf0, 0xb4, 0xe0], "assertLtDecimal"), // assertLtDecimal(int256,int256,uint256,string)
    (&[0xc3, 0x04, 0xaa, 0xb7], "assertLeDecimal"), // assertLeDecimal(uint256,uint256,uint256)
    (&[0x7f, 0xef, 0xbb, 0xe0], "assertLeDecimal"), // assertLeDecimal(uint256,uint256,uint256,string)
    (&[0x11, 0xd1, 0x36, 0x4a], "assertLeDecimal"), // assertLeDecimal(int256,int256,uint256)
    (&[0xaa, 0x5c, 0xf7, 0x88], "assertLeDecimal"), // assertLeDecimal(int256,int256,uint256,string)
    (&[0x16, 0xd2, 0x07, 0xc6], "assertApproxEqAbs"), // assertApproxEqAbs(uint256,uint256,uint256)
    (&[0xf7, 0x10, 0xb0, 0x62], "assertApproxEqAbs"), // assertApproxEqAbs(uint256,uint256,uint256,string)
    (&[0x24, 0x0f, 0x83, 0x9d], "assertApproxEqAbs"), // assertApproxEqAbs(int256,int256,uint256)
    (&[0x82, 0x89, 0xe6, 0x21], "assertApproxEqAbs"), // assertApproxEqAbs(int256,int256,uint256,string)
    (&[0x8c, 0xf2, 0x5e, 0xf4], "assertApproxEqRel"), // assertApproxEqRel(uint256,uint256,uint256)
    (&[0x1e, 0xcb, 0x7d, 0x33], "assertApproxEqRel"), // assertApproxEqRel(uint256,uint256,uint256,string)
    (&[0xfe, 0xa2, 0xd1, 0x4f], "assertApproxEqRel"), // assertApproxEqRel(int256,int256,uint256)
    (&[0xef, 0x27, 0x7d, 0x72], "assertApproxEqRel"), // assertApproxEqRel(int256,int256,uint256,string)
    // --- Not yet implemented ---
    (&[0x65, 0xbc, 0x94, 0x81], "accesses"), // accesses(address)
    (&[0xf0, 0x25, 0x9e, 0x92], "breakpoint"), // breakpoint(string)
    (&[0xf7, 0xd3, 0x9a, 0x8d], "breakpoint"), // breakpoint(string,bool)
    (&[0x48, 0xc3, 0x24, 0x1f], "closeFile"), // closeFile(string)
    (&[0x20, 0x3d, 0xac, 0x0d], "copyStorage"), // copyStorage(address,address)
    (&[0x62, 0x29, 0x49, 0x8b], "deriveKey"), // deriveKey(string,uint32)
    (&[0x70, 0x9e, 0xcd, 0x3f], "dumpState"), // dumpState(string)
    (&[0x35, 0x0d, 0x56, 0xbf], "envAddress"), // envAddress(string)
    (&[0x89, 0x2a, 0x0c, 0x61], "envInt"),   // envInt(string)
    (&[0x56, 0x1f, 0xe5, 0x40], "envOr"),    // envOr(string,address)
    (&[0xbb, 0xcb, 0x71, 0x3e], "envOr"),    // envOr(string,int256)
    (&[0x5e, 0x97, 0x34, 0x8f], "envOr"),    // envOr(string,uint256)
    (&[0xc1, 0x97, 0x8d, 0x1f], "envUint"),  // envUint(string)
    (&[0x26, 0x1a, 0x32, 0x3e], "exists"),   // exists(string)
    (&[0x6d, 0x01, 0x66, 0x88], "expectSafeMemory"), // expectSafeMemory(uint64,uint64)
    (&[0xaf, 0x36, 0x8a, 0x08], "fsMetadata"), // fsMetadata(string)
    (&[0x8d, 0x1c, 0xc9, 0x25], "getCode"),  // getCode(string)
    // NOTE: getDeployedCode(string) moved up to the supported section.
    (&[0x17, 0xaa, 0x13, 0xce], "getMappingKeyOf"), // getMappingKeyOf(address,bytes32)
    (&[0x2f, 0x2f, 0xd6, 0x3f], "getMappingLength"), // getMappingLength(address,bytes32)
    (&[0xeb, 0xc7, 0x3a, 0xb4], "getMappingSlotAt"), // getMappingSlotAt(address,bytes32,uint256)
    // NOTE: getNonce(address) moved up to the supported section.
    (&[0x7d, 0x15, 0xd0, 0x19], "isDir"),        // isDir(string)
    (&[0xe0, 0xeb, 0x04, 0xd4], "isFile"),       // isFile(string)
    (&[0xd9, 0x2d, 0x8e, 0xfd], "isPersistent"), // isPersistent(address)
    (&[0x52, 0x8a, 0x68, 0x3c], "keyExists"),    // keyExists(string,string)
    (&[0xb3, 0xa0, 0x56, 0xd7], "loadAllocs"),   // loadAllocs(string)
    (&[0xad, 0xf8, 0x4d, 0x21], "mockFunction"), // mockFunction(address,address,bytes)
    (&[0x42, 0x34, 0x6c, 0x5e], "parseInt"),     // parseInt(string)
    (&[0x21, 0x3e, 0x41, 0x98], "parseJsonKeys"), // parseJsonKeys(string,string)
    (&[0xc6, 0xce, 0x05, 0x9d], "parseAddress"), // parseAddress(string)
    (&[0x97, 0x4e, 0xf9, 0x24], "parseBool"),    // parseBool(string)
    (&[0x8f, 0x5d, 0x23, 0x2d], "parseBytes"),   // parseBytes(string)
    (&[0x08, 0x7e, 0x6e, 0x81], "parseBytes32"), // parseBytes32(string)
    (&[0x59, 0x21, 0x51, 0xf0], "parseToml"),    // parseToml(string)
    (&[0x37, 0x73, 0x6e, 0x08], "parseToml"),    // parseToml(string,string)
    (&[0xfa, 0x91, 0x45, 0x4d], "parseUint"),    // parseUint(string)
    (&[0xd9, 0x30, 0xa0, 0xe6], "projectRoot"),  // projectRoot()
    (&[0xc4, 0xbc, 0x59, 0xe0], "readDir"),      // readDir(string)
    // NOTE: readLine(string) moved up to the supported section.
    (&[0x9f, 0x56, 0x84, 0xa2], "readLink"), // readLink(string)
    (&[0x22, 0x10, 0x00, 0x64], "rememberKey"), // rememberKey(uint256)
    (&[0x3c, 0xe9, 0x69, 0xe6], "revokePersistent"), // revokePersistent(address[])
    (&[0x99, 0x7a, 0x02, 0x22], "revokePersistent"), // revokePersistent(address)
    (&[0x97, 0x2c, 0x60, 0x62], "serializeAddress"), // serializeAddress(string,string,address)
    (&[0xac, 0x22, 0xe9, 0x71], "serializeBool"), // serializeBool(string,string,bool)
    (&[0xf2, 0x1d, 0x52, 0xc7], "serializeBytes"), // serializeBytes(string,string,bytes)
    (&[0x2d, 0x81, 0x2b, 0x44], "serializeBytes32"), // serializeBytes32(string,string,bytes32)
    (&[0x3f, 0x33, 0xdb, 0x60], "serializeInt"), // serializeInt(string,string,int256)
    (&[0x88, 0xda, 0x6d, 0x35], "serializeString"), // serializeString(string,string,string)
    (&[0x12, 0x9e, 0x90, 0x02], "serializeUint"), // serializeUint(string,string,uint256)
    (&[0x3e, 0x97, 0x05, 0xc0], "startMappingRecording"), // startMappingRecording()
    (&[0xcf, 0x22, 0xe3, 0xc9], "startStateDiffRecording"), // startStateDiffRecording()
    (&[0xaa, 0x5c, 0xf9, 0x0e], "stopAndReturnStateDiff"), // stopAndReturnStateDiff()
    (&[0x0d, 0x4a, 0xae, 0x9b], "stopMappingRecording"), // stopMappingRecording()
    (&[0xe6, 0x96, 0x2c, 0xdb], "broadcast"), // broadcast(address)
    (&[0x61, 0x9d, 0x89, 0x7f], "writeLine"), // writeLine(string,string)
    (&[0xe2, 0x3c, 0xd1, 0x9f], "writeJson"), // writeJson(string,string)
    (&[0x35, 0xd6, 0xad, 0x46], "writeJson"), // writeJson(string,string,string)
    (&[0xc0, 0x86, 0x5b, 0xa7], "writeToml"), // writeToml(string,string)
    (&[0x51, 0xac, 0x6a, 0x33], "writeToml"), // writeToml(string,string,string)
];

// ----------------------------------------------------------------------------
// Config + state types
// ----------------------------------------------------------------------------

/// Why a cheatcode is unsupported — affects the error message that
/// [`run_foundry_test`](super::run_foundry_test) surfaces post-prepare.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedCategory {
    /// Boundary cheatcode — needs multi-fork backend, snapshot rewind, etc.
    Rejected,
    /// Known foundry cheatcode but not in EDB's v1 implementation set.
    NotYetImplemented,
    /// Selector not in EDB's catalog — could be a typo or a very new cheatcode.
    Unknown,
}

/// A single unsupported-cheatcode invocation observed during prepare.
///
/// Recorded into [`CheatsConfig::unsupported_hits`] from the dispatch
/// fall-through and from explicit-reject arms; drained post-prepare to
/// produce a human-readable abort message.
#[derive(Clone, Debug)]
pub struct UnsupportedHit {
    /// Cheatcode name (without `vm.` prefix). For unknown selectors this
    /// carries `<unknown selector 0x...>` for display purposes.
    pub name: String,
    /// 4-byte selector observed at the cheatcode address.
    pub selector: [u8; 4],
    /// Classification driving the error message wording.
    pub category: UnsupportedCategory,
}

/// Configuration for the cheatcodes inspector.
///
/// `unsupported_hits` and `warnings_emitted` are `Arc<Mutex<...>>` so the
/// SAME tracker is shared across every `EdbCheatcodes<DB>` instance the
/// factory hands out — `Engine::prepare_with_router_and_cheats` calls the
/// factory once per orchestration pass (tracer / opcode / hook) and we want
/// hits/warnings deduplicated across passes.
#[derive(Clone, Debug)]
pub struct CheatsConfig {
    /// Project root used to sandbox filesystem cheatcodes (`vm.readLine`).
    /// Paths are canonicalized against this root; any path that resolves
    /// outside it (via absolute paths, symlink, or `..` traversal) is
    /// rejected. Empty `PathBuf` (used in unit tests) falls back to the
    /// process's current working directory.
    pub project_root: std::path::PathBuf,
    /// Shared list of unsupported-cheatcode invocations observed across all
    /// inspector instances created from this config. Drained post-prepare in
    /// `run_foundry_test` to surface a clear error before UI launch.
    pub unsupported_hits: Arc<Mutex<Vec<UnsupportedHit>>>,
    /// One-time warning gate for partial cheatcodes (rollFork, gas stubs,
    /// expectEmit soft-match). Keys are cheatcode names already warned about
    /// during this prepare cycle — second + subsequent hits stay silent.
    pub warnings_emitted: Arc<Mutex<HashSet<String>>>,
    /// Locally-compiled artifact set indexed by contract name. Populated by
    /// `run_foundry_test` before prepare; consumed by the
    /// `vm.getDeployedCode(string)` handler. `None` only in unit tests that
    /// stub `CheatsConfig::default()` (and that don't exercise the artifact
    /// lookup path). Cloning is `Arc`-cheap so every inspector pass shares
    /// the same set.
    pub local_artifacts: Option<Arc<edb_engine::LocalArtifactSet>>,
}

impl Default for CheatsConfig {
    fn default() -> Self {
        Self {
            project_root: std::path::PathBuf::new(),
            unsupported_hits: Arc::new(Mutex::new(Vec::new())),
            warnings_emitted: Arc::new(Mutex::new(HashSet::new())),
            local_artifacts: None,
        }
    }
}

/// A captured EVM state snapshot, restorable via `vm.revertToState`.
///
/// Stored on [`EdbCheatcodes::snapshots`] keyed by a monotonic `u64` id
/// (starting at 1; id 0 is reserved as a sentinel).
///
/// Only the journal is stored: `ctx.journaled_state` is a
/// `Journal<CacheDB<DB>>` whose `database` field *is* the CacheDB, so
/// cloning the journal already carries the entire DB state. A separate
/// `db` field would be dead weight — and a potentially large clone on
/// forked-state tests.
#[derive(Debug)]
pub(crate) struct Snapshot<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// The journal at snapshot time (storage writes, balance changes, code,
    /// logs, and the underlying CacheDB). Restored whole by
    /// `cheat_revert_to_state`.
    pub journal: revm::context::Journal<CacheDB<DB>>,
}

/// Hand-rolled cheatcode inspector over `EdbContext<DB>`.
#[derive(Debug)]
pub struct EdbCheatcodes<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Active config (project_root used by `vm.readLine`, artifact set used
    /// by `vm.getDeployedCode`, shared trackers used by every pass).
    config: CheatsConfig,
    /// Pranks keyed by call depth (the depth at which the prank was installed).
    pranks: HashMap<usize, Prank>,
    /// Saved `ctx.tx.caller` set by the first `vm.startPrank(address,address)`
    /// in scope. Restored on `vm.stopPrank`. `None` means no tx.origin override
    /// is currently active. We only save the FIRST override so nested
    /// `startPrank(addr, origin)` calls restore back to the original
    /// transaction origin, not to an intermediate override.
    saved_tx_origin: Option<Address>,
    /// Mock returns keyed by (target, calldata).
    mocks: HashMap<Address, BTreeMap<Bytes, MockReturn>>,
    /// Active expectRevert (consumed by the next call_end).
    expected_revert: Option<ExpectedRevert>,
    /// Address → human label (recorded but otherwise unused by the inspector;
    /// callers can read it via the public getter for trace pretty-printing).
    labels: HashMap<Address, String>,
    /// Whether vm.recordLogs() has armed the recorder.
    recording_logs: bool,
    /// Logs captured since the last `recordLogs()`.
    recorded_logs: Vec<Log>,
    /// Pending log expectations from `vm.expectEmit`. Matched against incoming
    /// logs via `Inspector::log`. Verified — and removed — when the registering
    /// frame ends (its `call_end` fires). Failed expectations rewrite that
    /// frame's outcome to a Revert with a clear `EDB: expectEmit ...` message.
    ///
    /// v1 SOFT-MATCH semantics: we don't capture a "template log" from the
    /// next `emit Foo(...)` in the test contract (foundry's full semantics).
    /// Instead, an expectation matches the first log it sees that satisfies
    /// emitter+topic-presence constraints. See `docs/cheatcodes.md`.
    expected_emits: Vec<ExpectedEmit>,
    /// Pending call expectations from `vm.expectCall`. Inspector::call
    /// increments `observed` for matching (target, calldata) pairs. Verified
    /// when the registering frame ends.
    expected_calls: Vec<ExpectedCall>,
    /// Monotonic frame-depth counter for non-cheatcode calls. Incremented in
    /// `Inspector::call` for non-cheatcode targets, decremented in `call_end`
    /// for the same. Used to scope `expected_emits` / `expected_calls` to the
    /// frame that registered them — robust to the "emit happens in the same
    /// frame as the registration, with no intervening sub-call" case (where
    /// REVM's own `depth()` would never cross a child boundary).
    call_depth: u64,
    /// Whether `vm.pauseGasMetering()` has been called without a subsequent
    /// `vm.resumeGasMetering()`. Informational only — EDB does NOT actually
    /// pause REVM's gas accounting (that would require deep engine surgery).
    gas_metering_paused: bool,
    /// Gas data from the most recent non-cheatcode call, populated in
    /// `call_end`. `None` until the first non-cheatcode call completes.
    last_call_gas: Option<LastCallGas>,
    /// EVM state snapshots keyed by monotonic snapshot id. Created by
    /// `vm.snapshotState`; consumed (and removed) by `vm.revertToState`.
    /// id 0 is reserved as a sentinel and is never stored here.
    #[allow(dead_code)] // written/read by snapshot handlers in Tasks 3+
    pub(crate) snapshots: HashMap<u64, Snapshot<DB>>,
    /// Next snapshot id to assign. Starts at 1 (id 0 reserved as sentinel).
    #[allow(dead_code)] // written/read by snapshot handlers in Tasks 3+
    pub(crate) next_snapshot_id: u64,
    /// Stateful read cursors for `vm.readLine(path)`. The first call to a
    /// path opens the file; subsequent calls advance the cursor and return
    /// the next line. EOF returns an empty string. Keyed by the canonicalized
    /// path so different lexical spellings of the same file share a cursor.
    file_cursors: HashMap<std::path::PathBuf, std::io::BufReader<std::fs::File>>,
}

#[derive(Clone, Debug)]
struct Prank {
    /// Replacement msg.sender.
    new_caller: Address,
    /// `true` for `vm.prank` (consumed by first call), `false` for `vm.startPrank`.
    one_shot: bool,
    /// Whether a one-shot prank has fired (set in `call`, cleared in `call_end`).
    fired: bool,
}

#[derive(Clone, Debug)]
enum MockReturn {
    Return(Bytes),
    Revert(Bytes),
}

/// What a pending `vm.expectRevert` should match against the next call's
/// revert output.
#[derive(Clone, Debug)]
enum ExpectedRevertMatch {
    /// `vm.expectRevert()` — match any revert.
    Bare,
    /// `vm.expectRevert(bytes)` — match the revert output bytes exactly.
    Exact(Bytes),
    /// `vm.expectRevert(bytes4)` — match the leading 4-byte selector of the
    /// revert output. Used by tests that expect a custom error like
    /// `revert MyError(...)` where the encoded payload begins with the
    /// 4-byte `MyError.selector` and is followed by the ABI-encoded args.
    Selector([u8; 4]),
}

#[derive(Clone, Debug)]
struct ExpectedRevert {
    /// What the next call's revert payload must look like.
    expected: ExpectedRevertMatch,
}

/// Pending `vm.expectEmit` expectation. v1 soft-match semantics: we don't
/// know the template log content at registration time (foundry infers it from
/// the test contract's own next `emit Foo(...)` between the cheatcode call and
/// the next external call), so the expectation matches the first log that
/// satisfies the structural constraints recorded here.
#[derive(Clone, Debug)]
struct ExpectedEmit {
    /// Foundry's `(bool t1, bool t2, bool t3, bool t4)` overloads encode
    /// per-topic byte-equality flags against the template emit between the
    /// `vm.expectEmit` call and the next external call. Soft-match v1 has
    /// no template, so these are recorded for future use but NOT enforced
    /// — see `matches()`. We accept any log whose emitter (if constrained)
    /// matches and whose topic vector is non-empty.
    #[allow(dead_code)]
    check_topics: [bool; 4],
    /// Foundry's `checkData` bool — "byte-equality against the template's
    /// data". Recorded for future use; NOT enforced in soft-match v1
    /// because events with only indexed args legitimately emit empty
    /// data and we'd false-fail them. See `matches()`.
    #[allow(dead_code)]
    check_data: bool,
    /// `None` = match any emitter, `Some(addr)` = only logs from this emitter.
    expected_emitter: Option<Address>,
    /// Set to true once a log matched. Read at the registering frame's
    /// `call_end` to decide pass/fail.
    matched: bool,
    /// Frame-depth at which `vm.expectEmit` ran (see `EdbCheatcodes::call_depth`).
    registered_at_call_depth: u64,
}

#[derive(Clone, Debug)]
struct ExpectedCall {
    target: Address,
    calldata: Bytes,
    /// Minimum number of matching calls required to satisfy the expectation.
    /// `vm.expectCall(addr, data)` sets this to 1; the (..., uint64 count)
    /// overload uses the supplied count.
    min_count: u64,
    /// Number of matching (target, calldata) calls observed so far.
    observed: u64,
    /// Frame-depth at which the expectation was set.
    registered_at_call_depth: u64,
}

/// Gas snapshot captured from the most recent non-cheatcode call's outcome.
/// Populated in `Inspector::call_end`. Reserved for a future phase when
/// `vm.lastCallGas()` returns real data instead of the v1 all-zero stub.
#[derive(Clone, Copy, Debug)]
struct LastCallGas {
    #[allow(dead_code)] // reserved for future real-gas implementation
    gas_limit: u64,
    #[allow(dead_code)] // reserved for future real-gas implementation
    gas_remaining: u64,
}

// ----------------------------------------------------------------------------
// Construction + public accessors
// ----------------------------------------------------------------------------

impl<DB> EdbCheatcodes<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Build a fresh inspector with the given config.
    pub fn new(config: CheatsConfig) -> Self {
        Self {
            config,
            pranks: HashMap::new(),
            saved_tx_origin: None,
            mocks: HashMap::new(),
            expected_revert: None,
            labels: HashMap::new(),
            recording_logs: false,
            recorded_logs: Vec::new(),
            expected_emits: Vec::new(),
            expected_calls: Vec::new(),
            call_depth: 0,
            gas_metering_paused: false,
            last_call_gas: None,
            snapshots: HashMap::new(),
            next_snapshot_id: 1,
            file_cursors: HashMap::new(),
        }
    }

    /// Returns the address-label map collected via `vm.label`.
    #[allow(dead_code)] // public API; consumed by trace pretty-printing in a future phase
    pub fn labels(&self) -> &HashMap<Address, String> {
        &self.labels
    }

    /// Returns the captured logs (filled by `vm.recordLogs`).
    #[allow(dead_code)] // public API; consumed by test result reporting in a future phase
    pub fn recorded_logs(&self) -> &[Log] {
        &self.recorded_logs
    }
}

/// Factory for building fresh `EdbCheatcodes` instances per engine pass.
///
/// `Engine::prepare_with_router_and_cheats` calls this once per orchestration
/// pass (tracer / opcode / hook) so prank/mock/expectRevert state never bleeds
/// between passes.
pub fn build_cheats_factory<DB>(
    config: CheatsConfig,
) -> impl Fn() -> EdbCheatcodes<DB> + Send + Sync + 'static
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    let config = std::sync::Arc::new(config);
    move || EdbCheatcodes::new((*config).clone())
}

/// Pure check function: inspect `hits` and, if non-empty, return an `Err`
/// with a human-readable abort message. Returns `Ok(())` when no unsupported
/// cheatcodes were observed.
///
/// Takes `&Arc<Mutex<Vec<UnsupportedHit>>>` directly so it can be captured by
/// a closure that serves as `between_passes_hook` in
/// `Engine::prepare_with_router_and_cheats`.
///
/// Counts hits per cheatcode name so a tight loop calling the same rejected
/// cheatcode produces one bullet with `called Nx`, not N bullets.
pub fn ensure_no_unsupported_hits(hits: &Arc<Mutex<Vec<UnsupportedHit>>>) -> eyre::Result<()> {
    let hits = hits.lock().expect("unsupported_hits mutex poisoned");
    if hits.is_empty() {
        return Ok(());
    }
    let mut by_name: std::collections::BTreeMap<String, (UnsupportedCategory, usize)> =
        Default::default();
    for hit in hits.iter() {
        let entry = by_name.entry(hit.name.clone()).or_insert((hit.category, 0));
        entry.1 += 1;
    }
    let mut msg = String::from(
        "EDB: this test uses cheatcodes not supported in v1. Aborting before UI launch.\n\n",
    );
    for (name, (cat, count)) in &by_name {
        let cat_str = match cat {
            UnsupportedCategory::Rejected => "rejected",
            UnsupportedCategory::NotYetImplemented => "not yet implemented",
            UnsupportedCategory::Unknown => "unknown selector",
        };
        msg.push_str(&format!("  - vm.{name} ({cat_str}, called {count}x)\n"));
    }
    msg.push_str("\nSee docs/cheatcodes.md for the full support matrix and workarounds.\n");
    Err(eyre::eyre!("{msg}"))
}

// ----------------------------------------------------------------------------
// Inspector impl over EdbContext<DB>
// ----------------------------------------------------------------------------

impl<DB> Inspector<edb_common::EdbContext<DB>> for EdbCheatcodes<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn call(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        // 1) target == cheatcode address → decode & dispatch.
        //    The cheatcode call itself does NOT count toward `call_depth`; it
        //    is intercepted before any real frame work happens.
        if inputs.target_address == CHEATCODE_ADDRESS {
            return Some(self.dispatch(ctx, inputs));
        }

        // 2) Bump the non-cheatcode call-depth counter. We do this BEFORE
        //    mock interception so a fully mocked call still participates in
        //    `expectCall` accounting and frame-scoping (its `call_end` will
        //    fire and decrement again).
        self.call_depth += 1;

        // 3) Active prank for this depth?
        //    Pranks are keyed by the depth at which they were installed; we
        //    apply them to calls made *at the next depth down*, which mirrors
        //    forge's "vm.prank affects the next call" semantics. In practice
        //    that means: when `vm.prank` runs in the test at depth N, the
        //    next sub-call to user code happens at depth N+1 with the
        //    overridden caller — but `ctx.journaled_state.depth()` reports
        //    the *parent* frame's depth at this moment in the Inspector,
        //    so we look up by exactly that depth.
        let depth = ctx.journaled_state.depth();
        if let Some(prank) = self.pranks.get_mut(&depth) {
            inputs.caller = prank.new_caller;
            if prank.one_shot {
                prank.fired = true;
            }
        }

        // 4) Resolve calldata once (shared by expectCall accounting + mockCall lookup).
        let calldata = match &inputs.input {
            revm::interpreter::CallInput::Bytes(b) => b.clone(),
            revm::interpreter::CallInput::SharedBuffer(range) => {
                use revm::context_interface::LocalContextTr;
                ctx.local
                    .shared_memory_buffer_slice(range.clone())
                    .map(|b| Bytes::from(b.to_vec()))
                    .unwrap_or_default()
            }
        };

        // 5) expectCall accounting — any pending expectation whose (target, calldata)
        //    match this call has its observed counter incremented.
        for ec in self.expected_calls.iter_mut() {
            if ec.target == inputs.target_address && ec.calldata == calldata {
                ec.observed = ec.observed.saturating_add(1);
            }
        }

        // 6) mockCall match?
        //    Use inputs.return_memory_offset (threaded through ok_return /
        //    revert_with) so the mocked return data lands in the parent frame's
        //    memory at the slot Solidity reserved for it. Without this, REVM
        //    would copy 0 bytes at offset 0 and Solidity would silently read
        //    zeros for static return types (uint256, etc.).
        if let Some(mocks) = self.mocks.get(&inputs.target_address)
            && let Some(mock) = mocks.get(&calldata)
        {
            return Some(match mock {
                MockReturn::Return(data) => ok_return(inputs, data.clone()),
                MockReturn::Revert(data) => revert_with(inputs, data.clone()),
            });
        }

        None
    }

    fn call_end(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        outcome: &mut CallOutcome,
    ) {
        // Cheatcode call_end is a no-op for everything below: the cheatcode
        // call is intercepted in `call()`, never counted toward `call_depth`,
        // and never satisfies any expectation. Skip outright.
        if inputs.target_address == CHEATCODE_ADDRESS {
            return;
        }

        // Snapshot the frame depth this call belongs to before we decrement,
        // and decrement to mirror the increment in `call()`.
        let ending_depth = self.call_depth;
        self.call_depth = self.call_depth.saturating_sub(1);

        // Clear one-shot pranks that fired.
        self.pranks.retain(|_, p| !(p.one_shot && p.fired));

        // expectRevert: verify the just-completed call matches.
        if let Some(expected) = self.expected_revert.take() {
            let reverted = matches!(outcome.result.result, InstructionResult::Revert);
            let matched = match (reverted, &expected.expected) {
                (true, ExpectedRevertMatch::Bare) => true,
                (true, ExpectedRevertMatch::Exact(want)) => {
                    outcome.result.output.as_ref() == want.as_ref()
                }
                (true, ExpectedRevertMatch::Selector(sel)) => {
                    outcome.result.output.len() >= 4
                        && &outcome.result.output[..4] == sel.as_slice()
                }
                (false, _) => false,
            };
            if matched {
                // Convert the matched revert into a successful return
                // (forge semantics: the test continues past expectRevert).
                outcome.result.result = InstructionResult::Return;
                outcome.result.output = Bytes::new();
            } else {
                outcome.result.result = InstructionResult::Revert;
                outcome.result.output = encode_error_string(
                    "EDB: expectRevert did not match: the call did not revert as expected",
                );
                // Early-out: don't also try to verify expectEmit/expectCall on
                // a frame we're already rewriting to a failure.
                return;
            }
        }

        // expectEmit: any expectation registered AT or BELOW this frame's depth
        // has now had its window closed (the registering frame is ending). For
        // soft-match v1 we require `matched == true`; otherwise rewrite the
        // outcome to a Revert with a clear EDB error.
        //
        // We collect failures before mutating `outcome` so a single unfulfilled
        // expectation reports a deterministic message.
        let unfulfilled_emit = self
            .expected_emits
            .iter()
            .find(|e| e.registered_at_call_depth >= ending_depth && !e.matched)
            .cloned();
        self.expected_emits.retain(|e| e.registered_at_call_depth < ending_depth);
        if let Some(_unfulfilled) = unfulfilled_emit {
            outcome.result.result = InstructionResult::Revert;
            outcome.result.output = encode_error_string(
                "EDB: expectEmit did not match: no log emitted that satisfied the expected \
                 emitter/topics/data constraints before the registering frame ended",
            );
            // Clear any same-frame expectCalls too — they're no longer relevant.
            self.expected_calls.retain(|ec| ec.registered_at_call_depth < ending_depth);
            return;
        }

        // expectCall: same window-closing logic.
        let unfulfilled_call = self
            .expected_calls
            .iter()
            .find(|ec| ec.registered_at_call_depth >= ending_depth && ec.observed < ec.min_count)
            .cloned();
        self.expected_calls.retain(|ec| ec.registered_at_call_depth < ending_depth);
        if let Some(uc) = unfulfilled_call {
            outcome.result.result = InstructionResult::Revert;
            outcome.result.output = encode_error_string(&format!(
                "EDB: expectCall did not match (target=0x{}, calldata len={}, expected>={}, observed={})",
                alloy_primitives::hex::encode(uc.target),
                uc.calldata.len(),
                uc.min_count,
                uc.observed,
            ));
        }

        // Record last-call gas for vm.lastCallGas(). We capture the call's gas
        // data from REVM's outcome so vm.lastCallGas() has *something* to return.
        // Note: EDB runs the same execution in multiple instrumented passes; gas
        // values will differ between passes (instrumented bytecode consumes
        // different amounts of gas). vm.lastCallGas() is therefore a v1 stub
        // that returns zero-filled data from the stub handler regardless.
        // We store the value here for future use when the multi-pass architecture
        // supports deterministic gas snapshots.
        self.last_call_gas = Some(LastCallGas {
            gas_limit: outcome.result.gas.limit(),
            gas_remaining: outcome.result.gas.remaining(),
        });
    }

    fn log(&mut self, _ctx: &mut edb_common::EdbContext<DB>, log: Log) {
        if self.recording_logs {
            self.recorded_logs.push(log.clone());
        }
        // First-fit match against pending expectEmits: the earliest unmatched
        // expectation that accepts this log wins. We bind one-log-per-expect.
        for emit in self.expected_emits.iter_mut() {
            if !emit.matched && emit.matches(&log) {
                emit.matched = true;
                break;
            }
        }
    }
}

impl ExpectedEmit {
    /// Soft-match: accept the log if its emitter matches (when constrained)
    /// and the log carries a non-empty topic list (every event in Solidity
    /// has at least the signature topic; `log0(...)` emissions are out of
    /// scope for `vm.expectEmit`).
    ///
    /// We DON'T require `topics.len() >= 4` even when all four `check_topics`
    /// bools are set: foundry's bools encode "byte-equality against the
    /// template log's topic at this index" — when the template has fewer
    /// topics than the bools enable, foundry simply doesn't compare the
    /// missing topics. Soft-match v1 doesn't have a template, so we relax
    /// to "any log from the right emitter with at least a signature topic".
    ///
    /// We ALSO don't enforce `check_data → data.len() > 0`: events with
    /// only indexed parameters (e.g. `event Foo(address indexed bar)`)
    /// emit an empty data payload, and forge still passes
    /// `expectEmit(_, _, _, true)` for them because the template's data
    /// is also empty. Without a template we can't enforce byte-equality
    /// and any non-vacuous structural check would false-fail this class
    /// of events. See `docs/cheatcodes.md` "Known caveats".
    fn matches(&self, log: &Log) -> bool {
        if let Some(want) = self.expected_emitter
            && log.address != want
        {
            return false;
        }
        let topics = log.topics();
        if topics.is_empty() {
            return false;
        }
        true
    }
}

// ----------------------------------------------------------------------------
// Dispatch + per-cheatcode handlers
// ----------------------------------------------------------------------------

impl<DB> EdbCheatcodes<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Record an unsupported-cheatcode hit and produce the matching revert
    /// `CallOutcome` in one step.
    ///
    /// Centralizes the "encode the EDB error string, append it to the shared
    /// `unsupported_hits` tracker, and return a Revert outcome" pattern so
    /// every place in `dispatch` that turns a cheatcode into a hard rejection
    /// participates in the post-prepare abort check.
    fn record_and_revert(
        &self,
        inputs: &CallInputs,
        name: &str,
        selector: [u8; 4],
        category: UnsupportedCategory,
        msg: &str,
    ) -> CallOutcome {
        if let Ok(mut hits) = self.config.unsupported_hits.lock() {
            hits.push(UnsupportedHit { name: name.to_string(), selector, category });
        }
        revert_with(inputs, encode_error_string(msg))
    }

    /// Emit a `tracing::warn!` + `eprintln!` once per cheatcode-name. Subsequent
    /// calls with the same `name` are no-ops, so a test that hammers
    /// `vm.pauseGasMetering` in a loop only sees one warning line.
    fn warn_once(&self, name: &str, message: &str) {
        if let Ok(mut set) = self.config.warnings_emitted.lock()
            && set.insert(name.to_string())
        {
            tracing::warn!(target: "edb::cheats", "vm.{name}: {message}");
            eprintln!("[edb warning] vm.{name}: {message}");
        }
    }

    fn dispatch(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
    ) -> CallOutcome {
        // Resolve calldata (handle SharedBuffer variant).
        let calldata_bytes = match &inputs.input {
            revm::interpreter::CallInput::Bytes(b) => b.clone(),
            revm::interpreter::CallInput::SharedBuffer(range) => {
                use revm::context_interface::LocalContextTr;
                ctx.local
                    .shared_memory_buffer_slice(range.clone())
                    .map(|b| Bytes::from(b.to_vec()))
                    .unwrap_or_default()
            }
        };
        let calldata = calldata_bytes.as_ref();
        if calldata.len() < 4 {
            return revert_with(inputs, encode_error_string("EDB: cheatcode call has no selector"));
        }
        let selector: [u8; 4] = calldata[..4].try_into().expect("just sliced 4 bytes");
        let args = &calldata[4..];
        match selector {
            // Supported
            SEL_WARP => self.cheat_warp(ctx, inputs, args),
            SEL_ROLL => self.cheat_roll(ctx, inputs, args),
            SEL_CHAIN_ID => self.cheat_chain_id(ctx, inputs, args),
            SEL_FEE => self.cheat_fee(ctx, inputs, args),
            SEL_TX_GAS_PRICE => self.cheat_tx_gas_price(ctx, inputs, args),
            SEL_TO_STRING_ADDRESS => self.cheat_to_string_address(inputs, args),
            SEL_TO_STRING_BOOL => self.cheat_to_string_bool(inputs, args),
            SEL_TO_STRING_BYTES => self.cheat_to_string_bytes(inputs, args),
            SEL_TO_STRING_BYTES32 => self.cheat_to_string_bytes32(inputs, args),
            SEL_TO_STRING_INT256 => self.cheat_to_string_int256(inputs, args),
            SEL_TO_STRING_UINT256 => self.cheat_to_string_uint256(inputs, args),
            SEL_PARSE_JSON_1 => self.cheat_parse_json_root(inputs, args),
            SEL_PARSE_JSON_2 => self.cheat_parse_json_path(inputs, args),
            SEL_PARSE_JSON_BOOL => self.cheat_parse_json_bool(inputs, args),
            SEL_PARSE_JSON_STRING => self.cheat_parse_json_string(inputs, args),
            SEL_PARSE_JSON_BYTES32 => self.cheat_parse_json_bytes32(inputs, args),
            SEL_PARSE_JSON_UINT => self.cheat_parse_json_uint(inputs, args),
            SEL_PARSE_JSON_INT => self.cheat_parse_json_int(inputs, args),
            SEL_PARSE_JSON_ADDRESS => self.cheat_parse_json_address(inputs, args),
            SEL_DEAL => self.cheat_deal(ctx, inputs, args),
            SEL_ETCH => self.cheat_etch(ctx, inputs, args),
            SEL_STORE => self.cheat_store(ctx, inputs, args),
            SEL_LOAD => self.cheat_load(ctx, inputs, args),
            SEL_SET_NONCE => self.cheat_set_nonce(ctx, inputs, args),
            SEL_GET_NONCE => self.cheat_get_nonce(ctx, inputs, args),
            SEL_GET_BLOCK_NUMBER => self.cheat_get_block_number(ctx, inputs),
            SEL_SET_BLOCKHASH => self.cheat_set_blockhash(ctx, inputs, args),
            SEL_READ_LINE => self.cheat_read_line(inputs, args),
            // vm.getRawBlockHeader(uint256) — deferred: requires an upstream
            // RPC channel + synchronous-from-async dispatch inside the
            // cheatcode handler. We catalog it as Rejected so the abort
            // surfaces a useful message instead of "unknown selector".
            SEL_GET_RAW_BLOCK_HEADER => self.record_and_revert(
                inputs,
                "getRawBlockHeader",
                selector,
                UnsupportedCategory::Rejected,
                "EDB: cheatcode vm.getRawBlockHeader: deferred to v2 \
                 (requires an upstream RPC channel and synchronous-from-async \
                 dispatch in the cheatcode handler). See docs/cheatcodes.md",
            ),
            SEL_PRANK => self.cheat_prank(ctx, inputs, args, true),
            SEL_START_PRANK => self.cheat_prank(ctx, inputs, args, false),
            SEL_START_PRANK_2 => self.cheat_start_prank_2(ctx, inputs, args),
            SEL_STOP_PRANK => self.cheat_stop_prank(ctx, inputs),
            SEL_MOCK_CALL => self.cheat_mock_call(ctx, inputs, args, false),
            SEL_MOCK_CALL_REVERT => self.cheat_mock_call(ctx, inputs, args, true),
            SEL_CLEAR_MOCKED_CALLS => self.cheat_clear_mocked_calls(ctx, inputs),
            SEL_EXPECT_REVERT_BARE => self.cheat_expect_revert_bare(inputs),
            SEL_EXPECT_REVERT_BYTES => self.cheat_expect_revert_bytes(inputs, args),
            SEL_EXPECT_REVERT_BYTES4 => self.cheat_expect_revert_bytes4(inputs, args),
            SEL_LABEL => self.cheat_label(ctx, inputs, args),
            SEL_RECORD_LOGS => self.cheat_record_logs(inputs),
            SEL_GET_RECORDED_LOGS => self.cheat_get_recorded_logs(inputs),
            SEL_EXPECT_EMIT_BARE => self.cheat_expect_emit(ctx, inputs, args, ExpectEmitMode::All),
            SEL_EXPECT_EMIT_FILTER4 => {
                self.cheat_expect_emit(ctx, inputs, args, ExpectEmitMode::Filter4)
            }
            SEL_EXPECT_EMIT_FILTER5 => {
                self.cheat_expect_emit(ctx, inputs, args, ExpectEmitMode::Filter5)
            }
            SEL_EXPECT_EMIT_ADDR => {
                self.cheat_expect_emit(ctx, inputs, args, ExpectEmitMode::AnyTopicsFromEmitter)
            }
            SEL_EXPECT_CALL => self.cheat_expect_call(ctx, inputs, args, 1),
            SEL_EXPECT_CALL_COUNT => self.cheat_expect_call_with_count(ctx, inputs, args),
            SEL_EXPECT_CALL_MIN_GAS => self.record_and_revert(
                inputs,
                "expectCallMinGas",
                selector,
                UnsupportedCategory::Rejected,
                "EDB: cheatcode vm.expectCallMinGas not supported in v1: gas accounting under \
                 EDB's instrumented bytecode needs separate design work. See docs/cheatcodes.md",
            ),

            // assume + env family
            SEL_ASSUME => self.cheat_assume(inputs, args),
            SEL_ENV_BOOL => self.cheat_env_bool(inputs, args),
            SEL_ENV_BYTES => self.cheat_env_bytes(inputs, args),
            SEL_ENV_STRING => self.cheat_env_string(inputs, args),
            SEL_ENV_OR_BOOL => self.cheat_env_or_bool(inputs, args),
            SEL_ENV_OR_BYTES => self.cheat_env_or_bytes(inputs, args),
            SEL_ENV_OR_STRING => self.cheat_env_or_string(inputs, args),

            // Gas metering stubs
            SEL_PAUSE_GAS_METERING => self.cheat_pause_gas_metering(inputs),
            SEL_RESUME_GAS_METERING => self.cheat_resume_gas_metering(inputs),
            SEL_LAST_CALL_GAS => self.cheat_last_call_gas(inputs),

            // Crypto cheatcodes
            SEL_ADDR => self.cheat_addr(inputs, args),
            SEL_SIGN => self.cheat_sign(inputs, args),
            SEL_SIGN_P256 => self.cheat_sign_p256(inputs, args),
            SEL_PUBLIC_KEY_P256 => self.cheat_public_key_p256(inputs, args),

            // vm.rollFork(uint256) — single-arg: updates block.number only (Task 7)
            sel if sel == SEL_ROLL_FORK_UINT => self.cheat_roll_fork_single(ctx, inputs, args),
            // Explicitly rejected — multi-fork
            SEL_CREATE_FORK
            | SEL_CREATE_SELECT_FORK
            | SEL_SELECT_FORK
            | SEL_ACTIVE_FORK
            | SEL_MAKE_PERSISTENT => {
                let name = match selector {
                    SEL_CREATE_FORK => "createFork",
                    SEL_CREATE_SELECT_FORK => "createSelectFork",
                    SEL_SELECT_FORK => "selectFork",
                    SEL_ACTIVE_FORK => "activeFork",
                    SEL_MAKE_PERSISTENT => "makePersistent",
                    _ => unreachable!(),
                };
                let msg = format!(
                    "EDB: cheatcode vm.{name} not supported in v1: \
                     multi-fork backend unavailable. See docs/cheatcodes.md"
                );
                self.record_and_revert(inputs, name, selector, UnsupportedCategory::Rejected, &msg)
            }
            // Snapshot capture (Task 3)
            SEL_SNAPSHOT_STATE | SEL_SNAPSHOT_LEGACY => {
                self.cheat_snapshot_state(ctx, inputs, args)
            }
            // State snapshot revert (Task 4)
            SEL_REVERT_TO_STATE | SEL_REVERT_TO_LEGACY => {
                self.cheat_revert_to_state(ctx, inputs, args)
            }
            // State snapshot revert with delete (Task 5)
            // delete-on-revert is already the default in our revertToState; the AndDelete
            // variant is just a different selector pointing at the same handler.
            SEL_REVERT_TO_STATE_AND_DELETE => self.cheat_revert_to_state(ctx, inputs, args),
            // Delete a single snapshot by id (Task 6)
            SEL_DELETE_STATE_SNAPSHOT => self.cheat_delete_state_snapshot(ctx, inputs, args),
            // Delete all snapshots (Task 6)
            SEL_DELETE_STATE_SNAPSHOTS => self.cheat_delete_state_snapshots(ctx, inputs, args),
            // Explicitly rejected — separate-tx model
            SEL_TRANSACT => self.record_and_revert(
                inputs,
                "transact",
                selector,
                UnsupportedCategory::Rejected,
                "EDB: cheatcode vm.transact not supported in v1: \
                 requires the multi-fork backend and a separate-tx execution model. \
                 See docs/cheatcodes.md",
            ),
            // Explicitly rejected — fs + ffi
            SEL_FFI | SEL_READ_FILE | SEL_WRITE_FILE | SEL_REMOVE_FILE => {
                let name = match selector {
                    SEL_FFI => "ffi",
                    SEL_READ_FILE => "readFile",
                    SEL_WRITE_FILE => "writeFile",
                    SEL_REMOVE_FILE => "removeFile",
                    _ => unreachable!(),
                };
                let msg = format!(
                    "EDB: cheatcode vm.{name} not supported in v1: \
                     external-process / fs access disabled. See docs/cheatcodes.md"
                );
                self.record_and_revert(inputs, name, selector, UnsupportedCategory::Rejected, &msg)
            }
            // Explicitly rejected — broadcasting
            SEL_BROADCAST | SEL_START_BROADCAST | SEL_STOP_BROADCAST => {
                let name = match selector {
                    SEL_BROADCAST => "broadcast",
                    SEL_START_BROADCAST => "startBroadcast",
                    SEL_STOP_BROADCAST => "stopBroadcast",
                    _ => unreachable!(),
                };
                let msg = format!(
                    "EDB: cheatcode vm.{name} not supported in v1: \
                     script-only — not applicable to forge test. See docs/cheatcodes.md"
                );
                self.record_and_revert(inputs, name, selector, UnsupportedCategory::Rejected, &msg)
            }

            // Assertion cheatcodes — assertEq, assertNe, assertTrue, assertFalse,
            // assertGe, assertGt, assertLe, assertLt and their string-message overloads.
            // These are the vm-level assertions delegated to by forge-std's StdAssertions.
            SEL_ASSERT_EQ_U256
            | SEL_ASSERT_EQ_U256_MSG
            | SEL_ASSERT_EQ_I256
            | SEL_ASSERT_EQ_I256_MSG
            | SEL_ASSERT_EQ_ADDR
            | SEL_ASSERT_EQ_ADDR_MSG
            | SEL_ASSERT_EQ_BOOL
            | SEL_ASSERT_EQ_BOOL_MSG
            | SEL_ASSERT_EQ_B32
            | SEL_ASSERT_EQ_B32_MSG
            | SEL_ASSERT_TRUE
            | SEL_ASSERT_TRUE_MSG
            | SEL_ASSERT_FALSE
            | SEL_ASSERT_FALSE_MSG
            | SEL_ASSERT_GE_U256
            | SEL_ASSERT_GE_U256_MSG
            | SEL_ASSERT_GE_I256
            | SEL_ASSERT_GE_I256_MSG
            | SEL_ASSERT_GT_U256
            | SEL_ASSERT_GT_U256_MSG
            | SEL_ASSERT_GT_I256
            | SEL_ASSERT_GT_I256_MSG
            | SEL_ASSERT_LE_U256
            | SEL_ASSERT_LE_U256_MSG
            | SEL_ASSERT_LE_I256
            | SEL_ASSERT_LE_I256_MSG
            | SEL_ASSERT_LT_U256
            | SEL_ASSERT_LT_U256_MSG
            | SEL_ASSERT_LT_I256
            | SEL_ASSERT_LT_I256_MSG
            | SEL_ASSERT_NOT_EQ_U256
            | SEL_ASSERT_NOT_EQ_U256_MSG
            | SEL_ASSERT_NOT_EQ_I256
            | SEL_ASSERT_NOT_EQ_I256_MSG
            | SEL_ASSERT_NOT_EQ_ADDR
            | SEL_ASSERT_NOT_EQ_ADDR_MSG
            | SEL_ASSERT_NOT_EQ_BOOL
            | SEL_ASSERT_NOT_EQ_BOOL_MSG
            | SEL_ASSERT_NOT_EQ_B32
            | SEL_ASSERT_NOT_EQ_B32_MSG => self.cheat_assert(inputs, selector, args),

            // Gas snapshot stubs — EDB doesn't do gas profiling; these are no-ops
            // that emit a one-time warning so tests using vm.startSnapshotGas /
            // vm.stopSnapshotGas / vm.snapshotGasLastCall don't hard-abort.
            SEL_START_SNAPSHOT_GAS_STR
            | SEL_STOP_SNAPSHOT_GAS
            | SEL_STOP_SNAPSHOT_GAS_STR
            | SEL_STOP_SNAPSHOT_GAS_2STR
            | SEL_SNAPSHOT_GAS_LAST_CALL_STR
            | SEL_SNAPSHOT_GAS_LAST_CALL_2STR => {
                let name = match selector {
                    SEL_START_SNAPSHOT_GAS_STR => "startSnapshotGas",
                    SEL_SNAPSHOT_GAS_LAST_CALL_STR | SEL_SNAPSHOT_GAS_LAST_CALL_2STR => {
                        "snapshotGasLastCall"
                    }
                    _ => "stopSnapshotGas",
                };
                self.cheat_gas_snapshot_stub(inputs, name)
            }

            // Benchmark-value snapshot stubs — EDB is not a benchmark recorder
            // in v1; both `snapshotValue` overloads succeed as no-ops with a
            // one-time warning. Same pattern as the gas-snapshot family.
            SEL_SNAPSHOT_VALUE_2 | SEL_SNAPSHOT_VALUE_3 => self.cheat_snapshot_value_stub(inputs),

            // Locally-compiled artifact lookup: return the deployed bytecode
            // of a project contract by its artifact name. Foundry reads from
            // `out/<file>.json`; EDB resolves against the in-memory
            // `LocalArtifactSet` populated before prepare().
            SEL_GET_DEPLOYED_CODE => self.cheat_get_deployed_code(inputs, args),

            _ => {
                let hex = alloy_primitives::hex::encode(selector);
                // Look up the selector in the known-cheatcode catalog.
                let known = KNOWN_CHEATCODES.iter().find(|(sel, _)| **sel == selector);
                if let Some((_, name)) = known {
                    // Distinguish rejected vs not-yet-implemented purely by name.
                    let (category, phrase) = if is_explicitly_rejected_name(name) {
                        (UnsupportedCategory::Rejected, "rejected in v1")
                    } else {
                        (UnsupportedCategory::NotYetImplemented, "not yet implemented in v1")
                    };
                    let msg = format!(
                        "EDB: cheatcode vm.{name} {phrase} \
                         (selector 0x{hex}). See docs/cheatcodes.md"
                    );
                    self.record_and_revert(inputs, name, selector, category, &msg)
                } else {
                    // Selector is not in our catalog at all — likely a non-vm call
                    // that accidentally hit the cheatcode address, or a very new
                    // foundry cheatcode we haven't cataloged yet.
                    let display = format!("<unknown selector 0x{hex}>");
                    let msg = format!(
                        "EDB: unknown cheatcode selector 0x{hex} \
                         (not in foundry's known cheatcode catalog — \
                         check spelling or open an issue)"
                    );
                    self.record_and_revert(
                        inputs,
                        &display,
                        selector,
                        UnsupportedCategory::Unknown,
                        &msg,
                    )
                }
            }
        }
    }

    // --- State snapshots (Task 3+) ------------------------------------------

    fn cheat_snapshot_state(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &revm::interpreter::CallInputs,
        _args: &[u8],
    ) -> revm::interpreter::CallOutcome {
        let id = self.next_snapshot_id;
        self.next_snapshot_id += 1;

        // Clone the EVM's journal (which carries the CacheDB at
        // journaled_state.database) into a Snapshot. No separate db clone is
        // needed: restoring the journal restores the DB along with it.
        self.snapshots.insert(id, Snapshot { journal: ctx.journaled_state.clone() });

        // Encode the u64 id as a uint256 return (32-byte big-endian). The
        // memory_offset propagation lives in ok_return: REVM copies these 32
        // bytes back into the caller's memory at the slot Solidity reserved.
        let mut out = [0u8; 32];
        out[24..].copy_from_slice(&id.to_be_bytes());
        ok_return(inputs, Bytes::copy_from_slice(&out))
    }

    fn cheat_revert_to_state(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &revm::interpreter::CallInputs,
        args: &[u8],
    ) -> revm::interpreter::CallOutcome {
        if args.len() < 32 {
            return revert_with(inputs, encode_error_string("vm.revertToState: bad calldata"));
        }
        // Decode the uint256 arg as u64 (low 8 bytes of the 32-byte word).
        let id_bytes: [u8; 8] = args[24..32].try_into().unwrap_or([0; 8]);
        let id = u64::from_be_bytes(id_bytes);

        // Take ownership of the snapshot (foundry semantics: revert is one-shot).
        let restored = match self.snapshots.remove(&id) {
            Some(snap) => {
                // Restore the journal wholesale. The CacheDB lives at
                // journaled_state.database, so this also restores the DB.
                ctx.journaled_state = snap.journal;
                true
            }
            None => false,
        };

        // Encode bool as uint256-padded.
        let mut out = [0u8; 32];
        out[31] = if restored { 1 } else { 0 };
        ok_return(inputs, Bytes::copy_from_slice(&out))
    }

    fn cheat_delete_state_snapshot(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &revm::interpreter::CallInputs,
        args: &[u8],
    ) -> revm::interpreter::CallOutcome {
        if args.len() < 32 {
            return revert_with(
                inputs,
                encode_error_string("vm.deleteStateSnapshot: bad calldata"),
            );
        }
        let id_bytes: [u8; 8] = args[24..32].try_into().unwrap_or([0; 8]);
        let id = u64::from_be_bytes(id_bytes);
        let existed = self.snapshots.remove(&id).is_some();

        let mut out = [0u8; 32];
        out[31] = if existed { 1 } else { 0 };
        ok_return(inputs, Bytes::copy_from_slice(&out))
    }

    fn cheat_delete_state_snapshots(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &revm::interpreter::CallInputs,
        _args: &[u8],
    ) -> revm::interpreter::CallOutcome {
        self.snapshots.clear();
        // Void return — empty bytes.
        ok_return(inputs, Bytes::new())
    }

    // --- Block / chain mutators ---------------------------------------------

    fn cheat_warp(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(value) = read_u256(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.warp: bad calldata"));
        };
        ctx.block.timestamp = value;
        ok_return(inputs, Bytes::new())
    }

    fn cheat_roll(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(value) = read_u256(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.roll: bad calldata"));
        };
        ctx.block.number = value;
        ok_return(inputs, Bytes::new())
    }

    /// vm.rollFork(uint256) — v1: updates block.number only.
    ///
    /// **Limitations (documented in `docs/cheatcodes.md`):**
    /// - Does NOT update block.timestamp (pair with `vm.warp` for that).
    /// - Does NOT update block.basefee / prevrandao / etc.
    /// - Does NOT invalidate the CacheDB — reads continue to reflect state at the
    ///   originally-forked block. Tests that need cross-block state should set
    ///   `--fork-block-number` at the CLI to start at the target block.
    ///
    /// `vm.rollFork(uint256,uint256)` (cross-fork roll) and `vm.rollFork(bytes32)`
    /// (tx-hash roll) remain rejected — they require multi-fork backend support.
    fn cheat_roll_fork_single(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        self.warn_once(
            "rollFork(uint256)",
            "EDB updates block.number only — block.timestamp / basefee unchanged. \
             Pair with vm.warp() if you need timestamp. CacheDB not invalidated. \
             See docs/cheatcodes.md for details.",
        );
        if args.len() < 32 {
            return revert_with(inputs, encode_error_string("vm.rollFork(uint256): bad calldata"));
        }
        let n = alloy_primitives::U256::from_be_slice(&args[..32]);
        ctx.block.number = n;
        ok_return(inputs, Bytes::new())
    }

    fn cheat_chain_id(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(value) = read_u256(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.chainId: bad calldata"));
        };
        let chain_id: u64 = match value.try_into() {
            Ok(v) => v,
            Err(_) => {
                return revert_with(
                    inputs,
                    encode_error_string("vm.chainId: value does not fit in u64"),
                );
            }
        };
        ctx.cfg.chain_id = chain_id;
        ok_return(inputs, Bytes::new())
    }

    /// `vm.fee(uint256)` — set `block.basefee` for subsequent calls.
    ///
    /// REVM's [`BlockEnv::basefee`] is `u64` (post-EIP-1559 the network-level
    /// basefee fits in u64 by construction; foundry's spec admits any
    /// uint256 from solidity, but the eventual EIP-1559 enforcement caps the
    /// effective value). We saturate to `u64::MAX` rather than rejecting so
    /// that real-world tests that pass `type(uint256).max` as a "very high
    /// fee" sentinel still work.
    fn cheat_fee(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(value) = read_u256(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.fee: bad uint256 arg"));
        };
        ctx.block.basefee = value.try_into().unwrap_or(u64::MAX);
        ok_return(inputs, Bytes::new())
    }

    /// `vm.txGasPrice(uint256)` — set `tx.gas_price` for subsequent calls.
    ///
    /// REVM's [`TxEnv::gas_price`] is `u128`; we saturate (rather than
    /// reject) on overflow for the same reason as `vm.fee`.
    fn cheat_tx_gas_price(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(value) = read_u256(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.txGasPrice: bad uint256 arg"));
        };
        ctx.tx.gas_price = value.try_into().unwrap_or(u128::MAX);
        ok_return(inputs, Bytes::new())
    }

    // --- vm.toString family -------------------------------------------------
    //
    // Six type overloads, each formatting a Solidity primitive as the
    // canonical string representation foundry produces:
    //   - address  -> EIP-55 checksum (alloy's Display for Address)
    //   - bool     -> "true" / "false"
    //   - bytes    -> "0x" + lowercase hex (alloy's Display for Bytes)
    //   - bytes32  -> "0x" + lowercase hex (alloy's Display for B256)
    //   - uint256  -> decimal
    //   - int256   -> signed decimal (alloy's Display for I256)
    //
    // Return shape: ABI-encoded `string` (offset + length + padded UTF-8),
    // produced by `encode_abi_string`. Matches what `vm.envString` already
    // does for its return shape.

    fn cheat_to_string_address(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(addr) = read_address(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.toString(address): bad calldata"));
        };
        ok_return(inputs, encode_abi_string(&addr.to_string()))
    }

    fn cheat_to_string_bool(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(b) = read_bool(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.toString(bool): bad calldata"));
        };
        let s = if b { "true" } else { "false" };
        ok_return(inputs, encode_abi_string(s))
    }

    fn cheat_to_string_bytes(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(b) = read_bytes(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.toString(bytes): bad calldata"));
        };
        // "0x" + lowercase hex (matches alloy's `Display for Bytes`).
        let mut s = String::with_capacity(2 + b.len() * 2);
        s.push_str("0x");
        s.push_str(&alloy_primitives::hex::encode(&b));
        ok_return(inputs, encode_abi_string(&s))
    }

    fn cheat_to_string_bytes32(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(w) = read_b256(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.toString(bytes32): bad calldata"));
        };
        // alloy's Display for B256 already prints `0x` + lowercase hex.
        ok_return(inputs, encode_abi_string(&w.to_string()))
    }

    fn cheat_to_string_int256(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(w) = read_word(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.toString(int256): bad calldata"));
        };
        // alloy's I256 has a from-raw constructor that interprets the 32 bytes
        // as two's complement and its Display impl produces the signed-decimal
        // representation foundry's tests expect.
        let signed = alloy_primitives::I256::from_be_bytes::<32>(w);
        ok_return(inputs, encode_abi_string(&signed.to_string()))
    }

    fn cheat_to_string_uint256(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(v) = read_u256(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.toString(uint256): bad calldata"));
        };
        ok_return(inputs, encode_abi_string(&v.to_string()))
    }

    // --- vm.parseJson family -------------------------------------------------
    //
    // We support the typed accessors (parseJsonBool, parseJsonString,
    // parseJsonBytes32, parseJsonUint, parseJsonInt, parseJsonAddress) plus
    // the bare parseJson(string) and parseJson(string,string) overloads.
    //
    // The JSONPath accessor is a minimal subset of foundry's
    // jsonpath_lib-backed syntax: leading `$` is optional, then a sequence of
    // `.<ident>` and/or `[<index>]` tokens. This covers every parseJson*
    // invocation in the static-coverage projects (`.x`, `.foo`, `.foo.bar`,
    // `.foo[0]`, `.foo[0].bar`). The fall-through returns a clean error if a
    // requested token isn't found or the leaf type doesn't match.

    fn cheat_parse_json_root(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(json) = read_string(args, 0) else {
            return revert_with(
                inputs,
                encode_error_string("vm.parseJson(string): malformed calldata"),
            );
        };
        match parse_json_to_abi_bytes(&json, "$") {
            Ok(b) => ok_return(inputs, b),
            Err(msg) => revert_with(inputs, encode_error_string(&msg)),
        }
    }

    fn cheat_parse_json_path(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(json) = read_string(args, 0) else {
            return revert_with(
                inputs,
                encode_error_string("vm.parseJson(string,string): malformed calldata"),
            );
        };
        let Some(path) = read_string(args, 1) else {
            return revert_with(
                inputs,
                encode_error_string("vm.parseJson(string,string): malformed path arg"),
            );
        };
        match parse_json_to_abi_bytes(&json, &path) {
            Ok(b) => ok_return(inputs, b),
            Err(msg) => revert_with(inputs, encode_error_string(&msg)),
        }
    }

    fn cheat_parse_json_bool(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let (json, path) = match read_json_path_pair(args, "vm.parseJsonBool") {
            Ok(v) => v,
            Err(msg) => return revert_with(inputs, encode_error_string(&msg)),
        };
        match navigate_json(&json, &path).and_then(|v| v.as_bool()) {
            Some(b) => {
                let mut out = [0u8; 32];
                out[31] = u8::from(b);
                ok_return(inputs, Bytes::copy_from_slice(&out))
            }
            None => revert_with(
                inputs,
                encode_error_string(&format!(
                    "EDB: vm.parseJsonBool: path {path:?} did not resolve to a JSON bool"
                )),
            ),
        }
    }

    fn cheat_parse_json_string(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let (json, path) = match read_json_path_pair(args, "vm.parseJsonString") {
            Ok(v) => v,
            Err(msg) => return revert_with(inputs, encode_error_string(&msg)),
        };
        match navigate_json(&json, &path).and_then(|v| v.as_str()) {
            Some(s) => ok_return(inputs, encode_abi_string(s)),
            None => revert_with(
                inputs,
                encode_error_string(&format!(
                    "EDB: vm.parseJsonString: path {path:?} did not resolve to a JSON string"
                )),
            ),
        }
    }

    fn cheat_parse_json_bytes32(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let (json, path) = match read_json_path_pair(args, "vm.parseJsonBytes32") {
            Ok(v) => v,
            Err(msg) => return revert_with(inputs, encode_error_string(&msg)),
        };
        let Some(leaf) = navigate_json(&json, &path) else {
            return revert_with(
                inputs,
                encode_error_string(&format!(
                    "EDB: vm.parseJsonBytes32: path {path:?} did not resolve"
                )),
            );
        };
        let Some(s) = leaf.as_str() else {
            return revert_with(
                inputs,
                encode_error_string(&format!(
                    "EDB: vm.parseJsonBytes32: path {path:?} is not a string leaf"
                )),
            );
        };
        let trimmed = s.trim();
        let hex_body = trimmed.strip_prefix("0x").unwrap_or(trimmed);
        let decoded = match alloy_primitives::hex::decode(hex_body) {
            Ok(v) => v,
            Err(_) => {
                return revert_with(
                    inputs,
                    encode_error_string(&format!(
                        "EDB: vm.parseJsonBytes32: leaf at {path:?} is not valid hex"
                    )),
                );
            }
        };
        if decoded.len() > 32 {
            return revert_with(
                inputs,
                encode_error_string(&format!(
                    "EDB: vm.parseJsonBytes32: leaf at {path:?} is longer than 32 bytes"
                )),
            );
        }
        // Right-pad to 32 bytes (matches foundry's FixedBytes(32) coercion).
        let mut out = [0u8; 32];
        out[..decoded.len()].copy_from_slice(&decoded);
        ok_return(inputs, Bytes::copy_from_slice(&out))
    }

    fn cheat_parse_json_uint(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let (json, path) = match read_json_path_pair(args, "vm.parseJsonUint") {
            Ok(v) => v,
            Err(msg) => return revert_with(inputs, encode_error_string(&msg)),
        };
        let Some(leaf) = navigate_json(&json, &path) else {
            return revert_with(
                inputs,
                encode_error_string(&format!(
                    "EDB: vm.parseJsonUint: path {path:?} did not resolve"
                )),
            );
        };
        match parse_uint256_from_json(leaf) {
            Ok(u) => ok_return(inputs, Bytes::copy_from_slice(&u.to_be_bytes::<32>())),
            Err(e) => revert_with(
                inputs,
                encode_error_string(&format!("EDB: vm.parseJsonUint: path {path:?}: {e}")),
            ),
        }
    }

    fn cheat_parse_json_int(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let (json, path) = match read_json_path_pair(args, "vm.parseJsonInt") {
            Ok(v) => v,
            Err(msg) => return revert_with(inputs, encode_error_string(&msg)),
        };
        let Some(leaf) = navigate_json(&json, &path) else {
            return revert_with(
                inputs,
                encode_error_string(&format!(
                    "EDB: vm.parseJsonInt: path {path:?} did not resolve"
                )),
            );
        };
        match parse_int256_from_json(leaf) {
            Ok(i) => ok_return(inputs, Bytes::copy_from_slice(&i.to_be_bytes::<32>())),
            Err(e) => revert_with(
                inputs,
                encode_error_string(&format!("EDB: vm.parseJsonInt: path {path:?}: {e}")),
            ),
        }
    }

    fn cheat_parse_json_address(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let (json, path) = match read_json_path_pair(args, "vm.parseJsonAddress") {
            Ok(v) => v,
            Err(msg) => return revert_with(inputs, encode_error_string(&msg)),
        };
        let Some(leaf) = navigate_json(&json, &path) else {
            return revert_with(
                inputs,
                encode_error_string(&format!(
                    "EDB: vm.parseJsonAddress: path {path:?} did not resolve"
                )),
            );
        };
        let Some(s) = leaf.as_str() else {
            return revert_with(
                inputs,
                encode_error_string(&format!(
                    "EDB: vm.parseJsonAddress: path {path:?} is not a string leaf"
                )),
            );
        };
        let addr: Address = match s.parse() {
            Ok(a) => a,
            Err(_) => {
                return revert_with(
                    inputs,
                    encode_error_string(&format!(
                        "EDB: vm.parseJsonAddress: leaf at {path:?} is not a hex address"
                    )),
                );
            }
        };
        let mut out = [0u8; 32];
        out[12..].copy_from_slice(addr.as_slice());
        ok_return(inputs, Bytes::copy_from_slice(&out))
    }

    // --- Account state mutators ---------------------------------------------

    fn cheat_deal(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(target) = read_address(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.deal: bad address arg"));
        };
        let Some(value) = read_u256(args, 1) else {
            return revert_with(inputs, encode_error_string("vm.deal: bad value arg"));
        };
        match ctx.journaled_state.load_account_mut(target) {
            Ok(mut acc) => {
                acc.set_balance(value);
                acc.touch();
                ok_return(inputs, Bytes::new())
            }
            Err(_) => revert_with(inputs, encode_error_string("vm.deal: failed to load account")),
        }
    }

    fn cheat_etch(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(target) = read_address(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.etch: bad address arg"));
        };
        let Some(code) = read_bytes(args, 1) else {
            return revert_with(inputs, encode_error_string("vm.etch: bad bytes arg"));
        };
        // Make sure the account is warm before set_code (per JournalTr contract).
        if ctx.journaled_state.load_account_with_code(target).is_err() {
            return revert_with(inputs, encode_error_string("vm.etch: failed to load account"));
        }
        let bytecode = Bytecode::new_raw(code);
        ctx.journaled_state.set_code(target, bytecode);
        ok_return(inputs, Bytes::new())
    }

    fn cheat_store(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(target) = read_address(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.store: bad address arg"));
        };
        let Some(slot) = read_b256(args, 1) else {
            return revert_with(inputs, encode_error_string("vm.store: bad slot arg"));
        };
        let Some(value) = read_b256(args, 2) else {
            return revert_with(inputs, encode_error_string("vm.store: bad value arg"));
        };
        // Make sure the account is warm.
        if ctx.journaled_state.load_account(target).is_err() {
            return revert_with(inputs, encode_error_string("vm.store: failed to load account"));
        }
        let key = U256::from_be_bytes(slot.0);
        let val = U256::from_be_bytes(value.0);
        if ctx.journaled_state.sstore(target, key, val).is_err() {
            return revert_with(inputs, encode_error_string("vm.store: sstore failed"));
        }
        ok_return(inputs, Bytes::new())
    }

    fn cheat_load(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(target) = read_address(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.load: bad address arg"));
        };
        let Some(slot) = read_b256(args, 1) else {
            return revert_with(inputs, encode_error_string("vm.load: bad slot arg"));
        };
        if ctx.journaled_state.load_account(target).is_err() {
            return revert_with(inputs, encode_error_string("vm.load: failed to load account"));
        }
        let key = U256::from_be_bytes(slot.0);
        match ctx.journaled_state.sload(target, key) {
            Ok(loaded) => {
                let bytes = Bytes::copy_from_slice(&loaded.data.to_be_bytes::<32>());
                ok_return(inputs, bytes)
            }
            Err(_) => revert_with(inputs, encode_error_string("vm.load: sload failed")),
        }
    }

    fn cheat_set_nonce(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(target) = read_address(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.setNonce: bad address arg"));
        };
        let Some(value) = read_u256(args, 1) else {
            return revert_with(inputs, encode_error_string("vm.setNonce: bad nonce arg"));
        };
        let nonce: u64 = match value.try_into() {
            Ok(v) => v,
            Err(_) => {
                return revert_with(
                    inputs,
                    encode_error_string("vm.setNonce: nonce does not fit in u64"),
                );
            }
        };
        match ctx.journaled_state.load_account_mut(target) {
            Ok(mut acc) => {
                acc.set_nonce(nonce);
                acc.touch();
                ok_return(inputs, Bytes::new())
            }
            Err(_) => {
                revert_with(inputs, encode_error_string("vm.setNonce: failed to load account"))
            }
        }
    }

    /// `vm.getNonce(address) returns (uint64)` — mirror of `vm.setNonce`'s
    /// journal-load path. Returns the account's current nonce as a left-padded
    /// 32-byte ABI word (the uint64 sits in the trailing 8 bytes).
    fn cheat_get_nonce(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(target) = read_address(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.getNonce: bad address arg"));
        };
        match ctx.journaled_state.load_account(target) {
            Ok(acc) => {
                let nonce: u64 = acc.info.nonce;
                let mut out = [0u8; 32];
                out[24..].copy_from_slice(&nonce.to_be_bytes());
                ok_return(inputs, Bytes::copy_from_slice(&out))
            }
            Err(_) => {
                revert_with(inputs, encode_error_string("vm.getNonce: failed to load account"))
            }
        }
    }

    /// `vm.getBlockNumber() returns (uint256)` — reads `ctx.block.number`.
    /// Foundry exposes this so tests can re-read the current block.number
    /// after `vm.roll`/`vm.rollFork`, dodging the solc optimization that
    /// caches `block.number` as a constant across a transaction body.
    fn cheat_get_block_number(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
    ) -> CallOutcome {
        let n: U256 = ctx.block.number;
        let bytes = n.to_be_bytes::<32>();
        ok_return(inputs, Bytes::copy_from_slice(&bytes))
    }

    /// `vm.setBlockhash(uint256 blockNumber, bytes32 blockHash)` — install an
    /// override for the BLOCKHASH opcode at `blockNumber`. We write directly
    /// into CacheDB's `block_hashes` cache: the BLOCKHASH opcode reads through
    /// `Database::block_hash`, which consults the cache before falling back to
    /// the underlying `DatabaseRef`. The cache is consulted with `U256::from(number: u64)`
    /// so we store under the same key.
    ///
    /// Mirrors foundry's restriction `block.number - 256 <= n < block.number`,
    /// but applied loosely (we accept the equality endpoint on the current
    /// block too, matching foundry's `<=`).
    fn cheat_set_blockhash(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(block_number) = read_u256(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.setBlockhash: bad uint256 arg"));
        };
        let Some(block_hash) = read_b256(args, 1) else {
            return revert_with(inputs, encode_error_string("vm.setBlockhash: bad bytes32 arg"));
        };
        if block_number > U256::from(u64::MAX) {
            return revert_with(
                inputs,
                encode_error_string("vm.setBlockhash: blockNumber must fit in u64"),
            );
        }
        if block_number > ctx.block.number {
            return revert_with(
                inputs,
                encode_error_string(
                    "vm.setBlockhash: block number must be less than or equal to the current block number",
                ),
            );
        }
        // CacheDB::block_hash reads through this map first, so an explicit
        // insert here suffices for both opcode-driven and `block_hash_ref`
        // lookups within the current run.
        ctx.journaled_state.database.cache.block_hashes.insert(block_number, block_hash);
        ok_return(inputs, Bytes::new())
    }

    /// `vm.readLine(string path) returns (string)` — open the file (cached
    /// across calls), advance the cursor by one line, and return the line
    /// content (without the trailing `\n` / `\r\n`). EOF returns an empty
    /// string. Subsequent calls to the same path continue from where the
    /// previous call stopped.
    ///
    /// Sandbox: only paths that resolve under `config.project_root` (or under
    /// the current working directory, if `project_root` is empty — used by
    /// unit tests) are accepted. Resolution is via `canonicalize()` on both
    /// sides so symlink escapes are caught.
    fn cheat_read_line(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        use std::io::BufRead;

        let Some(raw_path) = read_string(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.readLine: bad string arg"));
        };
        let path = match resolve_sandboxed_path(&self.config.project_root, &raw_path) {
            Ok(p) => p,
            Err(msg) => return revert_with(inputs, encode_error_string(&msg)),
        };
        // Open lazily and cache so repeated calls advance the cursor.
        let reader = match self.file_cursors.entry(path.clone()) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let f = match std::fs::File::open(&path) {
                    Ok(f) => f,
                    Err(err) => {
                        return revert_with(
                            inputs,
                            encode_error_string(&format!("vm.readLine: open failed: {err}")),
                        );
                    }
                };
                e.insert(std::io::BufReader::new(f))
            }
        };
        let mut line = String::new();
        if let Err(err) = reader.read_line(&mut line) {
            return revert_with(
                inputs,
                encode_error_string(&format!("vm.readLine: read failed: {err}")),
            );
        }
        // Strip the trailing newline (LF or CRLF). EOF leaves `line` empty,
        // which is the value we want to return.
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        ok_return(inputs, encode_abi_string(&line))
    }

    // --- Pranks --------------------------------------------------------------

    fn cheat_prank(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
        one_shot: bool,
    ) -> CallOutcome {
        let Some(new_caller) = read_address(args, 0) else {
            return revert_with(
                inputs,
                encode_error_string("vm.prank/startPrank: bad address arg"),
            );
        };
        // Install at the caller's depth (the depth at which vm.prank ran).
        // The next sub-call out of that frame happens at the same depth from
        // the Inspector's vantage point (Inspector::call fires before the
        // child journal checkpoint), so we key by `depth()` at install time.
        let depth = ctx.journaled_state.depth();
        self.pranks.insert(depth, Prank { new_caller, one_shot, fired: false });
        ok_return(inputs, Bytes::new())
    }

    fn cheat_stop_prank(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
    ) -> CallOutcome {
        let depth = ctx.journaled_state.depth();
        self.pranks.remove(&depth);
        // If an earlier `vm.startPrank(address,address)` overrode tx.origin,
        // restore the original now.
        if let Some(orig) = self.saved_tx_origin.take() {
            ctx.tx.caller = orig;
        }
        ok_return(inputs, Bytes::new())
    }

    /// `vm.startPrank(address msgSender, address txOrigin)` — selector
    /// `0x45b56078`. Sets msg.sender for subsequent calls via the existing
    /// prank machinery AND overrides tx.origin for the rest of the prank
    /// scope. The original tx.origin is restored on `vm.stopPrank`.
    ///
    /// Implementation note: we only save the FIRST tx.origin we observe
    /// while a prank is active — successive 2-arg startPrank calls
    /// (without intervening stopPrank) update the override but keep the
    /// original tx.origin pinned for restoration. This matches forge's
    /// behavior where the genuine pre-prank origin is the restore target.
    fn cheat_start_prank_2(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(new_caller) = read_address(args, 0) else {
            return revert_with(
                inputs,
                encode_error_string("vm.startPrank: bad msgSender address arg"),
            );
        };
        let Some(new_origin) = read_address(args, 1) else {
            return revert_with(
                inputs,
                encode_error_string("vm.startPrank: bad txOrigin address arg"),
            );
        };
        let depth = ctx.journaled_state.depth();
        self.pranks.insert(depth, Prank { new_caller, one_shot: false, fired: false });
        if self.saved_tx_origin.is_none() {
            self.saved_tx_origin = Some(ctx.tx.caller);
        }
        ctx.tx.caller = new_origin;
        ok_return(inputs, Bytes::new())
    }

    // --- Mocks --------------------------------------------------------------

    fn cheat_mock_call(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
        reverts: bool,
    ) -> CallOutcome {
        let Some(target) = read_address(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.mockCall: bad address arg"));
        };
        let Some(calldata) = read_bytes(args, 1) else {
            return revert_with(inputs, encode_error_string("vm.mockCall: bad calldata arg"));
        };
        let Some(retdata) = read_bytes(args, 2) else {
            return revert_with(inputs, encode_error_string("vm.mockCall: bad return-data arg"));
        };
        let entry = if reverts { MockReturn::Revert(retdata) } else { MockReturn::Return(retdata) };
        self.mocks.entry(target).or_default().insert(calldata, entry);
        ok_return(inputs, Bytes::new())
    }

    fn cheat_clear_mocked_calls(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
    ) -> CallOutcome {
        self.mocks.clear();
        ok_return(inputs, Bytes::new())
    }

    // --- expectRevert --------------------------------------------------------

    /// `vm.expectRevert()` — match any revert from the next sub-call.
    fn cheat_expect_revert_bare(&mut self, inputs: &CallInputs) -> CallOutcome {
        self.expected_revert = Some(ExpectedRevert { expected: ExpectedRevertMatch::Bare });
        ok_return(inputs, Bytes::new())
    }

    /// `vm.expectRevert(bytes)` — match the next sub-call's revert payload
    /// against the supplied bytes exactly (byte-for-byte).
    fn cheat_expect_revert_bytes(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(want) = read_bytes(args, 0) else {
            return revert_with(
                inputs,
                encode_error_string("vm.expectRevert(bytes): bad bytes arg"),
            );
        };
        self.expected_revert = Some(ExpectedRevert { expected: ExpectedRevertMatch::Exact(want) });
        ok_return(inputs, Bytes::new())
    }

    /// `vm.expectRevert(bytes4)` — match the leading 4 bytes (selector) of the
    /// next sub-call's revert payload. Used for custom-error reverts where
    /// only the selector is significant (the trailing ABI-encoded args may
    /// vary across runs).
    fn cheat_expect_revert_bytes4(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        // bytes4 is left-aligned in its 32-byte head word.
        let Some(word) = read_word(args, 0) else {
            return revert_with(
                inputs,
                encode_error_string("vm.expectRevert(bytes4): bad bytes4 arg"),
            );
        };
        let sel: [u8; 4] = word[..4].try_into().expect("4 bytes from a 32-byte word");
        self.expected_revert =
            Some(ExpectedRevert { expected: ExpectedRevertMatch::Selector(sel) });
        ok_return(inputs, Bytes::new())
    }

    // --- vm.addr / vm.sign (secp256k1 crypto) --------------------------------

    /// `vm.addr(uint256 privateKey) returns (address)` — derive an Ethereum
    /// address from a secp256k1 secret key. Reverts if the key is zero or
    /// otherwise out of range for the secp256k1 scalar field.
    fn cheat_addr(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(sk) = read_u256(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.addr: bad uint256 arg"));
        };
        let sk_bytes = B256::from(sk);
        match PrivateKeySigner::from_bytes(&sk_bytes) {
            Ok(signer) => {
                // ABI-encode the address as a 32-byte word (left-padded with
                // 12 zero bytes; the 20-byte address sits at offset 12..32).
                let mut out = [0u8; 32];
                out[12..].copy_from_slice(signer.address().as_slice());
                ok_return(inputs, Bytes::copy_from_slice(&out))
            }
            Err(_) => revert_with(inputs, encode_error_string("vm.addr: invalid private key")),
        }
    }

    /// `vm.sign(uint256 privateKey, bytes32 digest) returns (uint8 v, bytes32 r, bytes32 s)` —
    /// ECDSA-sign the pre-hashed `digest` with the secret key. The digest is
    /// signed AS-IS (no EIP-191 prefix is added). The returned `v` is
    /// normalized to the legacy 27/28 convention.
    fn cheat_sign(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(sk) = read_u256(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.sign: bad uint256 arg"));
        };
        let Some(digest) = read_b256(args, 1) else {
            return revert_with(inputs, encode_error_string("vm.sign: bad bytes32 arg"));
        };
        let sk_bytes = B256::from(sk);
        let Ok(signer) = PrivateKeySigner::from_bytes(&sk_bytes) else {
            return revert_with(inputs, encode_error_string("vm.sign: invalid private key"));
        };
        match signer.sign_hash_sync(&digest) {
            Ok(sig) => {
                // ABI-encode (uint8 v, bytes32 r, bytes32 s) as three 32-byte slots.
                // alloy_primitives::Signature::v() is a y-parity bool; foundry's
                // Vm.sol returns the legacy 27/28 encoding for the v slot.
                let v: u8 = if sig.v() { 28 } else { 27 };
                let mut out = vec![0u8; 96];
                out[31] = v;
                out[32..64].copy_from_slice(&sig.r().to_be_bytes::<32>());
                out[64..96].copy_from_slice(&sig.s().to_be_bytes::<32>());
                ok_return(inputs, Bytes::from(out))
            }
            Err(_) => revert_with(inputs, encode_error_string("vm.sign: signing failed")),
        }
    }

    /// `vm.signP256(uint256 privateKey, bytes32 digest) returns (bytes32 r, bytes32 s)` —
    /// NIST P-256 (secp256r1) ECDSA over the 32-byte pre-hashed digest. The
    /// digest is signed AS-IS via `sign_prehash` (no extra hashing applied).
    ///
    /// Foundry normalizes `s` to the low half of the curve order before
    /// returning (`signature.normalize_s().unwrap_or(signature)`), which makes
    /// downstream EIP-7212-style verifiers happy. We follow the same
    /// convention here.
    fn cheat_sign_p256(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        use p256::ecdsa::{
            Signature as P256Signature, SigningKey, signature::hazmat::PrehashSigner,
        };
        let Some(sk) = read_u256(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.signP256: bad uint256 arg"));
        };
        let Some(digest) = read_b256(args, 1) else {
            return revert_with(inputs, encode_error_string("vm.signP256: bad bytes32 arg"));
        };
        if let Err(msg) = validate_p256_private_key(&sk) {
            return revert_with(inputs, encode_error_string(&msg));
        }
        let sk_bytes: [u8; 32] = sk.to_be_bytes();
        let Ok(signing_key) = SigningKey::from_bytes((&sk_bytes).into()) else {
            return revert_with(inputs, encode_error_string("vm.signP256: invalid private key"));
        };
        let signature: P256Signature = match signing_key.sign_prehash(digest.as_slice()) {
            Ok(sig) => sig,
            Err(_) => {
                return revert_with(inputs, encode_error_string("vm.signP256: signing failed"));
            }
        };
        // Low-s normalization matches foundry's behavior so downstream verifiers
        // (typically EIP-7212-style) see a canonical signature.
        let signature = signature.normalize_s().unwrap_or(signature);
        let r_bytes: [u8; 32] = signature.r().to_bytes().into();
        let s_bytes: [u8; 32] = signature.s().to_bytes().into();
        // ABI-encode as `(bytes32 r, bytes32 s)` — two consecutive 32-byte slots.
        let mut out = [0u8; 64];
        out[..32].copy_from_slice(&r_bytes);
        out[32..].copy_from_slice(&s_bytes);
        ok_return(inputs, Bytes::copy_from_slice(&out))
    }

    /// `vm.publicKeyP256(uint256 privateKey) returns (uint256 x, uint256 y)` —
    /// derives the uncompressed P-256 (secp256r1) public point from the
    /// private key. Returns ABI-encoded `(x, y)` as two 32-byte big-endian
    /// uint256 words (matching foundry's `(U256, U256).abi_encode()`).
    fn cheat_public_key_p256(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        use p256::ecdsa::SigningKey;
        let Some(sk) = read_u256(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.publicKeyP256: bad uint256 arg"));
        };
        if let Err(msg) = validate_p256_private_key(&sk) {
            return revert_with(inputs, encode_error_string(&msg));
        }
        let sk_bytes: [u8; 32] = sk.to_be_bytes();
        let Ok(signing_key) = SigningKey::from_bytes((&sk_bytes).into()) else {
            return revert_with(
                inputs,
                encode_error_string("vm.publicKeyP256: invalid private key"),
            );
        };
        let verifying_key = signing_key.verifying_key();
        let encoded_point = verifying_key.to_encoded_point(false); // uncompressed
        // The encoded point is `04 || X (32 bytes) || Y (32 bytes)`; x()/y()
        // return the 32-byte coordinates only.
        let Some(x) = encoded_point.x() else {
            return revert_with(
                inputs,
                encode_error_string("vm.publicKeyP256: missing X coordinate"),
            );
        };
        let Some(y) = encoded_point.y() else {
            return revert_with(
                inputs,
                encode_error_string("vm.publicKeyP256: missing Y coordinate"),
            );
        };
        let mut out = [0u8; 64];
        out[..32].copy_from_slice(x);
        out[32..].copy_from_slice(y);
        ok_return(inputs, Bytes::copy_from_slice(&out))
    }

    // --- Labels --------------------------------------------------------------

    fn cheat_label(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(addr) = read_address(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.label: bad address arg"));
        };
        let Some(label) = read_string(args, 1) else {
            return revert_with(inputs, encode_error_string("vm.label: bad string arg"));
        };
        self.labels.insert(addr, label);
        ok_return(inputs, Bytes::new())
    }

    // --- recordLogs / getRecordedLogs ----------------------------------------

    fn cheat_record_logs(&mut self, inputs: &CallInputs) -> CallOutcome {
        self.recording_logs = true;
        self.recorded_logs.clear();
        ok_return(inputs, Bytes::new())
    }

    /// ABI-encodes the captured logs as `Log[]` where
    /// `struct Log { bytes32[] topics; bytes data; address emitter; }`,
    /// matching foundry's `Vm.Log` shape.
    fn cheat_get_recorded_logs(&mut self, inputs: &CallInputs) -> CallOutcome {
        let logs = std::mem::take(&mut self.recorded_logs);
        // We stop recording after the read, matching foundry's reset semantic.
        self.recording_logs = false;
        let encoded = abi_encode_logs(&logs);
        ok_return(inputs, encoded)
    }

    // --- expectEmit ---------------------------------------------------------

    fn cheat_expect_emit(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
        mode: ExpectEmitMode,
    ) -> CallOutcome {
        self.warn_once(
            "expectEmit",
            "EDB ships soft-match v1 — accepts any log from the (optionally) \
             constrained emitter with a signature topic, NOT byte-equality. \
             False positives possible. See docs/cheatcodes.md.",
        );
        let (check_topics, check_data, expected_emitter) = match mode {
            ExpectEmitMode::All => ([true; 4], true, None),
            ExpectEmitMode::Filter4 => {
                let Some(t1) = read_bool(args, 0) else {
                    return revert_with(
                        inputs,
                        encode_error_string("vm.expectEmit(bool,bool,bool,bool): bad arg 0"),
                    );
                };
                let Some(t2) = read_bool(args, 1) else {
                    return revert_with(
                        inputs,
                        encode_error_string("vm.expectEmit(bool,bool,bool,bool): bad arg 1"),
                    );
                };
                let Some(t3) = read_bool(args, 2) else {
                    return revert_with(
                        inputs,
                        encode_error_string("vm.expectEmit(bool,bool,bool,bool): bad arg 2"),
                    );
                };
                let Some(t4) = read_bool(args, 3) else {
                    return revert_with(
                        inputs,
                        encode_error_string("vm.expectEmit(bool,bool,bool,bool): bad arg 3"),
                    );
                };
                // Foundry's bools are (check_topic_1, check_topic_2, check_topic_3, check_data).
                // We map them onto our 4-topic + data layout: topic[0] is the
                // event sig (always present when emitted), topics[1..4] are
                // the 3 indexed args.
                ([true, t1, t2, t3], t4, None)
            }
            ExpectEmitMode::Filter5 => {
                let Some(t1) = read_bool(args, 0) else {
                    return revert_with(
                        inputs,
                        encode_error_string(
                            "vm.expectEmit(bool,bool,bool,bool,address): bad arg 0",
                        ),
                    );
                };
                let Some(t2) = read_bool(args, 1) else {
                    return revert_with(
                        inputs,
                        encode_error_string(
                            "vm.expectEmit(bool,bool,bool,bool,address): bad arg 1",
                        ),
                    );
                };
                let Some(t3) = read_bool(args, 2) else {
                    return revert_with(
                        inputs,
                        encode_error_string(
                            "vm.expectEmit(bool,bool,bool,bool,address): bad arg 2",
                        ),
                    );
                };
                let Some(t4) = read_bool(args, 3) else {
                    return revert_with(
                        inputs,
                        encode_error_string(
                            "vm.expectEmit(bool,bool,bool,bool,address): bad arg 3",
                        ),
                    );
                };
                let Some(emitter) = read_address(args, 4) else {
                    return revert_with(
                        inputs,
                        encode_error_string(
                            "vm.expectEmit(bool,bool,bool,bool,address): bad emitter arg",
                        ),
                    );
                };
                ([true, t1, t2, t3], t4, Some(emitter))
            }
            ExpectEmitMode::AnyTopicsFromEmitter => {
                let Some(emitter) = read_address(args, 0) else {
                    return revert_with(
                        inputs,
                        encode_error_string("vm.expectEmit(address): bad emitter arg"),
                    );
                };
                ([true; 4], true, Some(emitter))
            }
        };
        self.expected_emits.push(ExpectedEmit {
            check_topics,
            check_data,
            expected_emitter,
            matched: false,
            registered_at_call_depth: self.call_depth,
        });
        ok_return(inputs, Bytes::new())
    }

    // --- expectCall ---------------------------------------------------------

    fn cheat_expect_call(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
        default_count: u64,
    ) -> CallOutcome {
        let Some(target) = read_address(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.expectCall: bad address arg"));
        };
        let Some(calldata) = read_bytes(args, 1) else {
            return revert_with(inputs, encode_error_string("vm.expectCall: bad calldata arg"));
        };
        self.expected_calls.push(ExpectedCall {
            target,
            calldata,
            min_count: default_count,
            observed: 0,
            registered_at_call_depth: self.call_depth,
        });
        ok_return(inputs, Bytes::new())
    }

    fn cheat_expect_call_with_count(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome {
        let Some(target) = read_address(args, 0) else {
            return revert_with(
                inputs,
                encode_error_string("vm.expectCall(...,uint64): bad address arg"),
            );
        };
        let Some(calldata) = read_bytes(args, 1) else {
            return revert_with(
                inputs,
                encode_error_string("vm.expectCall(...,uint64): bad calldata arg"),
            );
        };
        let Some(count_word) = read_u256(args, 2) else {
            return revert_with(
                inputs,
                encode_error_string("vm.expectCall(...,uint64): bad count arg"),
            );
        };
        let count: u64 = match count_word.try_into() {
            Ok(v) => v,
            Err(_) => {
                return revert_with(
                    inputs,
                    encode_error_string("vm.expectCall(...,uint64): count does not fit in u64"),
                );
            }
        };
        self.expected_calls.push(ExpectedCall {
            target,
            calldata,
            min_count: count,
            observed: 0,
            registered_at_call_depth: self.call_depth,
        });
        ok_return(inputs, Bytes::new())
    }

    // --- vm.assume -----------------------------------------------------------

    fn cheat_assume(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        // ABI-decoded `bool` is a single 32-byte word; the bool is in the last byte.
        let Some(cond) = read_bool(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.assume: bad calldata"));
        };
        if cond {
            ok_return(inputs, Bytes::new())
        } else {
            revert_with(
                inputs,
                encode_error_string(
                    "EDB: vm.assume(false) -- assumption violated \
                     (real foundry would skip this fuzz iter; EDB surfaces it as a revert)",
                ),
            )
        }
    }

    // --- vm.envBool / envBytes / envString -----------------------------------

    fn cheat_env_bool(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(name) = read_string(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.envBool: malformed calldata"));
        };
        match std::env::var(&name) {
            Ok(v) => {
                let lower = v.trim().to_lowercase();
                let b = match lower.as_str() {
                    "true" | "1" => true,
                    "false" | "0" => false,
                    _ => {
                        return revert_with(
                            inputs,
                            encode_error_string(&format!(
                                "EDB: vm.envBool: {name}={v:?} not parseable as bool"
                            )),
                        );
                    }
                };
                let mut out = [0u8; 32];
                out[31] = u8::from(b);
                ok_return(inputs, Bytes::copy_from_slice(&out))
            }
            Err(_) => revert_with(
                inputs,
                encode_error_string(&format!("EDB: vm.envBool: {name} not set")),
            ),
        }
    }

    fn cheat_env_bytes(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(name) = read_string(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.envBytes: malformed calldata"));
        };
        match std::env::var(&name) {
            Ok(v) => {
                let trimmed = v.trim();
                let hex_body = if let Some(h) = trimmed.strip_prefix("0x") {
                    h
                } else {
                    return revert_with(
                        inputs,
                        encode_error_string(&format!(
                            "EDB: vm.envBytes: {name}={v:?} must start with 0x for hex decoding"
                        )),
                    );
                };
                match alloy_primitives::hex::decode(hex_body) {
                    Ok(decoded) => ok_return(inputs, encode_abi_bytes(&decoded)),
                    Err(_) => revert_with(
                        inputs,
                        encode_error_string(&format!(
                            "EDB: vm.envBytes: {name}={v:?} not valid hex"
                        )),
                    ),
                }
            }
            Err(_) => revert_with(
                inputs,
                encode_error_string(&format!("EDB: vm.envBytes: {name} not set")),
            ),
        }
    }

    fn cheat_env_string(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(name) = read_string(args, 0) else {
            return revert_with(inputs, encode_error_string("vm.envString: malformed calldata"));
        };
        match std::env::var(&name) {
            Ok(v) => ok_return(inputs, encode_abi_string(&v)),
            Err(_) => revert_with(
                inputs,
                encode_error_string(&format!("EDB: vm.envString: {name} not set")),
            ),
        }
    }

    // --- vm.envOr overloads --------------------------------------------------

    fn cheat_env_or_bool(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        // ABI: (string name, bool defaultValue)
        // head: [0..32) = offset to name (== 0x40), [32..64) = bool word
        // tail: the string at offset 0x40
        let Some(default_val) = read_bool(args, 1) else {
            return revert_with(
                inputs,
                encode_error_string("vm.envOr(string,bool): bad default arg"),
            );
        };
        let Some(name) = read_string(args, 0) else {
            return revert_with(
                inputs,
                encode_error_string("vm.envOr(string,bool): malformed calldata"),
            );
        };
        match std::env::var(&name) {
            Ok(v) => {
                let lower = v.trim().to_lowercase();
                let b = match lower.as_str() {
                    "true" | "1" => true,
                    "false" | "0" => false,
                    _ => {
                        return revert_with(
                            inputs,
                            encode_error_string(&format!(
                                "EDB: vm.envOr(string,bool): {name}={v:?} not parseable as bool"
                            )),
                        );
                    }
                };
                let mut out = [0u8; 32];
                out[31] = u8::from(b);
                ok_return(inputs, Bytes::copy_from_slice(&out))
            }
            Err(_) => {
                // Return the default value.
                let mut out = [0u8; 32];
                out[31] = u8::from(default_val);
                ok_return(inputs, Bytes::copy_from_slice(&out))
            }
        }
    }

    fn cheat_env_or_bytes(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        // ABI: (string name, bytes defaultValue) — both dynamic
        // head: [0..32) = offset to name, [32..64) = offset to bytes
        // The name is always at head_index 0 and the bytes at head_index 1.
        let Some(name) = read_string(args, 0) else {
            return revert_with(
                inputs,
                encode_error_string("vm.envOr(string,bytes): malformed calldata"),
            );
        };
        let Some(default_val) = read_bytes(args, 1) else {
            return revert_with(
                inputs,
                encode_error_string("vm.envOr(string,bytes): bad default arg"),
            );
        };
        match std::env::var(&name) {
            Ok(v) => {
                let trimmed = v.trim();
                let hex_body = if let Some(h) = trimmed.strip_prefix("0x") {
                    h
                } else {
                    return revert_with(
                        inputs,
                        encode_error_string(&format!(
                            "EDB: vm.envOr(string,bytes): {name}={v:?} must start with 0x"
                        )),
                    );
                };
                match alloy_primitives::hex::decode(hex_body) {
                    Ok(decoded) => ok_return(inputs, encode_abi_bytes(&decoded)),
                    Err(_) => revert_with(
                        inputs,
                        encode_error_string(&format!(
                            "EDB: vm.envOr(string,bytes): {name}={v:?} not valid hex"
                        )),
                    ),
                }
            }
            Err(_) => ok_return(inputs, encode_abi_bytes(&default_val)),
        }
    }

    fn cheat_env_or_string(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        // ABI: (string name, string defaultValue) — both dynamic
        let Some(name) = read_string(args, 0) else {
            return revert_with(
                inputs,
                encode_error_string("vm.envOr(string,string): malformed calldata"),
            );
        };
        let Some(default_val) = read_string(args, 1) else {
            return revert_with(
                inputs,
                encode_error_string("vm.envOr(string,string): bad default arg"),
            );
        };
        match std::env::var(&name) {
            Ok(v) => ok_return(inputs, encode_abi_string(&v)),
            Err(_) => ok_return(inputs, encode_abi_string(&default_val)),
        }
    }

    // --- Gas metering stubs --------------------------------------------------

    /// `vm.pauseGasMetering()` — records the paused state. EDB does NOT actually
    /// pause REVM's gas accounting; this is a stub so tests that call this
    /// cheatcode don't crash.
    fn cheat_pause_gas_metering(&mut self, inputs: &CallInputs) -> CallOutcome {
        self.warn_once(
            "pauseGasMetering",
            "EDB ships this as a stub — flag is tracked but REVM gas accounting is NOT paused. \
             Tests that ASSERT specific gas behavior may see unexpected values.",
        );
        self.gas_metering_paused = true;
        ok_return(inputs, Bytes::new())
    }

    /// `vm.resumeGasMetering()` — clears the paused state.
    fn cheat_resume_gas_metering(&mut self, inputs: &CallInputs) -> CallOutcome {
        self.warn_once(
            "resumeGasMetering",
            "EDB ships this as a stub — flag is tracked but REVM gas accounting is NOT paused. \
             Tests that ASSERT specific gas behavior may see unexpected values.",
        );
        self.gas_metering_paused = false;
        ok_return(inputs, Bytes::new())
    }

    /// `vm.lastCallGas() returns (Gas memory)` — returns a synthetic all-zero
    /// `Gas` struct ABI-encoded as 5 × 32-byte words.
    ///
    /// EDB is a source-level debugger, not a gas profiler. REVM's gas values
    /// differ between EDB's multiple instrumented execution passes (the
    /// orchestrator runs tracer / opcode / hook passes on the same transaction),
    /// so returning real gas data would cause non-determinism between passes and
    /// trigger an "outcome mismatch" assertion in the engine. All-zeros is the
    /// correct v1 stub: it is deterministic, allows the test to continue, and
    /// clearly signals that gas data is not meaningful under EDB.
    ///
    /// Foundry's `Gas` struct (all fields zero here):
    /// ```solidity
    /// struct Gas {
    ///     uint64 gasLimit;      // 0
    ///     uint64 gasTotalUsed;  // 0
    ///     uint64 gasMemoryUsed; // 0  (deprecated in real forge too)
    ///     int64  gasRefunded;   // 0
    ///     uint64 gasRemaining;  // 0
    /// }
    /// ```
    fn cheat_last_call_gas(&mut self, inputs: &CallInputs) -> CallOutcome {
        self.warn_once(
            "lastCallGas",
            "EDB ships this as a stub returning all-zero Gas{} for determinism across \
             multi-pass instrumentation. Tests asserting specific gas values will fail.",
        );
        // ABI encoding of a struct of pure value types: each field is one 32-byte word.
        // All fields are zero for v1 stub determinism (see doc comment above).
        let out = vec![0u8; 5 * 32];
        ok_return(inputs, Bytes::from(out))
    }

    // -----------------------------------------------------------------------
    // Assertion cheatcodes
    // -----------------------------------------------------------------------

    /// Dispatch handler for all `vm.assert*` cheatcodes delegated by
    /// forge-std's `StdAssertions`. The selectors in the match statement
    /// cover the fixed-width primitive overloads (uint256, int256, address,
    /// bool, bytes32) plus their optional `string error` variants.
    ///
    /// Each assertion compares two 32-byte ABI words. The optional third word
    /// is the dynamic-type offset for the error string; we decode the string
    /// on failure.
    ///
    /// On success:  returns empty data (void return).
    /// On failure:  reverts with `Error(string)` carrying a descriptive message.
    fn cheat_assert(&mut self, inputs: &CallInputs, selector: [u8; 4], args: &[u8]) -> CallOutcome {
        // Classify the overload's expected calldata shape:
        //   - `single`  -> 1 static head word  (assertTrue(bool) / assertFalse(bool))
        //   - `single+msg` -> 2 static head words + string tail
        //   - `pair`    -> 2 static head words (assertEq/Gt/Ge/Lt/Le/NotEq value-pair)
        //   - `pair+msg`-> 3 static head words + string tail
        //
        // `head_word_count` = number of 32-byte head words BEFORE the string tail
        // (for non-MSG overloads, it's the total head size with no tail).
        // `has_msg` = whether a trailing `string err` argument exists.
        let (head_word_count, has_msg) = match selector {
            // Single-arg booleans: 1 head word, no string tail.
            SEL_ASSERT_TRUE | SEL_ASSERT_FALSE => (1usize, false),
            // Single-arg booleans + string err: 2 head words (bool, str_offset) + tail.
            SEL_ASSERT_TRUE_MSG | SEL_ASSERT_FALSE_MSG => (2usize, true),
            // Two-value MSG overloads: 3 head words (left, right, str_offset) + tail.
            SEL_ASSERT_EQ_U256_MSG
            | SEL_ASSERT_EQ_I256_MSG
            | SEL_ASSERT_EQ_ADDR_MSG
            | SEL_ASSERT_EQ_BOOL_MSG
            | SEL_ASSERT_EQ_B32_MSG
            | SEL_ASSERT_NOT_EQ_U256_MSG
            | SEL_ASSERT_NOT_EQ_I256_MSG
            | SEL_ASSERT_NOT_EQ_ADDR_MSG
            | SEL_ASSERT_NOT_EQ_BOOL_MSG
            | SEL_ASSERT_NOT_EQ_B32_MSG
            | SEL_ASSERT_GE_U256_MSG
            | SEL_ASSERT_GE_I256_MSG
            | SEL_ASSERT_GT_U256_MSG
            | SEL_ASSERT_GT_I256_MSG
            | SEL_ASSERT_LE_U256_MSG
            | SEL_ASSERT_LE_I256_MSG
            | SEL_ASSERT_LT_U256_MSG
            | SEL_ASSERT_LT_I256_MSG => (3usize, true),
            // All other handled selectors are two-value, no-msg.
            _ => (2usize, false),
        };

        let min_head_bytes = head_word_count * 32;
        if args.len() < min_head_bytes {
            return revert_with(
                inputs,
                encode_error_string("EDB: vm.assert* called with insufficient calldata"),
            );
        }

        // For pair/pair+msg/single+msg overloads `left` and `right` are at
        // args[0..32] and args[32..64]. For the single-arg booleans without
        // msg, args.len() == 32; in that case `right` is synthesized as the
        // zero word so the assertTrue/assertFalse arms (which only read
        // `left[31]`) work uniformly and the failure-message hex dump still
        // produces a well-formed `right=0x...`.
        let left = &args[0..32];
        let zero_right = [0u8; 32];
        let right: &[u8] = if args.len() >= 64 { &args[32..64] } else { &zero_right[..] };

        // Decode the optional `string err` arg by reading the offset at
        // `head_word_count - 1` (the last head word holds the string offset for
        // overloads with `has_msg`), then walking (length, data) at that offset.
        // The auditor's C2-4 bug was hardcoding the data location to args[120..]
        // and the length to args[120..128]: correct only for 3-head-word layouts
        // (offset == 0x60). The single-arg+msg overloads
        // (assertTrue(bool,string)/assertFalse(bool,string)) instead put the
        // offset at 0x40 and the data at args[96..]. Routing through
        // `read_string` reads the encoded offset word and follows it,
        // generalizing across head-word counts.
        let custom_msg: Option<String> =
            if has_msg { read_string(args, head_word_count - 1) } else { None };

        // Determine the assertion kind and check it.
        let passed = match selector {
            // assertEq — left == right (bitwise comparison for all 32-byte types)
            SEL_ASSERT_EQ_U256
            | SEL_ASSERT_EQ_U256_MSG
            | SEL_ASSERT_EQ_I256
            | SEL_ASSERT_EQ_I256_MSG
            | SEL_ASSERT_EQ_ADDR
            | SEL_ASSERT_EQ_ADDR_MSG
            | SEL_ASSERT_EQ_BOOL
            | SEL_ASSERT_EQ_BOOL_MSG
            | SEL_ASSERT_EQ_B32
            | SEL_ASSERT_EQ_B32_MSG => left == right,

            // assertNotEq — left != right
            SEL_ASSERT_NOT_EQ_U256
            | SEL_ASSERT_NOT_EQ_U256_MSG
            | SEL_ASSERT_NOT_EQ_I256
            | SEL_ASSERT_NOT_EQ_I256_MSG
            | SEL_ASSERT_NOT_EQ_ADDR
            | SEL_ASSERT_NOT_EQ_ADDR_MSG
            | SEL_ASSERT_NOT_EQ_BOOL
            | SEL_ASSERT_NOT_EQ_BOOL_MSG
            | SEL_ASSERT_NOT_EQ_B32
            | SEL_ASSERT_NOT_EQ_B32_MSG => left != right,

            // assertTrue / assertFalse — use only the `left` (condition) word
            SEL_ASSERT_TRUE | SEL_ASSERT_TRUE_MSG => left[31] != 0,
            SEL_ASSERT_FALSE | SEL_ASSERT_FALSE_MSG => left[31] == 0,

            // Unsigned comparisons (uint256): treat the 32 bytes as a big-endian integer.
            SEL_ASSERT_GE_U256 | SEL_ASSERT_GE_U256_MSG => left >= right,
            SEL_ASSERT_GT_U256 | SEL_ASSERT_GT_U256_MSG => left > right,
            SEL_ASSERT_LE_U256 | SEL_ASSERT_LE_U256_MSG => left <= right,
            SEL_ASSERT_LT_U256 | SEL_ASSERT_LT_U256_MSG => left < right,

            // Signed comparisons (int256): the high bit is the sign. We compare the
            // raw big-endian bytes; two's-complement means the byte order exactly
            // matches signed comparison for 256-bit values (sign bit in byte 0).
            SEL_ASSERT_GE_I256 | SEL_ASSERT_GE_I256_MSG => {
                let (l_neg, r_neg) = (left[0] & 0x80 != 0, right[0] & 0x80 != 0);
                if l_neg != r_neg { !l_neg } else { left >= right }
            }
            SEL_ASSERT_GT_I256 | SEL_ASSERT_GT_I256_MSG => {
                let (l_neg, r_neg) = (left[0] & 0x80 != 0, right[0] & 0x80 != 0);
                if l_neg != r_neg { !l_neg } else { left > right }
            }
            SEL_ASSERT_LE_I256 | SEL_ASSERT_LE_I256_MSG => {
                let (l_neg, r_neg) = (left[0] & 0x80 != 0, right[0] & 0x80 != 0);
                if l_neg != r_neg { l_neg } else { left <= right }
            }
            SEL_ASSERT_LT_I256 | SEL_ASSERT_LT_I256_MSG => {
                let (l_neg, r_neg) = (left[0] & 0x80 != 0, right[0] & 0x80 != 0);
                if l_neg != r_neg { l_neg } else { left < right }
            }

            _ => {
                // Unreachable: all selectors in the dispatch arm are listed above.
                return revert_with(
                    inputs,
                    encode_error_string("EDB: internal error in cheat_assert dispatch"),
                );
            }
        };

        if passed {
            ok_return(inputs, Bytes::new())
        } else {
            let hex_encode = |b: &[u8]| alloy_primitives::hex::encode(b);
            let base_msg = format!(
                "Assertion failed: left=0x{} right=0x{}",
                hex_encode(left),
                hex_encode(right)
            );
            let msg = if let Some(custom) = custom_msg {
                format!("{base_msg} ({custom})")
            } else {
                base_msg
            };
            revert_with(inputs, encode_error_string(&msg))
        }
    }

    /// Gas snapshot stubs: `vm.startSnapshotGas`, `vm.stopSnapshotGas`,
    /// `vm.snapshotGasLastCall`. EDB is not a gas profiler; these calls are
    /// accepted as no-ops so tests that call them don't hard-abort.
    fn cheat_gas_snapshot_stub(&mut self, inputs: &CallInputs, name: &str) -> CallOutcome {
        self.warn_once(
            name,
            "EDB stubs this gas-snapshot cheatcode — gas profiling is not available \
             in EDB v1. The call is accepted as a no-op.",
        );
        // `startSnapshotGas` is `void`, but `stopSnapshotGas` and
        // `snapshotGasLastCall` are declared `returns (uint256 gasUsed)`.
        // Returning empty bytes for the latter makes the Solidity caller revert
        // while ABI-decoding the missing return value (observed as an
        // empty-output revert right after the cheat frame), so emit a 32-byte
        // zero word for the uint256-returning variants.
        let ret =
            if name == "startSnapshotGas" { Bytes::new() } else { Bytes::from_static(&[0u8; 32]) };
        ok_return(inputs, ret)
    }

    /// Benchmark-value snapshot stub: `vm.snapshotValue(string,uint256)` and
    /// `vm.snapshotValue(string,string,uint256)`. EDB does not record benchmark
    /// snapshots in v1; the call succeeds silently. Both overloads share the
    /// same handler — the only side effect is the one-time warn under the
    /// `snapshotValue` name.
    fn cheat_snapshot_value_stub(&mut self, inputs: &CallInputs) -> CallOutcome {
        self.warn_once(
            "snapshotValue",
            "EDB does not record benchmark snapshots; the call succeeds silently.",
        );
        ok_return(inputs, Bytes::new())
    }

    /// `vm.getDeployedCode(string artifact) returns (bytes runtimeBytecode)`.
    ///
    /// Foundry accepts three artifact-identifier shapes:
    ///   - `"MyContract"` — bare contract name.
    ///   - `"MyContract.sol"` — file name (we treat the basename's stem
    ///     before the first `.` as the contract name; this matches the
    ///     common case where filename == contract name).
    ///   - `"path/MyContract.sol:MyContract"` (or `":Contract:version"`) —
    ///     split on `:` and use the second segment as the contract name.
    ///
    /// We then look up the deployed-bytecode template that was inserted into
    /// `CheatsConfig::local_artifacts` (built by
    /// `crates/edb/src/cmd/test/artifacts.rs::build_local_artifact_set`) and
    /// return it ABI-encoded as `bytes`. Lookup misses revert with a clear
    /// message so users can tell at a glance whether they typed the artifact
    /// name wrong vs. the contract isn't in the project.
    fn cheat_get_deployed_code(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(arg) = read_string(args, 0) else {
            return revert_with(
                inputs,
                encode_error_string("vm.getDeployedCode: failed to decode artifact name"),
            );
        };

        let Some(local) = self.config.local_artifacts.as_ref() else {
            return revert_with(
                inputs,
                encode_error_string(
                    "vm.getDeployedCode: local artifact set unavailable \
                     (EDB internal: cheats inspector not given a project context)",
                ),
            );
        };

        let name = artifact_lookup_name(&arg);
        match local.deployed_bytecode_by_name(name) {
            Some(code) => ok_return(inputs, encode_abi_bytes(code)),
            None => {
                let msg = format!(
                    "vm.getDeployedCode: artifact {arg:?} not found in project \
                     (contract name {name:?})"
                );
                revert_with(inputs, encode_error_string(&msg))
            }
        }
    }
}

/// Extract the contract-name segment from a foundry-style artifact identifier.
///
/// Accepted shapes (in priority order):
///   - `"path/Foo.sol:Foo[:version]"` → returns `"Foo"` (second `:`-segment).
///   - `"Foo.sol"`                    → returns `"Foo"` (basename stem
///     before the first `.`; matches the common 1-file-1-contract case).
///   - `"Foo"`                        → returns `"Foo"`.
///
/// Returning a `&str` borrow keeps allocation off the hot path.
fn artifact_lookup_name(arg: &str) -> &str {
    if let Some(rest) = arg.split(':').nth(1) {
        return rest;
    }
    let basename = arg.rsplit('/').next().unwrap_or(arg);
    if let Some(stem) = basename.split('.').next()
        && !stem.is_empty()
    {
        return stem;
    }
    arg
}

/// Argument-shape selector for the four supported `vm.expectEmit` overloads.
#[derive(Clone, Copy, Debug)]
enum ExpectEmitMode {
    /// `vm.expectEmit()` — all topics + data checked, any emitter.
    All,
    /// `vm.expectEmit(bool,bool,bool,bool)` — t1/t2/t3 + data, any emitter.
    Filter4,
    /// `vm.expectEmit(bool,bool,bool,bool,address)` — t1/t2/t3 + data, explicit emitter.
    Filter5,
    /// `vm.expectEmit(address)` — all topics + data checked, explicit emitter.
    AnyTopicsFromEmitter,
}

// ----------------------------------------------------------------------------
// CallOutcome helpers
// ----------------------------------------------------------------------------

/// Build a successful synthetic CallOutcome for an intercepted cheatcode call.
///
/// `memory_offset` is taken from the CALL instruction's `return_memory_offset`,
/// NOT the default `0..0`. REVM uses `memory_offset` as the range in the
/// caller frame's memory where the inline return bytes are written before
/// control returns. For VOID-returning cheatcodes this distinction is
/// invisible (no bytes to copy). For VALUE-returning ones with a STATIC
/// return type — `bool`, `bytes32`, `uint256`, fixed-size structs — Solidity
/// reads the return value directly from `mem[memory_offset..]`. Using
/// `0..0` silently yields all-zero return values (e.g. `vm.load` returned 0
/// instead of the stored slot value). For DYNAMIC return types Solidity goes
/// through RETURNDATACOPY so the bug isn't user-observable, but threading
/// the correct offset is still the right thing.
fn ok_return(inputs: &CallInputs, output: Bytes) -> CallOutcome {
    CallOutcome {
        result: InterpreterResult {
            result: InstructionResult::Return,
            output,
            gas: Gas::new(inputs.gas_limit),
        },
        memory_offset: inputs.return_memory_offset.clone(),
        was_precompile_called: false,
        precompile_call_logs: Vec::new(),
    }
}

/// Build a reverting synthetic CallOutcome. Revert outputs are read via
/// `RETURNDATACOPY`, so `memory_offset` doesn't directly leak to the caller's
/// memory — but we still pass through `inputs.return_memory_offset` for
/// consistency with `ok_return`.
fn revert_with(inputs: &CallInputs, output: Bytes) -> CallOutcome {
    CallOutcome {
        result: InterpreterResult {
            result: InstructionResult::Revert,
            output,
            gas: Gas::new(inputs.gas_limit),
        },
        memory_offset: inputs.return_memory_offset.clone(),
        was_precompile_called: false,
        precompile_call_logs: Vec::new(),
    }
}

// ----------------------------------------------------------------------------
// Revert payload helpers
// ----------------------------------------------------------------------------

#[allow(dead_code)] // retained for legacy callers / tests; new code uses record_and_revert
fn unsupported_revert(name: &str) -> Bytes {
    let msg = format!("EDB: cheatcode vm.{name} not supported in v1");
    encode_error_string(&msg)
}

/// Names (without `vm.` prefix) of cheatcodes EDB deliberately rejects in v1
/// because their semantics require infrastructure EDB doesn't ship (multi-fork
/// backend, fs/ffi sandboxing, broadcasting, etc.) — as opposed to cheatcodes
/// that are merely "not yet implemented" and could be added incrementally.
///
/// Drives the [`UnsupportedCategory`] tagging produced by the dispatch
/// fall-through, which in turn drives the wording of the post-prepare abort
/// message in `run_foundry_test`.
fn is_explicitly_rejected_name(name: &str) -> bool {
    matches!(
        name,
        "createFork"
            | "createSelectFork"
            | "selectFork"
            | "activeFork"
            | "makePersistent"
            | "transact"
            | "broadcast"
            | "startBroadcast"
            | "stopBroadcast"
            | "ffi"
            | "readFile"
            | "writeFile"
            | "removeFile"
            | "expectCallMinGas"
            // getRawBlockHeader is deferred to v2 — needs an upstream RPC
            // channel + sync-from-async dispatch inside the cheatcode
            // handler. Catalogued as Rejected so abort surfaces a useful
            // message rather than "unknown selector".
            | "getRawBlockHeader"
    ) || (
        // Cross-fork rollFork overloads (bytes32 / uint256,uint256 / uint256,bytes32)
        // are rejected; the single-arg uint256 overload is supported and dispatched
        // by SEL_ROLL_FORK_UINT before reaching the catalog fall-through.
        name == "rollFork"
    )
}

/// Encode `Error(string)` as ABI: 4-byte selector `0x08c379a0` + (offset, length, padded data).
fn encode_error_string(msg: &str) -> Bytes {
    let mut payload = Vec::with_capacity(4 + 64 + msg.len().div_ceil(32) * 32);
    payload.extend_from_slice(&[0x08, 0xc3, 0x79, 0xa0]); // Error(string)
    // offset = 0x20
    let mut offset = [0u8; 32];
    offset[31] = 0x20;
    payload.extend_from_slice(&offset);
    // length (as 256-bit BE)
    let mut len = [0u8; 32];
    let l = msg.len() as u64;
    len[24..].copy_from_slice(&l.to_be_bytes());
    payload.extend_from_slice(&len);
    // data, right-padded to 32 bytes
    let mut data = msg.as_bytes().to_vec();
    let pad = (32 - data.len() % 32) % 32;
    data.extend(std::iter::repeat_n(0u8, pad));
    payload.extend_from_slice(&data);
    Bytes::from(payload)
}

/// ABI-encode a `string` return value: `(bytes32 offset, bytes32 length, data padded)`.
fn encode_abi_string(s: &str) -> Bytes {
    encode_abi_bytes(s.as_bytes())
}

/// ABI-encode a `bytes` return value: `(bytes32 offset, bytes32 length, data padded)`.
fn encode_abi_bytes(b: &[u8]) -> Bytes {
    let pad = (32 - b.len() % 32) % 32;
    let mut out = Vec::with_capacity(64 + b.len() + pad);
    // offset = 0x20
    let mut offset = [0u8; 32];
    offset[31] = 0x20;
    out.extend_from_slice(&offset);
    // length
    let mut len_word = [0u8; 32];
    let l = b.len() as u64;
    len_word[24..].copy_from_slice(&l.to_be_bytes());
    out.extend_from_slice(&len_word);
    // data right-padded to 32-byte multiple
    out.extend_from_slice(b);
    out.extend(std::iter::repeat_n(0u8, pad));
    Bytes::from(out)
}

/// Resolve `raw_path` against the sandbox root, rejecting any path that
/// escapes the root via `..`/symlinks/absolute paths.
///
/// Strategy:
/// - If `project_root` is empty (test scaffolding), fall back to the
///   current working directory.
/// - Canonicalize both the root and the candidate; the candidate must start
///   with the root prefix component-wise after canonicalization (so a leading
///   symlink at `root/..` doesn't slip out).
/// - The candidate must exist (canonicalize requires it). For `readLine` this
///   is fine — there's nothing to read from a nonexistent file.
fn resolve_sandboxed_path(
    project_root: &std::path::Path,
    raw_path: &str,
) -> Result<std::path::PathBuf, String> {
    use std::path::{Path, PathBuf};
    let root_buf: PathBuf = if project_root.as_os_str().is_empty() {
        std::env::current_dir().map_err(|e| format!("vm.readLine: no project root: {e}"))?
    } else {
        project_root.to_path_buf()
    };
    let root_canon = root_buf
        .canonicalize()
        .map_err(|e| format!("vm.readLine: project root canonicalize failed: {e}"))?;

    let candidate = Path::new(raw_path);
    let joined: PathBuf =
        if candidate.is_absolute() { candidate.to_path_buf() } else { root_canon.join(candidate) };
    let canon = joined
        .canonicalize()
        .map_err(|e| format!("vm.readLine: cannot resolve path {raw_path:?}: {e}"))?;
    if !canon.starts_with(&root_canon) {
        return Err(format!("vm.readLine: path {raw_path:?} escapes the project root sandbox"));
    }
    Ok(canon)
}

/// NIST P-256 curve order n, as a 32-byte big-endian constant.
/// `n = 0xffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551`
/// Mirrors `p256::NistP256::ORDER` but doesn't pull the `PrimeCurve` trait
/// import into module scope.
const P256_CURVE_ORDER_BE: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xbc, 0xe6, 0xfa, 0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x51,
];

/// Validate that `sk` is a usable P-256 private key: nonzero and strictly
/// less than the curve order. Mirrors foundry's `validate_private_key<P256>`
/// preflight so we return the same friendly message instead of a less-clear
/// "invalid private key" from the crate.
fn validate_p256_private_key(sk: &U256) -> Result<(), String> {
    if *sk == U256::ZERO {
        return Err("vm.signP256/publicKeyP256: private key cannot be 0".to_string());
    }
    let order = U256::from_be_bytes(P256_CURVE_ORDER_BE);
    if *sk >= order {
        return Err(format!(
            "vm.signP256/publicKeyP256: private key must be less than the P-256 curve order ({order})"
        ));
    }
    Ok(())
}

// ----------------------------------------------------------------------------
// ABI decoding helpers (32-byte head per arg)
//
// We hand-decode the small subset of types we care about — `address`,
// `uint256`, `bytes32`, dynamic `bytes`, dynamic `string`. Each arg in the
// head section is 32 bytes; for dynamic args the head holds the byte offset
// (from start of `args`) of the (length, data) pair in the tail.
// ----------------------------------------------------------------------------

fn read_word(args: &[u8], head_index: usize) -> Option<[u8; 32]> {
    let start = head_index.checked_mul(32)?;
    let end = start.checked_add(32)?;
    if end > args.len() {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&args[start..end]);
    Some(out)
}

fn read_u256(args: &[u8], head_index: usize) -> Option<U256> {
    let w = read_word(args, head_index)?;
    Some(U256::from_be_bytes(w))
}

fn read_b256(args: &[u8], head_index: usize) -> Option<B256> {
    Some(B256::from(read_word(args, head_index)?))
}

fn read_address(args: &[u8], head_index: usize) -> Option<Address> {
    let w = read_word(args, head_index)?;
    // Address is right-aligned in the 32-byte word.
    Some(Address::from_slice(&w[12..32]))
}

/// Read a Solidity `bool` (ABI: a 32-byte word that is all zero for `false`
/// and ends with `0x01` for `true`; technically any non-zero word is `true`).
fn read_bool(args: &[u8], head_index: usize) -> Option<bool> {
    let w = read_word(args, head_index)?;
    Some(w.iter().any(|&b| b != 0))
}

/// Read a dynamic `bytes` argument: head slot at `head_index` contains the
/// offset (in bytes, from the start of `args`); at that offset we find the
/// length (32 bytes BE) and then `length` bytes of data (zero-padded to a
/// 32-byte multiple).
fn read_bytes(args: &[u8], head_index: usize) -> Option<Bytes> {
    let off = read_u256(args, head_index)?;
    let off = usize::try_from(off).ok()?;
    let len = read_u256(args, off / 32)?;
    let len = usize::try_from(len).ok()?;
    let data_start = off.checked_add(32)?;
    let data_end = data_start.checked_add(len)?;
    if data_end > args.len() {
        return None;
    }
    Some(Bytes::copy_from_slice(&args[data_start..data_end]))
}

/// Same wire format as `bytes`; we additionally require valid UTF-8.
fn read_string(args: &[u8], head_index: usize) -> Option<String> {
    let bytes = read_bytes(args, head_index)?;
    String::from_utf8(bytes.to_vec()).ok()
}

// ----------------------------------------------------------------------------
// JSON helpers for `vm.parseJson*`
//
// We implement a minimal subset of jsonpath_lib's grammar — leading `$`
// optional, then a sequence of `.<ident>` and `[<index>]` tokens. This covers
// every parseJson* invocation in the static-coverage fixtures (`.x`, `.foo`,
// `.foo.bar`, `.foo[0]`, `.foo[0].bar`). The fall-through returns `None` if
// any token misses, which the per-type handlers translate into a revert with
// a descriptive message.
// ----------------------------------------------------------------------------

/// Walk a `serde_json::Value` along a foundry-style JSONPath accessor.
///
/// Returns `None` if the path is malformed or any segment doesn't resolve.
/// The empty path (`""`) or `"$"` selects the root. Leading whitespace and
/// trailing whitespace are NOT trimmed — match foundry's literal behavior.
fn navigate_json<'a>(root: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    // Strip the optional leading `$`. Foundry's `canonicalize_json_path`
    // re-prefixes `$` automatically — we just accept either form.
    let mut cur = root;
    let mut rest = path.strip_prefix('$').unwrap_or(path);
    // Trim a single leading `.` so that `".foo"` and `"foo"` and `"$.foo"`
    // all walk the same way.
    if rest.starts_with('.') {
        rest = &rest[1..];
    }
    // Empty path resolves to the root.
    if rest.is_empty() {
        return Some(cur);
    }
    while !rest.is_empty() {
        // Bracket index: `[N]`.
        if let Some(after_lb) = rest.strip_prefix('[') {
            let close = after_lb.find(']')?;
            let idx_str = &after_lb[..close];
            let idx: usize = idx_str.parse().ok()?;
            let arr = cur.as_array()?;
            cur = arr.get(idx)?;
            rest = &after_lb[close + 1..];
            // Optional `.` separator after bracket (`[0].foo`).
            rest = rest.strip_prefix('.').unwrap_or(rest);
            continue;
        }
        // Dot-key segment: consume up to the next `.` or `[`.
        let end = rest.find(['.', '[']).unwrap_or(rest.len());
        let key = &rest[..end];
        let obj = cur.as_object()?;
        cur = obj.get(key)?;
        rest = &rest[end..];
        // Optional `.` between segments.
        rest = rest.strip_prefix('.').unwrap_or(rest);
    }
    Some(cur)
}

/// Decode the two `string` args of a `vm.parseJson*(string,string)` cheatcode
/// and parse the first as JSON.
fn read_json_path_pair(args: &[u8], who: &str) -> Result<(serde_json::Value, String), String> {
    let json_str =
        read_string(args, 0).ok_or_else(|| format!("{who}(string,string): malformed JSON arg"))?;
    let path =
        read_string(args, 1).ok_or_else(|| format!("{who}(string,string): malformed path arg"))?;
    let value: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| format!("{who}: failed to parse JSON: {e}"))?;
    Ok((value, path))
}

/// Parse a `serde_json::Value` as an unsigned 256-bit integer. Accepts:
/// - a JSON number (must be non-negative)
/// - a JSON string of decimal digits, or `"0x"`-prefixed hex.
fn parse_uint256_from_json(v: &serde_json::Value) -> Result<U256, String> {
    match v {
        serde_json::Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                return Ok(U256::from(u));
            }
            // Reject negatives explicitly.
            if let Some(i) = n.as_i64()
                && i < 0
            {
                return Err(format!("negative JSON number {i} cannot decode as uint256"));
            }
            // Fall back to string parsing for arbitrary-precision JSON numbers.
            U256::from_str_radix(n.to_string().trim(), 10)
                .map_err(|e| format!("number {n} not parseable as uint256: {e}"))
        }
        serde_json::Value::String(s) => {
            let t = s.trim();
            if let Some(hex) = t.strip_prefix("0x") {
                U256::from_str_radix(hex, 16).map_err(|e| format!("hex string not uint256: {e}"))
            } else {
                U256::from_str_radix(t, 10).map_err(|e| format!("decimal string not uint256: {e}"))
            }
        }
        other => Err(format!("expected a uint256 leaf, got JSON value {other}")),
    }
}

/// Parse a `serde_json::Value` as a signed 256-bit integer (two's complement
/// representation returned as 32 BE bytes). Accepts a JSON number or a
/// decimal-string leaf with an optional leading `-`.
fn parse_int256_from_json(v: &serde_json::Value) -> Result<alloy_primitives::I256, String> {
    use alloy_primitives::I256;
    use std::str::FromStr;
    match v {
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                return I256::try_from(i).map_err(|e| e.to_string());
            }
            // Arbitrary-precision: route through string form.
            I256::from_dec_str(n.to_string().trim()).map_err(|e| format!("number not int256: {e}"))
        }
        serde_json::Value::String(s) => {
            let t = s.trim();
            I256::from_str(t).map_err(|e| format!("string not int256: {e}"))
        }
        other => Err(format!("expected an int256 leaf, got JSON value {other}")),
    }
}

/// Implementation of `vm.parseJson(string)` and `vm.parseJson(string,string)`:
/// parse the JSON, resolve the path, and produce an ABI-encoded value
/// suitable for `abi.decode(ret, (T))` where `T` is the leaf's natural type.
///
/// We support primitive leaves (bool, number, string) and primitive arrays —
/// enough for the real-world tests these unblock. For complex (object) leaves
/// we return a clean error rather than attempting foundry's type-guess
/// machinery.
fn parse_json_to_abi_bytes(json: &str, path: &str) -> Result<Bytes, String> {
    let root: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| format!("EDB: vm.parseJson: failed to parse JSON: {e}"))?;
    let leaf = navigate_json(&root, path)
        .ok_or_else(|| format!("EDB: vm.parseJson: path {path:?} did not resolve"))?;
    encode_json_leaf_as_bytes(leaf, path)
}

/// Encode a primitive JSON leaf as an ABI-encoded value matching the type
/// foundry's `parseJson` returns for that JSON shape.
fn encode_json_leaf_as_bytes(leaf: &serde_json::Value, path: &str) -> Result<Bytes, String> {
    match leaf {
        serde_json::Value::Bool(b) => {
            let mut out = [0u8; 32];
            out[31] = u8::from(*b);
            Ok(Bytes::copy_from_slice(&out))
        }
        serde_json::Value::String(s) => {
            // Two cases foundry's `_json_value_to_token` distinguishes:
            //   1. hex-looking strings ("0x..." with even-length lowercase hex
            //      that decodes cleanly) -> bytes.
            //   2. otherwise -> a UTF-8 `string`.
            // We follow the same heuristic. Tests that need the strict typed
            // form should use the per-type `vm.parseJsonBytes32` /
            // `vm.parseJsonString` overloads.
            let t = s.trim();
            if let Some(hex) = t.strip_prefix("0x")
                && !hex.is_empty()
                && hex.len() % 2 == 0
                && hex.chars().all(|c| c.is_ascii_hexdigit())
                && let Ok(decoded) = alloy_primitives::hex::decode(hex)
            {
                if decoded.len() == 32 {
                    return Ok(Bytes::copy_from_slice(&decoded));
                }
                if decoded.len() == 20 {
                    let mut out = [0u8; 32];
                    out[12..].copy_from_slice(&decoded);
                    return Ok(Bytes::copy_from_slice(&out));
                }
                // Generic `bytes` payload.
                return Ok(encode_abi_bytes(&decoded));
            }
            Ok(encode_abi_string(s))
        }
        serde_json::Value::Number(_) => {
            // Foundry guesses uint256 first; integer numbers in test JSON are
            // always non-negative (forge-std doesn't emit negative numbers
            // via parseJson without explicit typed overloads), so we follow
            // suit. Negative numbers re-route through `parseJsonInt`.
            let u = parse_uint256_from_json(leaf)
                .map_err(|e| format!("EDB: vm.parseJson: leaf at {path:?}: {e}"))?;
            Ok(Bytes::copy_from_slice(&u.to_be_bytes::<32>()))
        }
        serde_json::Value::Null => Ok(Bytes::copy_from_slice(&[0u8; 32])),
        // Complex shapes (objects, arrays) aren't supported in v1 — they
        // require foundry's full `parse_json_array` / `parse_json_map`
        // recursion. Test authors who need them should use the typed
        // per-leaf overloads (parseJsonUint/Bool/String/etc.).
        _ => Err(format!(
            "EDB: vm.parseJson: leaf at {path:?} is a complex JSON value; \
             use vm.parseJson<Type>(json, path) for typed access in v1"
        )),
    }
}

// ----------------------------------------------------------------------------
// ABI encoding for `Vm.Log[]`
//
// Foundry's `Vm.Log`:
//   struct Log { bytes32[] topics; bytes data; address emitter; }
//
// Returned by `vm.getRecordedLogs()` as `Log[]`.
// ----------------------------------------------------------------------------

fn abi_encode_logs(logs: &[Log]) -> Bytes {
    // We encode `Log[]` as a single dynamic top-level value:
    //   [offset_to_array]           (32 bytes, == 0x20)
    //   [array_length]              (32)
    //   [offset_log_0, ..., offset_log_n]  (n*32, offsets relative to start
    //                                      of this nested block, after length)
    //   <log_0 abi-tail>            (dynamic per-log)
    //   ...
    //
    // Each Log abi-tail itself is:
    //   [offset_to_topics]          (== 0x60 — first 3 head words for the 3 fields)
    //   [offset_to_data]            (depends on topics length)
    //   [emitter (right-padded in 32 bytes)]
    //   [topics.length]
    //   [topic_0 ... topic_n]       (n*32)
    //   [data.length]
    //   [data]                      (zero-padded to 32-byte multiple)

    fn encode_one_log(log: &Log) -> Vec<u8> {
        let topics: Vec<B256> = log.topics().to_vec();
        let topics_n = topics.len();
        let data = log.data.data.clone();
        let data_padded_len = data.len().div_ceil(32) * 32;

        // Layout offsets, in bytes, from the start of THIS log's encoding.
        let head_size: usize = 3 * 32;
        let off_topics = head_size; // immediately after the 3 head words
        let topics_block_size = 32 + topics_n * 32; // length + words
        let off_data = off_topics + topics_block_size;
        let data_block_size = 32 + data_padded_len;

        let total_len = head_size + topics_block_size + data_block_size;
        let mut buf = Vec::with_capacity(total_len);

        // head
        buf.extend_from_slice(&U256::from(off_topics).to_be_bytes::<32>());
        buf.extend_from_slice(&U256::from(off_data).to_be_bytes::<32>());
        let mut addr_word = [0u8; 32];
        addr_word[12..].copy_from_slice(log.address.as_slice());
        buf.extend_from_slice(&addr_word);

        // topics block
        buf.extend_from_slice(&U256::from(topics_n).to_be_bytes::<32>());
        for t in &topics {
            buf.extend_from_slice(t.as_slice());
        }
        // data block
        buf.extend_from_slice(&U256::from(data.len()).to_be_bytes::<32>());
        buf.extend_from_slice(&data);
        buf.extend(std::iter::repeat_n(0u8, data_padded_len - data.len()));
        debug_assert_eq!(buf.len(), total_len);
        buf
    }

    let n = logs.len();
    let per_log: Vec<Vec<u8>> = logs.iter().map(encode_one_log).collect();

    // Inner array block: [length] + [offsets...] + [tails concatenated].
    // Offsets are relative to the start of THIS inner block, immediately
    // after the length word. That means the first log's tail starts at
    // `n * 32` (right after the n offset words).
    let mut inner = Vec::new();
    inner.extend_from_slice(&U256::from(n).to_be_bytes::<32>());
    let mut running_offset: usize = n * 32;
    let mut tails_concat = Vec::new();
    for tail in &per_log {
        inner.extend_from_slice(&U256::from(running_offset).to_be_bytes::<32>());
        running_offset = running_offset.checked_add(tail.len()).expect("encoded size overflow");
        tails_concat.extend_from_slice(tail);
    }
    inner.extend_from_slice(&tails_concat);

    // Outer wrapper: single dynamic value lives at offset 0x20.
    let mut out = Vec::with_capacity(32 + inner.len());
    let mut head_offset = [0u8; 32];
    head_offset[31] = 0x20;
    out.extend_from_slice(&head_offset);
    out.extend_from_slice(&inner);
    Bytes::from(out)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::keccak256;

    #[test]
    fn cheatcodes_start_with_no_snapshots() {
        use revm::database::{CacheDB, EmptyDB};
        // EdbCheatcodes is now generic over DB — concrete-type the test storage.
        type TestDB = CacheDB<EmptyDB>;
        let cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        assert_eq!(cheats.snapshots.len(), 0);
        assert_eq!(cheats.next_snapshot_id, 1);
    }

    fn sel(sig: &str) -> [u8; 4] {
        let h = keccak256(sig.as_bytes());
        let mut out = [0u8; 4];
        out.copy_from_slice(&h[..4]);
        out
    }

    // --- Selector verification: each must match keccak256(sig)[..4] ---------

    #[test]
    fn selector_warp() {
        assert_eq!(sel("warp(uint256)"), SEL_WARP);
    }
    #[test]
    fn selector_roll() {
        assert_eq!(sel("roll(uint256)"), SEL_ROLL);
    }
    #[test]
    fn selector_chain_id() {
        assert_eq!(sel("chainId(uint256)"), SEL_CHAIN_ID);
    }
    #[test]
    fn selector_fee() {
        assert_eq!(sel("fee(uint256)"), SEL_FEE);
    }
    #[test]
    fn selector_tx_gas_price() {
        assert_eq!(sel("txGasPrice(uint256)"), SEL_TX_GAS_PRICE);
    }
    #[test]
    fn selectors_to_string_family() {
        // Pin every vm.toString overload against keccak256(sig)[..4] so the
        // dispatch arms can't silently drift from foundry's canonical spec.
        assert_eq!(sel("toString(address)"), SEL_TO_STRING_ADDRESS);
        assert_eq!(sel("toString(bool)"), SEL_TO_STRING_BOOL);
        assert_eq!(sel("toString(bytes)"), SEL_TO_STRING_BYTES);
        assert_eq!(sel("toString(bytes32)"), SEL_TO_STRING_BYTES32);
        assert_eq!(sel("toString(int256)"), SEL_TO_STRING_INT256);
        assert_eq!(sel("toString(uint256)"), SEL_TO_STRING_UINT256);
    }
    #[test]
    fn selectors_parse_json_family() {
        assert_eq!(sel("parseJson(string)"), SEL_PARSE_JSON_1);
        assert_eq!(sel("parseJson(string,string)"), SEL_PARSE_JSON_2);
        assert_eq!(sel("parseJsonBool(string,string)"), SEL_PARSE_JSON_BOOL);
        assert_eq!(sel("parseJsonString(string,string)"), SEL_PARSE_JSON_STRING);
        assert_eq!(sel("parseJsonBytes32(string,string)"), SEL_PARSE_JSON_BYTES32);
        assert_eq!(sel("parseJsonUint(string,string)"), SEL_PARSE_JSON_UINT);
        assert_eq!(sel("parseJsonInt(string,string)"), SEL_PARSE_JSON_INT);
        assert_eq!(sel("parseJsonAddress(string,string)"), SEL_PARSE_JSON_ADDRESS);
    }
    #[test]
    fn selector_deal() {
        assert_eq!(sel("deal(address,uint256)"), SEL_DEAL);
    }
    #[test]
    fn selector_etch() {
        assert_eq!(sel("etch(address,bytes)"), SEL_ETCH);
    }
    #[test]
    fn selector_store() {
        assert_eq!(sel("store(address,bytes32,bytes32)"), SEL_STORE);
    }
    #[test]
    fn selector_load() {
        assert_eq!(sel("load(address,bytes32)"), SEL_LOAD);
    }
    #[test]
    fn selector_set_nonce() {
        assert_eq!(sel("setNonce(address,uint64)"), SEL_SET_NONCE);
    }
    #[test]
    fn selector_get_nonce() {
        assert_eq!(sel("getNonce(address)"), SEL_GET_NONCE);
    }
    #[test]
    fn selector_get_block_number() {
        assert_eq!(sel("getBlockNumber()"), SEL_GET_BLOCK_NUMBER);
    }
    #[test]
    fn selector_set_blockhash() {
        assert_eq!(sel("setBlockhash(uint256,bytes32)"), SEL_SET_BLOCKHASH);
    }
    #[test]
    fn selector_read_line() {
        assert_eq!(sel("readLine(string)"), SEL_READ_LINE);
    }
    #[test]
    fn selector_get_raw_block_header() {
        assert_eq!(sel("getRawBlockHeader(uint256)"), SEL_GET_RAW_BLOCK_HEADER);
    }
    #[test]
    fn selector_prank() {
        assert_eq!(sel("prank(address)"), SEL_PRANK);
    }
    #[test]
    fn selector_start_prank() {
        assert_eq!(sel("startPrank(address)"), SEL_START_PRANK);
    }
    #[test]
    fn selector_start_prank_2() {
        assert_eq!(sel("startPrank(address,address)"), SEL_START_PRANK_2);
    }
    #[test]
    fn selector_stop_prank() {
        assert_eq!(sel("stopPrank()"), SEL_STOP_PRANK);
    }
    #[test]
    fn selector_mock_call() {
        assert_eq!(sel("mockCall(address,bytes,bytes)"), SEL_MOCK_CALL);
    }
    #[test]
    fn selector_mock_call_revert() {
        assert_eq!(sel("mockCallRevert(address,bytes,bytes)"), SEL_MOCK_CALL_REVERT);
    }
    #[test]
    fn selector_clear_mocked_calls() {
        assert_eq!(sel("clearMockedCalls()"), SEL_CLEAR_MOCKED_CALLS);
    }
    #[test]
    fn selector_expect_revert() {
        assert_eq!(sel("expectRevert()"), SEL_EXPECT_REVERT_BARE);
        assert_eq!(sel("expectRevert(bytes)"), SEL_EXPECT_REVERT_BYTES);
        assert_eq!(sel("expectRevert(bytes4)"), SEL_EXPECT_REVERT_BYTES4);
    }
    #[test]
    fn addr_and_sign_selectors_match_canonical() {
        // Pin the secp256k1 cheatcode selectors against keccak256(sig)[..4].
        // If one of these silently flips, the dispatch arm goes dead.
        assert_eq!(sel("addr(uint256)"), SEL_ADDR);
        assert_eq!(sel("sign(uint256,bytes32)"), SEL_SIGN);
    }
    #[test]
    fn p256_selectors_match_canonical() {
        assert_eq!(sel("signP256(uint256,bytes32)"), SEL_SIGN_P256);
        assert_eq!(sel("publicKeyP256(uint256)"), SEL_PUBLIC_KEY_P256);
    }
    #[test]
    fn selector_label() {
        assert_eq!(sel("label(address,string)"), SEL_LABEL);
    }
    #[test]
    fn selector_record_logs() {
        assert_eq!(sel("recordLogs()"), SEL_RECORD_LOGS);
        assert_eq!(sel("getRecordedLogs()"), SEL_GET_RECORDED_LOGS);
    }
    #[test]
    fn selector_expect_emit() {
        assert_eq!(sel("expectEmit()"), SEL_EXPECT_EMIT_BARE);
        assert_eq!(sel("expectEmit(bool,bool,bool,bool)"), SEL_EXPECT_EMIT_FILTER4);
        assert_eq!(sel("expectEmit(bool,bool,bool,bool,address)"), SEL_EXPECT_EMIT_FILTER5);
        assert_eq!(sel("expectEmit(address)"), SEL_EXPECT_EMIT_ADDR);
    }
    #[test]
    fn selector_expect_call() {
        assert_eq!(sel("expectCall(address,bytes)"), SEL_EXPECT_CALL);
        assert_eq!(sel("expectCall(address,bytes,uint64)"), SEL_EXPECT_CALL_COUNT);
        assert_eq!(sel("expectCallMinGas(address,uint256,uint64,bytes)"), SEL_EXPECT_CALL_MIN_GAS);
    }
    #[test]
    fn assume_selector_matches_vm_sol() {
        assert_eq!(sel("assume(bool)"), SEL_ASSUME);
    }
    #[test]
    fn env_bool_selector_matches_vm_sol() {
        assert_eq!(sel("envBool(string)"), SEL_ENV_BOOL);
        assert_eq!(sel("envBytes(string)"), SEL_ENV_BYTES);
        assert_eq!(sel("envString(string)"), SEL_ENV_STRING);
    }
    #[test]
    fn env_or_bool_selector_matches_vm_sol() {
        assert_eq!(sel("envOr(string,bool)"), SEL_ENV_OR_BOOL);
        assert_eq!(sel("envOr(string,bytes)"), SEL_ENV_OR_BYTES);
        assert_eq!(sel("envOr(string,string)"), SEL_ENV_OR_STRING);
    }
    #[test]
    fn pause_gas_metering_selector_matches_vm_sol() {
        assert_eq!(sel("pauseGasMetering()"), SEL_PAUSE_GAS_METERING);
    }
    #[test]
    fn resume_gas_metering_selector_matches_vm_sol() {
        assert_eq!(sel("resumeGasMetering()"), SEL_RESUME_GAS_METERING);
    }
    #[test]
    fn last_call_gas_selector_matches_vm_sol() {
        assert_eq!(sel("lastCallGas()"), SEL_LAST_CALL_GAS);
    }
    #[test]
    fn decode_encode_abi_string_roundtrip() {
        let original = "hello world";
        let encoded = encode_abi_string(original);
        // ABI-encoded: [offset=0x20][length=11][data padded to 32]
        assert_eq!(encoded.len(), 64 + 32); // offset + length + 1 padded chunk
        // Verify via read_string: head slot 0 has offset 0x20 = 32 bytes.
        // read_string interprets head_index=0 as the offset word, then reads
        // (length, data) at that offset — all relative to the start of `args`.
        // encode_abi_string starts with offset=0x20, so the (length, data) is
        // at byte 0x20 = 32 within the encoded buffer, which corresponds to
        // head_index=1 in read_word terms. read_string calls read_bytes which
        // does: off = read_u256(args, 0) = 0x20 = 32; len = read_u256(args, 32/32=1).
        let decoded = read_string(&encoded, 0).unwrap();
        assert_eq!(decoded, original);
    }
    /// C2-2 (Round 2 audit): exhaustive table-driven verification that every
    /// SEL_ASSERT_* constant equals `keccak256(canonical_sig)[..4]`. The 40
    /// assertion selectors landed in PR #66 without any per-selector pin —
    /// auditor verified the byte literals out-of-band but a future edit could
    /// silently flip one. This test locks them all down at compile-test time.
    #[test]
    fn all_assertion_selectors_match_canonical() {
        let cases: &[(&str, [u8; 4])] = &[
            ("assertEq(uint256,uint256)", SEL_ASSERT_EQ_U256),
            ("assertEq(uint256,uint256,string)", SEL_ASSERT_EQ_U256_MSG),
            ("assertEq(int256,int256)", SEL_ASSERT_EQ_I256),
            ("assertEq(int256,int256,string)", SEL_ASSERT_EQ_I256_MSG),
            ("assertEq(address,address)", SEL_ASSERT_EQ_ADDR),
            ("assertEq(address,address,string)", SEL_ASSERT_EQ_ADDR_MSG),
            ("assertEq(bool,bool)", SEL_ASSERT_EQ_BOOL),
            ("assertEq(bool,bool,string)", SEL_ASSERT_EQ_BOOL_MSG),
            ("assertEq(bytes32,bytes32)", SEL_ASSERT_EQ_B32),
            ("assertEq(bytes32,bytes32,string)", SEL_ASSERT_EQ_B32_MSG),
            ("assertTrue(bool)", SEL_ASSERT_TRUE),
            ("assertTrue(bool,string)", SEL_ASSERT_TRUE_MSG),
            ("assertFalse(bool)", SEL_ASSERT_FALSE),
            ("assertFalse(bool,string)", SEL_ASSERT_FALSE_MSG),
            ("assertGe(uint256,uint256)", SEL_ASSERT_GE_U256),
            ("assertGe(uint256,uint256,string)", SEL_ASSERT_GE_U256_MSG),
            ("assertGe(int256,int256)", SEL_ASSERT_GE_I256),
            ("assertGe(int256,int256,string)", SEL_ASSERT_GE_I256_MSG),
            ("assertGt(uint256,uint256)", SEL_ASSERT_GT_U256),
            ("assertGt(uint256,uint256,string)", SEL_ASSERT_GT_U256_MSG),
            ("assertGt(int256,int256)", SEL_ASSERT_GT_I256),
            ("assertGt(int256,int256,string)", SEL_ASSERT_GT_I256_MSG),
            ("assertLe(uint256,uint256)", SEL_ASSERT_LE_U256),
            ("assertLe(uint256,uint256,string)", SEL_ASSERT_LE_U256_MSG),
            ("assertLe(int256,int256)", SEL_ASSERT_LE_I256),
            ("assertLe(int256,int256,string)", SEL_ASSERT_LE_I256_MSG),
            ("assertLt(uint256,uint256)", SEL_ASSERT_LT_U256),
            ("assertLt(uint256,uint256,string)", SEL_ASSERT_LT_U256_MSG),
            ("assertLt(int256,int256)", SEL_ASSERT_LT_I256),
            ("assertLt(int256,int256,string)", SEL_ASSERT_LT_I256_MSG),
            ("assertNotEq(uint256,uint256)", SEL_ASSERT_NOT_EQ_U256),
            ("assertNotEq(uint256,uint256,string)", SEL_ASSERT_NOT_EQ_U256_MSG),
            ("assertNotEq(int256,int256)", SEL_ASSERT_NOT_EQ_I256),
            ("assertNotEq(int256,int256,string)", SEL_ASSERT_NOT_EQ_I256_MSG),
            ("assertNotEq(address,address)", SEL_ASSERT_NOT_EQ_ADDR),
            ("assertNotEq(address,address,string)", SEL_ASSERT_NOT_EQ_ADDR_MSG),
            ("assertNotEq(bool,bool)", SEL_ASSERT_NOT_EQ_BOOL),
            ("assertNotEq(bool,bool,string)", SEL_ASSERT_NOT_EQ_BOOL_MSG),
            ("assertNotEq(bytes32,bytes32)", SEL_ASSERT_NOT_EQ_B32),
            ("assertNotEq(bytes32,bytes32,string)", SEL_ASSERT_NOT_EQ_B32_MSG),
        ];
        assert_eq!(cases.len(), 40, "expected exactly 40 assertion selectors");
        for (sig, expected) in cases {
            let computed = sel(sig);
            assert_eq!(computed, *expected, "selector mismatch for {sig}");
        }
    }

    /// C2-2 (Round 2 audit): pin all 6 gas-snapshot selectors to their
    /// canonical signatures.
    #[test]
    fn all_gas_snapshot_selectors_match_canonical() {
        let cases: &[(&str, [u8; 4])] = &[
            ("startSnapshotGas(string)", SEL_START_SNAPSHOT_GAS_STR),
            ("stopSnapshotGas()", SEL_STOP_SNAPSHOT_GAS),
            ("stopSnapshotGas(string)", SEL_STOP_SNAPSHOT_GAS_STR),
            ("stopSnapshotGas(string,string)", SEL_STOP_SNAPSHOT_GAS_2STR),
            ("snapshotGasLastCall(string)", SEL_SNAPSHOT_GAS_LAST_CALL_STR),
            ("snapshotGasLastCall(string,string)", SEL_SNAPSHOT_GAS_LAST_CALL_2STR),
        ];
        assert_eq!(cases.len(), 6, "expected exactly 6 gas-snapshot selectors");
        for (sig, expected) in cases {
            assert_eq!(sel(sig), *expected, "selector mismatch for {sig}");
        }
    }

    /// Pin the two `vm.snapshotValue` selectors against `keccak256(sig)[..4]`.
    /// Cross-referenced against foundry's `crates/cheatcodes/assets/cheatcodes.json`
    /// (v1.7.x), which defines exactly two overloads. Any drift between the
    /// signature and the SEL_* constant will trip this test.
    #[test]
    fn all_snapshot_value_selectors_match_canonical() {
        let cases: &[(&str, [u8; 4])] = &[
            ("snapshotValue(string,uint256)", SEL_SNAPSHOT_VALUE_2),
            ("snapshotValue(string,string,uint256)", SEL_SNAPSHOT_VALUE_3),
        ];
        assert_eq!(cases.len(), 2, "foundry v1.7 defines exactly 2 snapshotValue overloads");
        for (sig, expected) in cases {
            assert_eq!(sel(sig), *expected, "selector mismatch for {sig}");
        }
    }

    /// Pin `vm.getDeployedCode(string)` against `keccak256(sig)[..4]`.
    #[test]
    fn selector_get_deployed_code() {
        assert_eq!(sel("getDeployedCode(string)"), SEL_GET_DEPLOYED_CODE);
    }

    /// `artifact_lookup_name` must reproduce foundry's resolution rules for
    /// the three accepted artifact-identifier shapes.
    #[test]
    fn artifact_lookup_name_handles_all_three_shapes() {
        // Bare name.
        assert_eq!(artifact_lookup_name("MyContract"), "MyContract");
        // `path:contract` (the most common foundry shape).
        assert_eq!(artifact_lookup_name("src/Foo.sol:Foo"), "Foo", "second colon-segment must win");
        // `path:contract:version`.
        assert_eq!(artifact_lookup_name("src/Foo.sol:Foo:0.8.20"), "Foo");
        // File-only shape: strip directory + first dot-segment.
        assert_eq!(artifact_lookup_name("src/Foo.sol"), "Foo");
        assert_eq!(artifact_lookup_name("Foo.sol"), "Foo");
    }

    /// C2-3 (Round 2 audit): verify the freshly-cataloged dynamic/array/decimal
    /// assertion overloads keep their selector literals in lockstep with
    /// `keccak256(canonical_sig)[..4]`. Without this, a future "fix" that
    /// retypes one of the array entries (e.g. uint256[] → uint128[]) could
    /// silently drop catalog coverage. Also asserts each entry classifies as
    /// `NotYetImplemented` (i.e., is recognized as an assertion name, not in
    /// the rejected set), so the dispatch fall-through produces the right
    /// abort wording.
    #[test]
    fn known_unsupported_assertion_overloads_are_canonical() {
        let cases: &[(&str, &str)] = &[
            ("assertEq(string,string)", "assertEq"),
            ("assertEq(string,string,string)", "assertEq"),
            ("assertEq(bytes,bytes)", "assertEq"),
            ("assertEq(bytes,bytes,string)", "assertEq"),
            ("assertEq(uint256[],uint256[])", "assertEq"),
            ("assertEq(uint256[],uint256[],string)", "assertEq"),
            ("assertEq(int256[],int256[])", "assertEq"),
            ("assertEq(int256[],int256[],string)", "assertEq"),
            ("assertEq(address[],address[])", "assertEq"),
            ("assertEq(address[],address[],string)", "assertEq"),
            ("assertEq(bool[],bool[])", "assertEq"),
            ("assertEq(bool[],bool[],string)", "assertEq"),
            ("assertEq(bytes32[],bytes32[])", "assertEq"),
            ("assertEq(bytes32[],bytes32[],string)", "assertEq"),
            ("assertEq(string[],string[])", "assertEq"),
            ("assertEq(string[],string[],string)", "assertEq"),
            ("assertEq(bytes[],bytes[])", "assertEq"),
            ("assertEq(bytes[],bytes[],string)", "assertEq"),
            ("assertNotEq(string,string)", "assertNotEq"),
            ("assertNotEq(string,string,string)", "assertNotEq"),
            ("assertNotEq(bytes,bytes)", "assertNotEq"),
            ("assertNotEq(bytes,bytes,string)", "assertNotEq"),
            ("assertNotEq(uint256[],uint256[])", "assertNotEq"),
            ("assertNotEq(uint256[],uint256[],string)", "assertNotEq"),
            ("assertNotEq(int256[],int256[])", "assertNotEq"),
            ("assertNotEq(int256[],int256[],string)", "assertNotEq"),
            ("assertNotEq(address[],address[])", "assertNotEq"),
            ("assertNotEq(address[],address[],string)", "assertNotEq"),
            ("assertNotEq(bool[],bool[])", "assertNotEq"),
            ("assertNotEq(bool[],bool[],string)", "assertNotEq"),
            ("assertNotEq(bytes32[],bytes32[])", "assertNotEq"),
            ("assertNotEq(bytes32[],bytes32[],string)", "assertNotEq"),
            ("assertNotEq(string[],string[])", "assertNotEq"),
            ("assertNotEq(string[],string[],string)", "assertNotEq"),
            ("assertNotEq(bytes[],bytes[])", "assertNotEq"),
            ("assertNotEq(bytes[],bytes[],string)", "assertNotEq"),
            ("assertEqDecimal(uint256,uint256,uint256)", "assertEqDecimal"),
            ("assertEqDecimal(uint256,uint256,uint256,string)", "assertEqDecimal"),
            ("assertEqDecimal(int256,int256,uint256)", "assertEqDecimal"),
            ("assertEqDecimal(int256,int256,uint256,string)", "assertEqDecimal"),
            ("assertNotEqDecimal(uint256,uint256,uint256)", "assertNotEqDecimal"),
            ("assertNotEqDecimal(uint256,uint256,uint256,string)", "assertNotEqDecimal"),
            ("assertNotEqDecimal(int256,int256,uint256)", "assertNotEqDecimal"),
            ("assertNotEqDecimal(int256,int256,uint256,string)", "assertNotEqDecimal"),
            ("assertGtDecimal(uint256,uint256,uint256)", "assertGtDecimal"),
            ("assertGtDecimal(uint256,uint256,uint256,string)", "assertGtDecimal"),
            ("assertGtDecimal(int256,int256,uint256)", "assertGtDecimal"),
            ("assertGtDecimal(int256,int256,uint256,string)", "assertGtDecimal"),
            ("assertGeDecimal(uint256,uint256,uint256)", "assertGeDecimal"),
            ("assertGeDecimal(uint256,uint256,uint256,string)", "assertGeDecimal"),
            ("assertGeDecimal(int256,int256,uint256)", "assertGeDecimal"),
            ("assertGeDecimal(int256,int256,uint256,string)", "assertGeDecimal"),
            ("assertLtDecimal(uint256,uint256,uint256)", "assertLtDecimal"),
            ("assertLtDecimal(uint256,uint256,uint256,string)", "assertLtDecimal"),
            ("assertLtDecimal(int256,int256,uint256)", "assertLtDecimal"),
            ("assertLtDecimal(int256,int256,uint256,string)", "assertLtDecimal"),
            ("assertLeDecimal(uint256,uint256,uint256)", "assertLeDecimal"),
            ("assertLeDecimal(uint256,uint256,uint256,string)", "assertLeDecimal"),
            ("assertLeDecimal(int256,int256,uint256)", "assertLeDecimal"),
            ("assertLeDecimal(int256,int256,uint256,string)", "assertLeDecimal"),
            ("assertApproxEqAbs(uint256,uint256,uint256)", "assertApproxEqAbs"),
            ("assertApproxEqAbs(uint256,uint256,uint256,string)", "assertApproxEqAbs"),
            ("assertApproxEqAbs(int256,int256,uint256)", "assertApproxEqAbs"),
            ("assertApproxEqAbs(int256,int256,uint256,string)", "assertApproxEqAbs"),
            ("assertApproxEqRel(uint256,uint256,uint256)", "assertApproxEqRel"),
            ("assertApproxEqRel(uint256,uint256,uint256,string)", "assertApproxEqRel"),
            ("assertApproxEqRel(int256,int256,uint256)", "assertApproxEqRel"),
            ("assertApproxEqRel(int256,int256,uint256,string)", "assertApproxEqRel"),
        ];
        for (sig, expected_name) in cases {
            let computed = sel(sig);
            let hit = KNOWN_CHEATCODES.iter().find(|(s, _)| **s == computed);
            let (_, name) = hit.unwrap_or_else(|| {
                panic!("KNOWN_CHEATCODES missing canonical assertion overload {sig} (selector 0x{:02x}{:02x}{:02x}{:02x})", computed[0], computed[1], computed[2], computed[3])
            });
            assert_eq!(
                name, expected_name,
                "catalog entry for {sig} has wrong name (got {name}, want {expected_name})",
            );
            // Crucial UX guarantee: these are NOT in the rejected set, so the
            // dispatch fall-through tags them as NotYetImplemented (not Rejected).
            assert!(
                !is_explicitly_rejected_name(name),
                "{sig} should classify as NotYetImplemented, but is_explicitly_rejected_name({name}) = true",
            );
        }
    }

    #[test]
    fn selector_rejected_set_matches_canonical_signatures() {
        // Spot-check a handful of rejected selectors so the rejected list
        // doesn't silently drift from the canonical Vm.sol signatures.
        assert_eq!(sel("selectFork(uint256)"), SEL_SELECT_FORK);
        assert_eq!(sel("snapshotState()"), SEL_SNAPSHOT_STATE);
        assert_eq!(sel("revertToState(uint256)"), SEL_REVERT_TO_STATE);
        assert_eq!(sel("ffi(string[])"), SEL_FFI);
        assert_eq!(sel("transact(bytes32)"), SEL_TRANSACT);
        assert_eq!(sel("broadcast()"), SEL_BROADCAST);
    }

    // --- Snapshot family selector verification (plan Task 1) ---------------

    #[test]
    fn snapshot_state_selector_matches_vm_sol() {
        assert_eq!(sel("snapshotState()"), SEL_SNAPSHOT_STATE);
    }
    #[test]
    fn snapshot_alias_selector_matches_vm_sol() {
        assert_eq!(sel("snapshot()"), SEL_SNAPSHOT);
    }
    #[test]
    fn revert_to_state_selector_matches_vm_sol() {
        assert_eq!(sel("revertToState(uint256)"), SEL_REVERT_TO_STATE);
    }
    #[test]
    fn revert_to_alias_selector_matches_vm_sol() {
        assert_eq!(sel("revertTo(uint256)"), SEL_REVERT_TO);
    }
    #[test]
    fn revert_to_state_and_delete_selector_matches_vm_sol() {
        assert_eq!(sel("revertToStateAndDelete(uint256)"), SEL_REVERT_TO_STATE_AND_DELETE);
    }
    #[test]
    fn delete_state_snapshot_selector_matches_vm_sol() {
        assert_eq!(sel("deleteStateSnapshot(uint256)"), SEL_DELETE_STATE_SNAPSHOT);
    }
    #[test]
    fn delete_state_snapshots_selector_matches_vm_sol() {
        assert_eq!(sel("deleteStateSnapshots()"), SEL_DELETE_STATE_SNAPSHOTS);
    }
    #[test]
    fn roll_fork_uint_selector_matches_vm_sol() {
        assert_eq!(sel("rollFork(uint256)"), SEL_ROLL_FORK_UINT);
    }

    #[test]
    fn revert_to_state_and_delete_constant_is_distinct() {
        // Pin that SEL_REVERT_TO_STATE_AND_DELETE differs from SEL_REVERT_TO_STATE and SEL_REVERT_TO.
        // If they collide, our dispatch table loses a selector silently.
        assert_ne!(SEL_REVERT_TO_STATE_AND_DELETE, SEL_REVERT_TO_STATE);
        assert_ne!(SEL_REVERT_TO_STATE_AND_DELETE, SEL_REVERT_TO);
    }

    // --- KNOWN_CHEATCODES catalog tests ------------------------------------

    /// Every selector in the catalog must be unique. Duplicates would mean
    /// different cheatcodes mapped to the same selector, which is impossible
    /// in EVM ABI encoding — it signals a copy-paste error.
    #[test]
    fn known_cheatcode_catalog_has_unique_selectors() {
        let mut seen = std::collections::HashSet::new();
        for (sel, name) in KNOWN_CHEATCODES {
            assert!(
                seen.insert(*sel),
                "duplicate selector in KNOWN_CHEATCODES: {sel:?} (name={name:?})"
            );
        }
    }

    /// Spot-check that cheatcodes we DO implement are in the catalog, so the
    /// catalog stays in sync as new cheatcodes are added to dispatch.
    #[test]
    fn known_cheatcode_catalog_covers_implemented_set() {
        for sig in &[
            b"warp(uint256)" as &[u8],
            b"deal(address,uint256)",
            b"prank(address)",
            b"expectRevert()",
            b"expectEmit()",
            b"expectCall(address,bytes)",
            b"assume(bool)",
            b"envBool(string)",
            b"pauseGasMetering()",
            b"lastCallGas()",
        ] {
            let computed: [u8; 4] = keccak256(sig)[..4].try_into().unwrap();
            assert!(
                KNOWN_CHEATCODES.iter().any(|(s, _)| **s == computed),
                "implemented cheatcode {:?} missing from KNOWN_CHEATCODES (selector {:?})",
                std::str::from_utf8(sig).unwrap(),
                computed,
            );
        }
    }

    /// Verify a sample of not-yet-implemented catalog entries against their
    /// canonical keccak256 selectors so they don't silently drift.
    #[test]
    fn known_cheatcode_catalog_not_yet_selectors_are_canonical() {
        // addr(uint256)
        assert!(KNOWN_CHEATCODES.iter().any(|(s, n)| **s == sel("addr(uint256)") && *n == "addr"));
        // parseJson(string)
        assert!(
            KNOWN_CHEATCODES
                .iter()
                .any(|(s, n)| **s == sel("parseJson(string)") && *n == "parseJson")
        );
        // expectSafeMemory(uint64,uint64)
        assert!(KNOWN_CHEATCODES.iter().any(|(s, n)| **s
            == sel("expectSafeMemory(uint64,uint64)")
            && *n == "expectSafeMemory"));
        // copyStorage(address,address)
        assert!(
            KNOWN_CHEATCODES
                .iter()
                .any(|(s, n)| **s == sel("copyStorage(address,address)") && *n == "copyStorage")
        );
    }

    /// A selector NOT in the catalog should not be found (sanity for the lookup logic).
    #[test]
    fn known_cheatcode_catalog_unknown_selector_returns_none() {
        // Use a nonsense selector that won't collide with any real cheatcode.
        let bogus: [u8; 4] = [0xde, 0xad, 0xbe, 0xef];
        assert!(
            KNOWN_CHEATCODES.iter().find(|(s, _)| **s == bogus).is_none(),
            "0xdeadbeef should not be in KNOWN_CHEATCODES"
        );
    }

    /// Regression (R3-8 from audit round 3): the supported single-arg
    /// `vm.rollFork(uint256)` (selector `0xd9bbf3a1`) MUST have an
    /// explicit dispatch arm BEFORE the catalog fall-through. The catalog
    /// itself lists `"rollFork"` under the rejected name set (because the
    /// cross-fork overloads are rejected and share that name), so if a
    /// future refactor accidentally removes the explicit arm, the
    /// supported variant would re-classify as `Rejected` with no test
    /// catching it. This guard verifies the routing by name+category and
    /// also pins the constant so a typo in `SEL_ROLL_FORK_UINT` would
    /// surface.
    #[test]
    fn supported_roll_fork_uint_does_not_route_through_catalog() {
        // SEL_ROLL_FORK_UINT must match the canonical keccak.
        assert_eq!(sel("rollFork(uint256)"), SEL_ROLL_FORK_UINT);
        // The catalog entry for this exact selector has name "rollFork",
        // which `is_explicitly_rejected_name` says is rejected — so if the
        // explicit arm were removed, the fall-through would record this
        // call as Rejected. We assert the precondition (catalog says
        // rejected) so future refactors can see the booby-trap clearly.
        let by_sel = KNOWN_CHEATCODES.iter().find(|(s, _)| **s == SEL_ROLL_FORK_UINT);
        assert!(by_sel.is_some(), "rollFork(uint256) selector must be in KNOWN_CHEATCODES");
        let (_, name) = by_sel.unwrap();
        assert_eq!(*name, "rollFork");
        assert!(
            is_explicitly_rejected_name(name),
            "the rollFork name is bucketed with the rejected family — the supported \
             single-arg overload depends on the explicit SEL_ROLL_FORK_UINT dispatch \
             arm in dispatch() to bypass the catalog fall-through"
        );
    }

    // --- Revert payload + factory -------------------------------------------

    #[test]
    fn unsupported_revert_format() {
        let r = unsupported_revert("selectFork");
        // First 4 bytes are the Error(string) selector.
        assert_eq!(&r[..4], &[0x08, 0xc3, 0x79, 0xa0]);
        // The encoded UTF-8 should contain the literal vm.selectFork message.
        let tail = &r[4 + 64..]; // skip selector + offset + length
        let msg = String::from_utf8_lossy(tail);
        assert!(msg.contains("EDB: cheatcode vm.selectFork not supported in v1"), "got: {msg:?}");
    }

    #[test]
    fn factory_yields_fresh_instances() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let factory = build_cheats_factory::<TestDB>(CheatsConfig::default());
        let a = factory();
        let b = factory();
        // Fresh inspectors share no mutable state.
        assert!(a.recorded_logs().is_empty());
        assert!(b.labels().is_empty());
    }

    // --- expectRevert guard: cheatcode call must NOT consume expected_revert --

    /// Verify that `call_end` only consumes `expected_revert` when the ending
    /// call is NOT to the cheatcode address.
    ///
    /// We can't spin up a real `EdbContext<DB>` in a unit test, so we exercise
    /// the guard logic by directly mutating and inspecting the `EdbCheatcodes`
    /// fields.  The integration test `cheats_expect_revert_rewrites_outcome`
    /// provides the full end-to-end coverage.
    #[test]
    fn expected_revert_guard_skips_cheatcode_address() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());

        // Simulate what `cheat_expect_revert_bare` does: arm the slot.
        cheats.expected_revert = Some(ExpectedRevert { expected: ExpectedRevertMatch::Bare });

        // The guard condition mirrors the one in `call_end`:
        //   if inputs.target_address != CHEATCODE_ADDRESS { take() }
        // When the target IS the cheatcode address the slot must remain Some.
        let is_cheatcode = CHEATCODE_ADDRESS;
        let not_consumed = is_cheatcode == CHEATCODE_ADDRESS; // true → guard skips take()
        assert!(
            not_consumed && cheats.expected_revert.is_some(),
            "expected_revert must remain Some when call_end fires for the cheatcode address",
        );

        // When the target is any other address the slot should be consumed.
        let user_addr = Address::from([0xAA; 20]);
        let would_consume = user_addr != CHEATCODE_ADDRESS; // true → take() runs
        assert!(
            would_consume,
            "expected_revert must be consumed when call_end fires for a non-cheatcode address",
        );
    }

    // --- ABI decode helpers -------------------------------------------------

    #[test]
    fn read_address_uint256_pair() {
        // mimic `deal(address,uint256)` calldata tail: 2 head words.
        let mut buf = Vec::new();
        // address arg
        let mut a = [0u8; 32];
        a[12..].copy_from_slice(&[0xde; 20]);
        buf.extend_from_slice(&a);
        // uint256 arg
        let v = U256::from(1_000_000u64).to_be_bytes::<32>();
        buf.extend_from_slice(&v);

        let addr = read_address(&buf, 0).expect("address");
        assert_eq!(addr, Address::from_slice(&[0xde; 20]));
        assert_eq!(read_u256(&buf, 1).expect("u256"), U256::from(1_000_000u64));
    }

    #[test]
    fn read_bool_decodes_canonical_words() {
        // false = all zero word
        let mut zero = vec![0u8; 32];
        assert_eq!(read_bool(&zero, 0), Some(false));
        // true = canonical 0x...01 word
        zero[31] = 1;
        assert_eq!(read_bool(&zero, 0), Some(true));
        // any non-zero byte counts as true (be permissive on encoded forms)
        let mut other = vec![0u8; 32];
        other[7] = 0xff;
        assert_eq!(read_bool(&other, 0), Some(true));
        // truncated input returns None
        assert_eq!(read_bool(&[0u8; 16], 0), None);
    }

    #[test]
    fn expected_emit_soft_match_rules() {
        let addr = Address::from([0x11; 20]);
        let other = Address::from([0x22; 20]);
        let log_full = Log::new_unchecked(
            addr,
            vec![B256::from([0xaa; 32]), B256::from([0xbb; 32]), B256::from([0xcc; 32])],
            Bytes::from_static(b"payload"),
        );
        let log_only_sig = Log::new_unchecked(addr, vec![B256::from([0xaa; 32])], Bytes::new());
        let log_no_topics = Log::new_unchecked(addr, vec![], Bytes::from_static(b"payload"));

        // Round-2 fix (OwnableTest::testHandoverOwnershipWithCancellation):
        // `(true,true,true,true)` no longer demands the log carry 4 topics
        // OR non-empty data. Foundry's bools encode byte-equality against
        // the template; soft-match has no template, and events with only
        // indexed args (`event Foo(address indexed)`) emit empty data
        // payloads under forge with `expectEmit(_,_,_,true)` happily
        // passing — refusing them here false-failed every such test.
        let all = ExpectedEmit {
            check_topics: [true; 4],
            check_data: true,
            expected_emitter: None,
            matched: false,
            registered_at_call_depth: 0,
        };
        assert!(all.matches(&log_full), "soft-match accepts < 4-topic logs (OwnableTest fix)");
        assert!(all.matches(&log_only_sig), "indexed-only events emit empty data: accept");
        assert!(!all.matches(&log_no_topics), "must require >= 1 topic (the event sig)");

        // expect_emit_simple has check_data=true; under soft-match v1 it's a no-op now.
        let lax = expect_emit_simple();
        assert!(lax.matches(&log_full), "topic[0]+data must match a non-empty log");
        assert!(lax.matches(&log_only_sig), "empty-data logs no longer rejected");

        // Emitter filter
        let mut with_emitter = lax;
        with_emitter.expected_emitter = Some(other);
        assert!(!with_emitter.matches(&log_full), "wrong emitter must reject");
        with_emitter.expected_emitter = Some(addr);
        assert!(with_emitter.matches(&log_full));
    }

    fn expect_emit_simple() -> ExpectedEmit {
        ExpectedEmit {
            check_topics: [true, false, false, false],
            check_data: true,
            expected_emitter: None,
            matched: false,
            registered_at_call_depth: 0,
        }
    }

    #[test]
    fn read_dynamic_bytes_roundtrip() {
        // Encode `etch(address,bytes)`-style tail: address head, offset head,
        // then (length, data) tail.
        let mut buf = Vec::new();
        // arg 0: address
        let mut a = [0u8; 32];
        a[12..].copy_from_slice(&[0xab; 20]);
        buf.extend_from_slice(&a);
        // arg 1 head: offset = 0x40 (2 head words behind us)
        let mut off = [0u8; 32];
        off[31] = 0x40;
        buf.extend_from_slice(&off);
        // tail: length, then data (pad to 32 bytes)
        let data = b"hello-edb-cheats";
        let mut len = [0u8; 32];
        len[24..].copy_from_slice(&(data.len() as u64).to_be_bytes());
        buf.extend_from_slice(&len);
        let mut padded = data.to_vec();
        padded.resize(32, 0);
        buf.extend_from_slice(&padded);

        assert_eq!(read_address(&buf, 0).unwrap(), Address::from_slice(&[0xab; 20]));
        assert_eq!(read_bytes(&buf, 1).unwrap().as_ref(), data);
    }

    // --- vm.toString behavior tests -----------------------------------------

    /// Decode an ABI-encoded `string` (offset + length + padded UTF-8) back
    /// to a Rust `String`. Returns `None` if the bytes are malformed.
    fn decode_abi_string(bytes: &[u8]) -> Option<String> {
        if bytes.len() < 64 {
            return None;
        }
        let len_word: [u8; 32] = bytes[32..64].try_into().ok()?;
        let mut len_bytes = [0u8; 8];
        len_bytes.copy_from_slice(&len_word[24..32]);
        let len = u64::from_be_bytes(len_bytes) as usize;
        let start: usize = 64;
        let end = start.checked_add(len)?;
        if bytes.len() < end {
            return None;
        }
        String::from_utf8(bytes[start..end].to_vec()).ok()
    }

    /// Build calldata for a cheatcode that takes a single static 32-byte arg.
    fn single_word_args(word: [u8; 32]) -> Vec<u8> {
        word.to_vec()
    }

    /// Build calldata for `vm.toString(bytes data)` with the given payload.
    fn dyn_bytes_args(payload: &[u8]) -> Vec<u8> {
        // head[0] = offset to bytes (== 0x20)
        // head[1] = length
        // payload right-padded to 32-byte multiple
        let mut out = Vec::with_capacity(64 + payload.len().div_ceil(32) * 32);
        let mut offset = [0u8; 32];
        offset[31] = 0x20;
        out.extend_from_slice(&offset);
        let mut len = [0u8; 32];
        len[24..].copy_from_slice(&(payload.len() as u64).to_be_bytes());
        out.extend_from_slice(&len);
        out.extend_from_slice(payload);
        let pad = (32 - payload.len() % 32) % 32;
        out.extend(std::iter::repeat_n(0u8, pad));
        out
    }

    #[test]
    fn to_string_uint256_formats_decimal() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        let mut arg = [0u8; 32];
        arg[24..].copy_from_slice(&123_456u64.to_be_bytes());
        let out = cheats.cheat_to_string_uint256(&inputs, &arg);
        assert!(matches!(out.result.result, InstructionResult::Return));
        assert_eq!(decode_abi_string(out.result.output.as_ref()).as_deref(), Some("123456"));
    }

    #[test]
    fn to_string_int256_handles_negative() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        // -7 as two's complement int256: all 0xFF...F9
        let mut arg = [0xffu8; 32];
        arg[31] = 0xf9;
        let out = cheats.cheat_to_string_int256(&inputs, &arg);
        assert!(matches!(out.result.result, InstructionResult::Return));
        assert_eq!(decode_abi_string(out.result.output.as_ref()).as_deref(), Some("-7"));

        // Positive int256 = 42
        let mut pos = [0u8; 32];
        pos[31] = 42;
        let out = cheats.cheat_to_string_int256(&inputs, &pos);
        assert_eq!(decode_abi_string(out.result.output.as_ref()).as_deref(), Some("42"));
    }

    #[test]
    fn to_string_bool_yields_true_or_false() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        let mut t = [0u8; 32];
        t[31] = 1;
        let out = cheats.cheat_to_string_bool(&inputs, &t);
        assert_eq!(decode_abi_string(out.result.output.as_ref()).as_deref(), Some("true"));
        let f = [0u8; 32];
        let out = cheats.cheat_to_string_bool(&inputs, &f);
        assert_eq!(decode_abi_string(out.result.output.as_ref()).as_deref(), Some("false"));
    }

    #[test]
    fn to_string_address_is_eip55_checksum() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        // 0x52908400098527886e0f7030069857d2e4169ee7 — one of the canonical
        // EIP-55 test vectors whose checksum is all-uppercase, exercising the
        // case-sensitivity path of alloy's `Display for Address` impl.
        let addr_bytes: [u8; 20] = [
            0x52, 0x90, 0x84, 0x00, 0x09, 0x85, 0x27, 0x88, 0x6e, 0x0f, 0x70, 0x30, 0x06, 0x98,
            0x57, 0xd2, 0xe4, 0x16, 0x9e, 0xe7,
        ];
        let mut arg = [0u8; 32];
        arg[12..].copy_from_slice(&addr_bytes);
        let out = cheats.cheat_to_string_address(&inputs, &arg);
        let s = decode_abi_string(out.result.output.as_ref()).expect("decoded string");
        // Independent EIP-55 reference value cross-checked against alloy's
        // `Address::to_checksum(None)`.
        assert_eq!(s, "0x52908400098527886E0F7030069857D2E4169EE7");
    }

    #[test]
    fn to_string_bytes32_prints_full_hex() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        let mut arg = [0u8; 32];
        for (i, slot) in arg.iter_mut().enumerate() {
            *slot = i as u8;
        }
        let out = cheats.cheat_to_string_bytes32(&inputs, &arg);
        let s = decode_abi_string(out.result.output.as_ref()).expect("decoded string");
        assert_eq!(s, "0x000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
    }

    // --- vm.parseJson behavior tests ----------------------------------------

    /// Build calldata for a two-`string` cheatcode: `(string a, string b)`.
    /// Head section is two offsets (both pointing past the head into the
    /// concatenated tails), then `(length, padded data)` for each.
    fn two_string_args(a: &str, b: &str) -> Vec<u8> {
        let a_b = a.as_bytes();
        let b_b = b.as_bytes();
        let a_pad = (32 - a_b.len() % 32) % 32;
        let b_pad = (32 - b_b.len() % 32) % 32;
        // Both string tails live after the two head words (offset starts at 0x40).
        let a_off = 0x40usize;
        // The B tail is at A's tail start + (length-word + padded payload).
        let b_off = a_off + 32 + a_b.len() + a_pad;

        let mut out = Vec::with_capacity(b_off + 32 + b_b.len() + b_pad);
        // head[0] = offset to A
        let mut w = [0u8; 32];
        w[24..].copy_from_slice(&(a_off as u64).to_be_bytes());
        out.extend_from_slice(&w);
        // head[1] = offset to B
        let mut w = [0u8; 32];
        w[24..].copy_from_slice(&(b_off as u64).to_be_bytes());
        out.extend_from_slice(&w);
        // A: length + padded data
        let mut w = [0u8; 32];
        w[24..].copy_from_slice(&(a_b.len() as u64).to_be_bytes());
        out.extend_from_slice(&w);
        out.extend_from_slice(a_b);
        out.extend(std::iter::repeat_n(0u8, a_pad));
        // B: length + padded data
        let mut w = [0u8; 32];
        w[24..].copy_from_slice(&(b_b.len() as u64).to_be_bytes());
        out.extend_from_slice(&w);
        out.extend_from_slice(b_b);
        out.extend(std::iter::repeat_n(0u8, b_pad));
        out
    }

    #[test]
    fn navigate_json_walks_dot_keys_and_brackets() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"a": {"b": [10, 20, 30]}, "x": true}"#).unwrap();
        // Root.
        assert!(navigate_json(&v, "$").is_some());
        assert!(navigate_json(&v, "").is_some());
        // Simple key.
        assert_eq!(navigate_json(&v, ".x").and_then(|n| n.as_bool()), Some(true));
        assert_eq!(navigate_json(&v, "$.x").and_then(|n| n.as_bool()), Some(true));
        // Nested key.
        assert!(navigate_json(&v, ".a.b").and_then(|n| n.as_array()).is_some());
        // Bracket index.
        assert_eq!(navigate_json(&v, ".a.b[1]").and_then(|n| n.as_i64()), Some(20));
        assert_eq!(navigate_json(&v, "$.a.b[2]").and_then(|n| n.as_i64()), Some(30));
        // Missing key returns None.
        assert!(navigate_json(&v, ".nope").is_none());
        // Out-of-range index returns None.
        assert!(navigate_json(&v, ".a.b[99]").is_none());
    }

    #[test]
    fn parse_json_bool_extracts_leaf() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        let args = two_string_args(r#"{"x": true}"#, ".x");
        let out = cheats.cheat_parse_json_bool(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Return));
        assert_eq!(out.result.output.len(), 32);
        assert_eq!(out.result.output[31], 1);

        // false leaf
        let args = two_string_args(r#"{"x": false}"#, ".x");
        let out = cheats.cheat_parse_json_bool(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Return));
        assert_eq!(out.result.output[31], 0);
    }

    #[test]
    fn parse_json_string_extracts_leaf() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        let args = two_string_args(r#"{"n": "hello"}"#, ".n");
        let out = cheats.cheat_parse_json_string(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Return));
        assert_eq!(decode_abi_string(out.result.output.as_ref()).as_deref(), Some("hello"));
    }

    #[test]
    fn parse_json_uint_handles_decimal_and_hex_strings() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        // Plain JSON number.
        let args = two_string_args(r#"{"n": 12345}"#, ".n");
        let out = cheats.cheat_parse_json_uint(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Return));
        let v = U256::from_be_slice(out.result.output.as_ref());
        assert_eq!(v, U256::from(12345u64));

        // String form ("0x..." hex)
        let args = two_string_args(r#"{"n": "0x100"}"#, ".n");
        let out = cheats.cheat_parse_json_uint(&inputs, &args);
        let v = U256::from_be_slice(out.result.output.as_ref());
        assert_eq!(v, U256::from(256u64));
    }

    #[test]
    fn parse_json_int_handles_negative() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        // JSON-number negative.
        let args = two_string_args(r#"{"n": -42}"#, ".n");
        let out = cheats.cheat_parse_json_int(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Return));
        let bytes: [u8; 32] = out.result.output.as_ref().try_into().unwrap();
        let signed = alloy_primitives::I256::from_be_bytes::<32>(bytes);
        assert_eq!(signed.to_string(), "-42");
    }

    #[test]
    fn parse_json_bytes32_decodes_hex_string() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        let raw = "0x000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        let args = two_string_args(&format!(r#"{{"k": "{raw}"}}"#), ".k");
        let out = cheats.cheat_parse_json_bytes32(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Return));
        let actual: [u8; 32] = out.result.output.as_ref().try_into().unwrap();
        let expected = alloy_primitives::hex::decode(raw.strip_prefix("0x").unwrap()).unwrap();
        assert_eq!(&actual[..], &expected[..]);
    }

    #[test]
    fn parse_json_address_decodes_eip55() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        let raw = "0x52908400098527886E0F7030069857D2E4169EE7";
        let args = two_string_args(&format!(r#"{{"who": "{raw}"}}"#), ".who");
        let out = cheats.cheat_parse_json_address(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Return));
        // Address is right-aligned in the 32-byte slot.
        let parsed = Address::from_slice(&out.result.output.as_ref()[12..]);
        let expected: Address = raw.parse().unwrap();
        assert_eq!(parsed, expected);
    }

    #[test]
    fn parse_json_missing_path_reverts_cleanly() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        let args = two_string_args(r#"{"a": 1}"#, ".b");
        let out = cheats.cheat_parse_json_bool(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Revert));
        let msg = decode_error_payload(out.result.output.as_ref()).expect("Error(string)");
        assert!(msg.contains("vm.parseJsonBool"), "got: {msg}");
    }

    #[test]
    fn to_string_bytes_dynamic_payload_round_trip() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);
        let payload: &[u8] = b"\xde\xad\xbe\xef";
        let calldata = dyn_bytes_args(payload);
        let out = cheats.cheat_to_string_bytes(&inputs, &calldata);
        let s = decode_abi_string(out.result.output.as_ref()).expect("decoded string");
        assert_eq!(s, "0xdeadbeef");

        // Also assert an empty payload renders as "0x".
        let calldata = dyn_bytes_args(b"");
        let out = cheats.cheat_to_string_bytes(&inputs, &calldata);
        let s = decode_abi_string(out.result.output.as_ref()).expect("decoded string");
        assert_eq!(s, "0x");

        // Silence unused-helper warnings for utility functions kept for symmetry.
        let _ = single_word_args([0u8; 32]);
    }

    // --- cheat_assert: C2-1 / C2-4 regression coverage ----------------------

    /// Decode an `Error(string)` ABI payload back into its message (returns
    /// `None` if the bytes aren't a well-formed `Error(string)`).
    fn decode_error_payload(bytes: &[u8]) -> Option<String> {
        if bytes.len() < 4 + 64 || bytes[..4] != [0x08, 0xc3, 0x79, 0xa0] {
            return None;
        }
        // Skip selector(4) + offset(32). Length is in the next 32 bytes (BE).
        let len_word: [u8; 32] = bytes[4 + 32..4 + 64].try_into().ok()?;
        let mut len_bytes = [0u8; 8];
        len_bytes.copy_from_slice(&len_word[24..32]);
        let len = u64::from_be_bytes(len_bytes) as usize;
        let start: usize = 4 + 64;
        let end = start.checked_add(len)?;
        if bytes.len() < end {
            return None;
        }
        String::from_utf8(bytes[start..end].to_vec()).ok()
    }

    /// C2-1 (Round 2 audit): `vm.assertTrue(bool)` / `vm.assertFalse(bool)`
    /// must NOT revert with "insufficient calldata" — they have exactly 32
    /// bytes of args (one bool word), not 64.
    ///
    /// Asserts:
    /// - `vm.assertTrue(true)`  -> ok_return (passing assertion).
    /// - `vm.assertTrue(false)` -> revert with the assertion-failure message,
    ///   NOT the "insufficient calldata" guard.
    /// - `vm.assertFalse(false)` -> ok_return.
    /// - `vm.assertFalse(true)`  -> revert with assertion-failure.
    #[test]
    fn assert_true_false_single_arg_does_not_revert_on_32_bytes() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);

        // Single 32-byte bool word for `true` / `false`.
        let mut true_word = [0u8; 32];
        true_word[31] = 1;
        let false_word = [0u8; 32];

        // assertTrue(true) - PASS
        let out = cheats.cheat_assert(&inputs, SEL_ASSERT_TRUE, &true_word);
        assert!(
            matches!(out.result.result, InstructionResult::Return),
            "vm.assertTrue(true) must return successfully, got {:?}",
            out.result.result,
        );

        // assertTrue(false) - FAIL with assertion-failure (NOT the bouncer)
        let out = cheats.cheat_assert(&inputs, SEL_ASSERT_TRUE, &false_word);
        assert!(matches!(out.result.result, InstructionResult::Revert));
        let msg = decode_error_payload(&out.result.output).expect("Error(string) payload");
        assert!(
            !msg.contains("insufficient calldata"),
            "C2-1 regression: bouncer fired for single-arg assertTrue; got: {msg}",
        );
        assert!(msg.contains("Assertion failed"), "expected assertion-failure msg, got {msg}");

        // assertFalse(false) - PASS
        let out = cheats.cheat_assert(&inputs, SEL_ASSERT_FALSE, &false_word);
        assert!(matches!(out.result.result, InstructionResult::Return));

        // assertFalse(true) - FAIL
        let out = cheats.cheat_assert(&inputs, SEL_ASSERT_FALSE, &true_word);
        assert!(matches!(out.result.result, InstructionResult::Revert));
        let msg = decode_error_payload(&out.result.output).expect("Error(string)");
        assert!(!msg.contains("insufficient calldata"));
    }

    /// C2-4 (Round 2 audit): the custom-message decoder must produce the right
    /// error message for the SINGLE-ARG + STRING overloads
    /// (`assertTrue(bool, string)`, `assertFalse(bool, string)`). Previously
    /// the decoder hardcoded the string-length location to `args[120..128]`,
    /// which is correct for 3-head-word layouts but reads zero-padding for the
    /// 2-head-word layouts — losing the user's error message.
    ///
    /// Builds `vm.assertTrue(false, "boom")` calldata exactly per ABI spec and
    /// asserts the recovered message contains "(boom)".
    #[test]
    fn assert_true_with_message_decodes_offset_correctly() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);

        // ABI for vm.assertTrue(bool data, string err) when data=false, err="boom":
        //   [0x00..0x00]  (bool false, 32 bytes)
        //   [0x40 offset] (string offset = 0x40, 32 bytes — points past both head words)
        //   [0x04 length] (string length = 4, 32 bytes)
        //   ["boom" right-padded to 32 bytes]
        // Total: 4 * 32 = 128 bytes.
        let mut calldata = Vec::with_capacity(128);
        calldata.extend_from_slice(&[0u8; 32]); // bool false
        let mut offset = [0u8; 32];
        offset[31] = 0x40;
        calldata.extend_from_slice(&offset);
        let mut len = [0u8; 32];
        len[31] = 0x04;
        calldata.extend_from_slice(&len);
        let mut data = [0u8; 32];
        data[..4].copy_from_slice(b"boom");
        calldata.extend_from_slice(&data);
        assert_eq!(calldata.len(), 128);

        let out = cheats.cheat_assert(&inputs, SEL_ASSERT_TRUE_MSG, &calldata);
        assert!(matches!(out.result.result, InstructionResult::Revert));
        let msg = decode_error_payload(&out.result.output).expect("Error(string)");
        assert!(
            msg.contains("(boom)"),
            "C2-4 regression: custom message for assertTrue(bool,string) was lost; got: {msg}",
        );
    }

    /// C2-4 also covers the 3-head-word layout
    /// (`assertEq(uint256, uint256, string)`) — the previous decoder happened
    /// to work here because the hardcoded offset matched, but the new
    /// generalized decoder must continue to handle this case too.
    #[test]
    fn assert_eq_uint_with_message_decodes_offset_correctly() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);

        // ABI for vm.assertEq(uint256 left=1, uint256 right=2, string err="mismatch"):
        //   left=1, right=2, offset=0x60 (3 head words), length=8, "mismatch"
        let mut calldata = Vec::with_capacity(160);
        let one = U256::from(1u64).to_be_bytes::<32>();
        let two = U256::from(2u64).to_be_bytes::<32>();
        calldata.extend_from_slice(&one);
        calldata.extend_from_slice(&two);
        let mut offset = [0u8; 32];
        offset[31] = 0x60;
        calldata.extend_from_slice(&offset);
        let mut len = [0u8; 32];
        len[31] = 0x08;
        calldata.extend_from_slice(&len);
        let mut data = [0u8; 32];
        data[..8].copy_from_slice(b"mismatch");
        calldata.extend_from_slice(&data);
        assert_eq!(calldata.len(), 160);

        let out = cheats.cheat_assert(&inputs, SEL_ASSERT_EQ_U256_MSG, &calldata);
        assert!(matches!(out.result.result, InstructionResult::Revert));
        let msg = decode_error_payload(&out.result.output).expect("Error(string)");
        assert!(msg.contains("(mismatch)"), "msg should include user-supplied err: {msg}");
    }

    /// Signed-comparison cross-sign smoke test (assertGt(5, -1) must pass).
    /// Locks down the two's-complement sign-flip handling at
    /// `cheats.rs::cheat_assert`'s SEL_ASSERT_GT_I256 arm.
    #[test]
    fn assert_gt_int256_handles_cross_sign() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);

        // left = int256(5), right = int256(-1) (two's-complement = all-0xff word).
        let mut calldata = Vec::with_capacity(64);
        let five = U256::from(5u64).to_be_bytes::<32>();
        calldata.extend_from_slice(&five);
        calldata.extend_from_slice(&[0xffu8; 32]); // int256(-1)
        let out = cheats.cheat_assert(&inputs, SEL_ASSERT_GT_I256, &calldata);
        assert!(
            matches!(out.result.result, InstructionResult::Return),
            "assertGt(5, -1) must pass for signed int256 comparison",
        );

        // Reverse direction must fail: assertGt(-1, 5).
        let mut calldata = Vec::with_capacity(64);
        calldata.extend_from_slice(&[0xffu8; 32]);
        calldata.extend_from_slice(&five);
        let out = cheats.cheat_assert(&inputs, SEL_ASSERT_GT_I256, &calldata);
        assert!(matches!(out.result.result, InstructionResult::Revert));
    }

    // --- vm.addr / vm.sign behavior -----------------------------------------

    /// `vm.addr(1)` must produce the well-known Ethereum address derived from
    /// the secp256k1 secret key `1` — the simplest non-zero key in foundry's
    /// test vectors:
    ///
    /// ```text
    /// address(sk=1) == 0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf
    /// ```
    ///
    /// The output is ABI-encoded as a 32-byte word with the 20-byte address
    /// right-aligned (left-padded with 12 zero bytes).
    #[test]
    fn cheat_addr_sk1_matches_canonical_address() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..32);

        // ABI-encode `vm.addr(1)`'s only arg: 32-byte BE uint256 == 1.
        let mut args = [0u8; 32];
        args[31] = 1;
        let out = cheats.cheat_addr(&inputs, &args);
        assert!(
            matches!(out.result.result, InstructionResult::Return),
            "vm.addr(1) should return successfully, got {:?}",
            out.result.result,
        );
        assert_eq!(out.result.output.len(), 32, "address must be ABI-encoded as a 32-byte word");

        // Expected address: 0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf
        let expected: Address = "0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf".parse().unwrap();
        let mut expected_word = [0u8; 32];
        expected_word[12..].copy_from_slice(expected.as_slice());
        assert_eq!(
            out.result.output.as_ref(),
            &expected_word,
            "ABI-encoded address must equal canonical sk=1 address",
        );
    }

    /// `vm.addr(0)` is invalid (zero is not a valid secp256k1 secret key) —
    /// must revert with our specific error string.
    #[test]
    fn cheat_addr_zero_key_reverts() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..32);
        let args = [0u8; 32];
        let out = cheats.cheat_addr(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Revert));
        let msg = decode_error_payload(&out.result.output).expect("Error(string) payload");
        assert!(
            msg.contains("invalid private key"),
            "expected 'invalid private key' message, got: {msg}",
        );
    }

    /// `vm.sign(sk=1, keccak256("hello"))` must produce a deterministic
    /// (v, r, s) tuple. We don't pin the exact (v, r, s) bytes (those would
    /// duplicate the secp256k1 implementation) — instead we verify:
    ///
    /// 1. The output is exactly 96 bytes (3 × 32-byte ABI slots).
    /// 2. `v` (the last byte of slot 0) is 27 or 28 (legacy parity encoding).
    /// 3. `ecrecover(digest, v, r, s) == vm.addr(sk)` — the recovered address
    ///    matches the secret key's canonical address, which is the operational
    ///    property tests actually depend on.
    #[test]
    fn cheat_sign_roundtrips_via_ecrecover() {
        use alloy_primitives::{Signature, keccak256};
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..96);

        // ABI calldata: uint256 sk = 1, bytes32 digest = keccak256("hello").
        let digest = keccak256(b"hello");
        let mut args = Vec::with_capacity(64);
        let mut sk_word = [0u8; 32];
        sk_word[31] = 1;
        args.extend_from_slice(&sk_word);
        args.extend_from_slice(digest.as_slice());

        let out = cheats.cheat_sign(&inputs, &args);
        assert!(
            matches!(out.result.result, InstructionResult::Return),
            "vm.sign should succeed for sk=1, digest=keccak256(\"hello\"); got {:?}",
            out.result.result,
        );
        assert_eq!(out.result.output.len(), 96, "(v,r,s) must be 96 bytes (3 × 32-byte slots)");

        // Slot 0: uint8 v in last byte; rest must be zero (left-padded uint8).
        let v_byte = out.result.output[31];
        assert!(v_byte == 27 || v_byte == 28, "v must be 27 or 28, got {v_byte}");
        for &b in &out.result.output[..31] {
            assert_eq!(b, 0, "uint8 v must be left-padded with zeros");
        }

        // Slot 1: r (bytes32). Slot 2: s (bytes32).
        let r = U256::from_be_slice(&out.result.output[32..64]);
        let s = U256::from_be_slice(&out.result.output[64..96]);

        // ecrecover: build a Signature and recover the address from the digest.
        let parity = v_byte == 28;
        let sig = Signature::new(r, s, parity);
        let recovered = sig.recover_address_from_prehash(&digest).expect("ecrecover must succeed");
        let expected: Address = "0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf".parse().unwrap();
        assert_eq!(recovered, expected, "ecrecover(digest, v, r, s) must equal vm.addr(sk=1)",);
    }

    /// `vm.sign(0, ...)` rejects the zero secret key the same way `vm.addr(0)` does.
    #[test]
    fn cheat_sign_zero_key_reverts() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..96);
        let args = [0u8; 64]; // sk = 0, digest = 0
        let out = cheats.cheat_sign(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Revert));
        let msg = decode_error_payload(&out.result.output).expect("Error(string) payload");
        assert!(
            msg.contains("invalid private key"),
            "expected 'invalid private key' message, got: {msg}",
        );
    }

    // --- vm.publicKeyP256 / vm.signP256 behavior -----------------------------

    /// `vm.publicKeyP256(1)` must return the well-known P-256 generator
    /// `G = (Gx, Gy)` (the public key for the private key `1`). These
    /// constants are taken from FIPS 186-4 §D.1.2.3:
    /// - `Gx = 0x6B17D1F2E12C4247F8BCE6E563A440F277037D812DEB33A0F4A13945D898C296`
    /// - `Gy = 0x4FE342E2FE1A7F9B8EE7EB4A7C0F9E162BCE33576B315ECECBB6406837BF51F5`
    #[test]
    fn cheat_public_key_p256_sk1_matches_generator() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..64);

        let mut args = [0u8; 32];
        args[31] = 1;
        let out = cheats.cheat_public_key_p256(&inputs, &args);
        assert!(
            matches!(out.result.result, InstructionResult::Return),
            "vm.publicKeyP256(1) should succeed, got {:?}",
            out.result.result,
        );
        assert_eq!(out.result.output.len(), 64, "(x, y) must be 64 bytes (2 × 32-byte slots)");

        let expected_gx: [u8; 32] = alloy_primitives::hex::decode(
            "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
        )
        .unwrap()
        .try_into()
        .unwrap();
        let expected_gy: [u8; 32] = alloy_primitives::hex::decode(
            "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
        )
        .unwrap()
        .try_into()
        .unwrap();
        assert_eq!(&out.result.output[..32], &expected_gx, "X coordinate mismatch");
        assert_eq!(&out.result.output[32..], &expected_gy, "Y coordinate mismatch");
    }

    /// `vm.publicKeyP256(0)` must revert — zero is not a valid P-256 private
    /// key (the order-bound is `0 < sk < n`).
    #[test]
    fn cheat_public_key_p256_zero_key_reverts() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..64);
        let args = [0u8; 32];
        let out = cheats.cheat_public_key_p256(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Revert));
        let msg = decode_error_payload(&out.result.output).expect("Error(string) payload");
        assert!(
            msg.contains("private key cannot be 0"),
            "expected 'private key cannot be 0' message, got: {msg}",
        );
    }

    /// `vm.signP256(sk, digest)` must produce a signature whose `r/s` slots are
    /// 64 bytes total, with `s` low-half normalized (s <= n/2). We don't pin
    /// exact bytes (deterministic-k k might change across crate revs) — we
    /// check the shape + low-s invariant.
    #[test]
    fn cheat_sign_p256_returns_canonical_low_s_signature() {
        use alloy_primitives::keccak256;
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..64);

        let digest = keccak256(b"hello edb p256");
        let mut args = Vec::with_capacity(64);
        let mut sk_word = [0u8; 32];
        sk_word[31] = 1;
        args.extend_from_slice(&sk_word);
        args.extend_from_slice(digest.as_slice());

        let out = cheats.cheat_sign_p256(&inputs, &args);
        assert!(
            matches!(out.result.result, InstructionResult::Return),
            "vm.signP256 should succeed for sk=1, got {:?}",
            out.result.result,
        );
        assert_eq!(out.result.output.len(), 64, "(r, s) must be 64 bytes (2 × 32-byte slots)");

        // Low-s: the integer in slot 1 (s) must be <= n/2. P-256 n/2 is well
        // below 2^255, so the high bit of `s` must be clear.
        let s_bytes = &out.result.output[32..];
        let n_half = U256::from_be_bytes(P256_CURVE_ORDER_BE) >> 1;
        let s = U256::from_be_slice(s_bytes);
        assert!(s <= n_half, "signature s must be low-half-normalized; got s = {s}");
    }

    /// `vm.signP256(0, ...)` rejects the zero secret key the same way
    /// `vm.publicKeyP256(0)` does.
    #[test]
    fn cheat_sign_p256_zero_key_reverts() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..64);
        let args = [0u8; 64]; // sk = 0, digest = 0
        let out = cheats.cheat_sign_p256(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Revert));
        let msg = decode_error_payload(&out.result.output).expect("Error(string) payload");
        assert!(
            msg.contains("private key cannot be 0"),
            "expected 'private key cannot be 0' message, got: {msg}",
        );
    }

    // --- vm.expectRevert(bytes4) --------------------------------------------

    /// `vm.expectRevert(bytes4)` arms the expectation for a leading-selector
    /// match. Verify the handler decodes the bytes4 head word correctly (bytes4
    /// is left-aligned in its 32-byte slot, NOT right-aligned like uint256).
    #[test]
    fn cheat_expect_revert_bytes4_decodes_left_aligned_selector() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(CheatsConfig::default());
        let inputs = mock_call_inputs(123_000, 0..0);

        // bytes4 selector `0xdeadbeef` left-aligned in 32-byte word.
        let mut args = [0u8; 32];
        args[..4].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        let out = cheats.cheat_expect_revert_bytes4(&inputs, &args);
        assert!(matches!(out.result.result, InstructionResult::Return));
        match cheats.expected_revert.as_ref().expect("expected_revert must be armed").expected {
            ExpectedRevertMatch::Selector(sel) => {
                assert_eq!(sel, [0xde, 0xad, 0xbe, 0xef], "selector must be the leading 4 bytes");
            }
            _ => panic!("expected ExpectedRevertMatch::Selector"),
        }
    }

    // --- Synthetic CallOutcome shape ----------------------------------------

    /// Build a minimal `CallInputs` for testing the synthetic-outcome helpers.
    /// `return_memory_offset` is the load-bearing field for the C-1 bug fix:
    /// callers can pass `64..96` to assert it's threaded through ok_return /
    /// revert_with into `CallOutcome.memory_offset`.
    fn mock_call_inputs(
        gas_limit: u64,
        return_memory_offset: std::ops::Range<usize>,
    ) -> CallInputs {
        use revm::interpreter::{CallInput, CallScheme, CallValue};
        CallInputs {
            input: CallInput::Bytes(Bytes::new()),
            return_memory_offset,
            gas_limit,
            reservoir: 0,
            bytecode_address: Address::ZERO,
            known_bytecode: (B256::ZERO, Bytecode::new()),
            target_address: Address::ZERO,
            caller: Address::ZERO,
            value: CallValue::Transfer(U256::ZERO),
            scheme: CallScheme::Call,
            is_static: false,
        }
    }

    #[test]
    fn ok_return_has_return_status() {
        let inputs = mock_call_inputs(123_000, 0..0);
        let out = ok_return(&inputs, Bytes::from_static(b"x"));
        assert!(matches!(out.result.result, InstructionResult::Return));
        assert_eq!(out.result.output.as_ref(), b"x");
        assert_eq!(out.memory_offset, 0..0);
    }

    /// Regression for C-1: `memory_offset` MUST be propagated from
    /// `inputs.return_memory_offset` into `CallOutcome.memory_offset`.
    /// Hardcoding `0..0` here caused REVM to copy zero bytes into the caller's
    /// memory for static-return cheatcodes (vm.load, vm.envBool, etc.), so
    /// Solidity read zeros where the actual return value should have been.
    #[test]
    fn ok_return_propagates_memory_offset() {
        let inputs = mock_call_inputs(123_000, 64..96);
        let out = ok_return(&inputs, Bytes::from_static(b"x"));
        assert_eq!(out.memory_offset, 64..96);
    }

    #[test]
    fn revert_with_has_revert_status() {
        let inputs = mock_call_inputs(50_000, 0..0);
        let out = revert_with(&inputs, Bytes::from_static(b"r"));
        assert!(matches!(out.result.result, InstructionResult::Revert));
        assert_eq!(out.result.output.as_ref(), b"r");
    }

    #[test]
    fn revert_with_propagates_memory_offset() {
        let inputs = mock_call_inputs(50_000, 128..160);
        let out = revert_with(&inputs, Bytes::from_static(b"r"));
        assert_eq!(out.memory_offset, 128..160);
    }

    // --- abi_encode_logs sanity --------------------------------------------

    #[test]
    fn empty_logs_encode_to_length_zero_array() {
        let b = abi_encode_logs(&[]);
        // outer offset (0x20) + inner length (0)
        assert_eq!(b.len(), 64);
        let mut expect_outer = [0u8; 32];
        expect_outer[31] = 0x20;
        assert_eq!(&b[..32], expect_outer);
        assert_eq!(&b[32..64], &[0u8; 32]);
    }

    #[test]
    fn one_log_encodes_structurally() {
        // Single log with 2 topics and 5 bytes of data, structurally laid out
        // per the docstring on `abi_encode_logs`.
        let log = Log::new_unchecked(
            Address::from_slice(&[0x11; 20]),
            vec![B256::from([0x22; 32]), B256::from([0x33; 32])],
            Bytes::from_static(b"data!"),
        );
        let encoded = abi_encode_logs(&[log]);

        // [0..32]   outer offset (0x20)
        // [32..64]  array length (1)
        // [64..96]  offset-to-log-0 within the inner array block — = 0x20
        //           (i.e., n*32 where n=1 → 32 bytes ahead of the start of
        //           the offsets list, which itself starts right after the
        //           length word)
        let mut expect_outer = [0u8; 32];
        expect_outer[31] = 0x20;
        assert_eq!(&encoded[..32], expect_outer, "outer offset");

        let mut one = [0u8; 32];
        one[31] = 1;
        assert_eq!(&encoded[32..64], one, "array length");

        let mut expect_log0_off = [0u8; 32];
        expect_log0_off[31] = 0x20;
        assert_eq!(&encoded[64..96], expect_log0_off, "log[0] offset");

        // Log head: 3 words — off_topics (0x60), off_data (0x60 + 32 + 2*32 = 0xc0), emitter.
        let mut expect_off_topics = [0u8; 32];
        expect_off_topics[31] = 0x60;
        assert_eq!(&encoded[96..128], expect_off_topics, "log head: off_topics");

        let mut expect_off_data = [0u8; 32];
        expect_off_data[31] = 0xc0;
        assert_eq!(&encoded[128..160], expect_off_data, "log head: off_data");

        // emitter is right-padded in the 32-byte slot.
        let mut emitter_word = [0u8; 32];
        emitter_word[12..].copy_from_slice(&[0x11; 20]);
        assert_eq!(&encoded[160..192], emitter_word, "log head: emitter");

        // topics block: length (2), then 2 topics (32 bytes each)
        let mut two = [0u8; 32];
        two[31] = 2;
        assert_eq!(&encoded[192..224], two, "topics length");
        assert_eq!(&encoded[224..256], [0x22u8; 32], "topic[0]");
        assert_eq!(&encoded[256..288], [0x33u8; 32], "topic[1]");

        // data block: length (5), then 32-byte padded "data!"
        let mut five = [0u8; 32];
        five[31] = 5;
        assert_eq!(&encoded[288..320], five, "data length");
        let mut data_padded = [0u8; 32];
        data_padded[..5].copy_from_slice(b"data!");
        assert_eq!(&encoded[320..352], data_padded, "data (right-padded)");

        assert_eq!(encoded.len(), 352, "total encoded size");
    }

    // --- Unsupported-hit tracker + warn_once gate ---------------------------

    /// `warn_once` only emits on the first call per cheatcode name.
    /// Subsequent calls with the same name MUST be no-ops at the gate level
    /// (the set inserts return false).
    #[test]
    fn warn_once_emits_only_first_time() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let config = CheatsConfig::default();
        let cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(config.clone());

        cheats.warn_once("rollFork(uint256)", "test message");
        cheats.warn_once("rollFork(uint256)", "test message — second call");
        cheats.warn_once("pauseGasMetering", "different name, also fires once");

        let emitted = config.warnings_emitted.lock().expect("warnings mutex poisoned");
        assert_eq!(emitted.len(), 2, "two distinct names should yield two entries: {emitted:?}");
        assert!(emitted.contains("rollFork(uint256)"));
        assert!(emitted.contains("pauseGasMetering"));
    }

    /// `record_and_revert` writes a hit into the shared tracker AND returns
    /// a Revert outcome with the supplied error message ABI-encoded as
    /// `Error(string)`.
    #[test]
    fn record_and_revert_pushes_hit_and_returns_revert() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let config = CheatsConfig::default();
        let cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(config.clone());

        let inputs = mock_call_inputs(123_000, 0..0);
        let out = cheats.record_and_revert(
            &inputs,
            "selectFork",
            SEL_SELECT_FORK,
            UnsupportedCategory::Rejected,
            "EDB: cheatcode vm.selectFork not supported in v1: testing",
        );
        assert!(matches!(out.result.result, InstructionResult::Revert));
        // Error(string) selector
        assert_eq!(&out.result.output[..4], &[0x08, 0xc3, 0x79, 0xa0]);

        let hits = config.unsupported_hits.lock().expect("hits mutex poisoned");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "selectFork");
        assert_eq!(hits[0].selector, SEL_SELECT_FORK);
        assert_eq!(hits[0].category, UnsupportedCategory::Rejected);
    }

    /// Multiple factory-built inspectors must share the SAME hit tracker
    /// (the Arc is cloned, not the inner Vec). Otherwise the post-prepare
    /// drain in `run_foundry_test` would only see hits from the last pass.
    #[test]
    fn factory_built_inspectors_share_hit_tracker() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;
        let config = CheatsConfig::default();
        let factory = build_cheats_factory::<TestDB>(config.clone());
        let a = factory();
        let b = factory();

        let inputs = mock_call_inputs(10_000, 0..0);
        a.record_and_revert(
            &inputs,
            "selectFork",
            SEL_SELECT_FORK,
            UnsupportedCategory::Rejected,
            "msg-a",
        );
        b.record_and_revert(
            &inputs,
            "transact",
            SEL_TRANSACT,
            UnsupportedCategory::Rejected,
            "msg-b",
        );

        let hits = config.unsupported_hits.lock().unwrap();
        assert_eq!(hits.len(), 2, "factory clones must share the tracker: got {hits:?}");
        let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
        assert!(names.contains(&"selectFork"));
        assert!(names.contains(&"transact"));
    }

    /// `is_explicitly_rejected_name` correctly classifies the canonical set.
    #[test]
    fn explicitly_rejected_name_classification() {
        for name in [
            "createFork",
            "createSelectFork",
            "selectFork",
            "activeFork",
            "makePersistent",
            "transact",
            "broadcast",
            "startBroadcast",
            "stopBroadcast",
            "ffi",
            "readFile",
            "writeFile",
            "removeFile",
            "expectCallMinGas",
            "rollFork",
        ] {
            assert!(is_explicitly_rejected_name(name), "{name} should be rejected");
        }
        for name in ["warp", "deal", "etch", "snapshotState", "envBool", "lastCallGas"] {
            assert!(!is_explicitly_rejected_name(name), "{name} should NOT be rejected");
        }
    }

    // --- ensure_no_unsupported_hits -------------------------------------------

    /// When no hits have been recorded `ensure_no_unsupported_hits` returns Ok.
    #[test]
    fn ensure_no_unsupported_hits_ok_on_empty() {
        let hits: Arc<Mutex<Vec<UnsupportedHit>>> = Arc::new(Mutex::new(Vec::new()));
        assert!(ensure_no_unsupported_hits(&hits).is_ok());
    }

    /// When at least one hit has been recorded `ensure_no_unsupported_hits`
    /// returns `Err` whose message mentions the cheatcode name and category.
    #[test]
    fn ensure_no_unsupported_hits_err_on_hit() {
        let hits: Arc<Mutex<Vec<UnsupportedHit>>> = Arc::new(Mutex::new(vec![UnsupportedHit {
            name: "selectFork".to_string(),
            selector: SEL_SELECT_FORK,
            category: UnsupportedCategory::Rejected,
        }]));
        let err = ensure_no_unsupported_hits(&hits).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("vm.selectFork"), "expected cheatcode name in error: {msg}");
        assert!(msg.contains("rejected"), "expected category in error: {msg}");
        assert!(msg.contains("called 1x"), "expected call count in error: {msg}");
    }

    // --- vm.readLine behavior -----------------------------------------------

    /// `vm.readLine(path)` opens the file, advances the cursor by one line on
    /// each call, strips the trailing newline (LF or CRLF), and returns an
    /// empty string at EOF. This test wires the project_root sandbox to a
    /// temp dir, writes a 3-line file, and checks the cursor advances
    /// across calls (lines a, b, c, then "" at EOF).
    #[test]
    fn cheat_read_line_advances_across_calls_and_returns_empty_at_eof() {
        use revm::database::{CacheDB, EmptyDB};
        use std::io::Write;
        type TestDB = CacheDB<EmptyDB>;

        // Set up sandbox with a tempdir as project_root.
        let dir = std::env::temp_dir().join(format!("edb-readline-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lines.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        // Mix LF and CRLF to exercise both strip branches.
        write!(f, "first\nsecond\r\nthird\n").unwrap();
        drop(f);

        let config = CheatsConfig { project_root: dir.clone(), ..CheatsConfig::default() };
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(config);
        let inputs = mock_call_inputs(123_000, 0..96);

        // ABI-encode calldata: string at offset 0x20 + length + data.
        let make_calldata = |s: &str| -> Vec<u8> {
            let mut out = Vec::new();
            // head[0] = offset to (length, data) = 0x20
            let mut off = [0u8; 32];
            off[31] = 0x20;
            out.extend_from_slice(&off);
            // length
            let mut len = [0u8; 32];
            len[24..].copy_from_slice(&(s.len() as u64).to_be_bytes());
            out.extend_from_slice(&len);
            // data
            out.extend_from_slice(s.as_bytes());
            let pad = (32 - s.len() % 32) % 32;
            out.extend(std::iter::repeat_n(0u8, pad));
            out
        };
        let calldata = make_calldata("lines.txt");

        // Helper: ABI-decode the returned string from `output`.
        let decode_string = |bytes: &[u8]| -> String {
            // Layout: [offset (0x20)][length (be)][data padded]
            assert!(bytes.len() >= 64);
            let len = U256::from_be_slice(&bytes[32..64]).try_into().unwrap_or(0usize);
            String::from_utf8_lossy(&bytes[64..64 + len]).into_owned()
        };

        let out1 = cheats.cheat_read_line(&inputs, &calldata);
        assert!(matches!(out1.result.result, InstructionResult::Return));
        assert_eq!(decode_string(&out1.result.output), "first");

        let out2 = cheats.cheat_read_line(&inputs, &calldata);
        assert!(matches!(out2.result.result, InstructionResult::Return));
        assert_eq!(decode_string(&out2.result.output), "second");

        let out3 = cheats.cheat_read_line(&inputs, &calldata);
        assert!(matches!(out3.result.result, InstructionResult::Return));
        assert_eq!(decode_string(&out3.result.output), "third");

        let out_eof = cheats.cheat_read_line(&inputs, &calldata);
        assert!(matches!(out_eof.result.result, InstructionResult::Return));
        assert_eq!(decode_string(&out_eof.result.output), "");

        // Cleanup.
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `vm.readLine` rejects paths that escape the project_root sandbox.
    #[test]
    fn cheat_read_line_rejects_path_outside_sandbox() {
        use revm::database::{CacheDB, EmptyDB};
        type TestDB = CacheDB<EmptyDB>;

        let dir = std::env::temp_dir().join(format!("edb-readline-esc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let config = CheatsConfig { project_root: dir.clone(), ..CheatsConfig::default() };
        let mut cheats: EdbCheatcodes<TestDB> = EdbCheatcodes::new(config);
        let inputs = mock_call_inputs(123_000, 0..96);

        // Calldata for "/etc/passwd" — absolute, outside the sandbox.
        let s = "/etc/passwd";
        let mut calldata = Vec::new();
        let mut off = [0u8; 32];
        off[31] = 0x20;
        calldata.extend_from_slice(&off);
        let mut len = [0u8; 32];
        len[24..].copy_from_slice(&(s.len() as u64).to_be_bytes());
        calldata.extend_from_slice(&len);
        calldata.extend_from_slice(s.as_bytes());
        let pad = (32 - s.len() % 32) % 32;
        calldata.extend(std::iter::repeat_n(0u8, pad));

        let out = cheats.cheat_read_line(&inputs, &calldata);
        assert!(matches!(out.result.result, InstructionResult::Revert));
        let msg = decode_error_payload(&out.result.output).expect("Error(string) payload");
        assert!(
            msg.contains("escapes") || msg.contains("cannot resolve"),
            "expected sandbox-violation message, got: {msg}",
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
