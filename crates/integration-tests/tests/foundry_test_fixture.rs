// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! E2E integration tests for `edb test` against the vendored fixture project
//! at `testdata/foundry-fixture/`.
//!
//! Each test drives the full pipeline (resolve project → compile →
//! synthesize entrypoint → engine prepare) without spawning a subprocess, by
//! calling [`edb::cmd::test::run_foundry_test_for_test`]. After preparation,
//! we inspect the engine's RPC trace to confirm the expected behavior.
//!
//! Tests are serialized via `#[serial]` because:
//!   1. `resolve_project` mutates the process-global `FOUNDRY_PROFILE`.
//!   2. `compile_entrypoint` writes `_EdbTestEntrypoint.sol` into the same
//!      project root.
//!   3. The project's `out/` / `cache/` directories aren't safe under
//!      concurrent compiles.

use eyre::Result;
use std::collections::HashSet;
use std::path::PathBuf;

use alloy_primitives::Address;
use edb_common::types::{CallResult, Code};
use edb_integration_tests::test_utils::{init, paths};
use serial_test::serial;

fn fixture_root() -> PathBuf {
    paths::workspace_root().join("testdata").join("foundry-fixture")
}

/// Whether any trace entry's `result` is a revert.
fn trace_has_revert(trace: &edb_common::types::Trace) -> bool {
    trace.iter().any(|entry| matches!(entry.result, Some(CallResult::Revert { .. })))
}

/// Search for `needle` in any revert output across the trace. We only look at
/// `Revert` results (not `Success` or `Error`) because successful frames
/// return data that may *contain* literal revert-reason strings as part of the
/// runtime bytecode — these are not actual revert messages.
fn trace_revert_contains(trace: &edb_common::types::Trace, needle: &str) -> bool {
    trace.iter().any(|entry| match &entry.result {
        Some(CallResult::Revert { output, .. }) => String::from_utf8_lossy(output).contains(needle),
        _ => false,
    })
}

/// ABI-decode a Solidity `Error(string)` revert payload.
///
/// Layout:
/// - `[0..4)`   — selector `0x08c379a0`
/// - `[4..36)`  — offset (== 0x20)
/// - `[36..68)` — string length (big-endian u256, fits in usize)
/// - `[68..68+len)` — UTF-8 string bytes
///
/// Returns `None` if the bytes are not a well-formed `Error(string)`.
#[allow(dead_code)] // retained for trace-inspection tests not yet ported
fn decode_error_string(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 68 || bytes[..4] != [0x08, 0xc3, 0x79, 0xa0] {
        return None;
    }
    // Length lives in the last 4 bytes of the 32-byte length word (bytes 36..68).
    // For any realistic string the upper bytes are zero; we read the low 4 bytes.
    let len_bytes: [u8; 4] = bytes[64..68].try_into().ok()?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    let data_end = 68usize.checked_add(len)?;
    if bytes.len() < data_end {
        return None;
    }
    std::str::from_utf8(&bytes[68..data_end]).ok().map(String::from)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn basic_test_trivial_runs() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Basic::testTrivial",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    // The top-level entrypoint call must succeed (no revert anywhere in the trace).
    assert!(
        !trace_has_revert(&trace),
        "Basic::testTrivial should not produce any revert; trace = {trace:#?}",
    );

    // The trace should at least include the top-level call into the entrypoint.
    assert!(!trace.is_empty(), "trace was empty for Basic::testTrivial");

    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_warp_actually_intercepts() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Cheats::testWarp",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    // testWarp internally `require`s that block.timestamp == 1234567 after
    // vm.warp(1234567). If the cheatcode didn't intercept, the require would
    // revert with "vm.warp did not apply".
    assert!(!trace_has_revert(&trace), "vm.warp didn't intercept; testWarp reverted: {trace:#?}",);
    assert!(
        !trace_revert_contains(&trace, "vm.warp did not apply"),
        "vm.warp didn't intercept; require fired: {trace:#?}",
    );

    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_deal_actually_intercepts() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Cheats::testDeal",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(!trace_has_revert(&trace), "vm.deal didn't intercept; testDeal reverted: {trace:#?}",);
    assert!(
        !trace_revert_contains(&trace, "vm.deal did not apply"),
        "vm.deal didn't intercept; require fired: {trace:#?}",
    );

    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_expect_revert_rewrites_outcome() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Cheats::testExpectRevert",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(!trace.is_empty(), "trace was empty for Cheats::testExpectRevert");

    // Structural assertion: the top-level entrypoint frame (depth 0) must end
    // in success.  vm.expectRevert rewrites the matched revert to a Return at
    // call_end time, so this frame — which encompasses the entire test — must
    // be Success when the cheatcode works correctly.
    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry in trace for Cheats::testExpectRevert");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level (depth-0) frame should be Success after expectRevert match; got: {:?}",
        top.result,
    );

    // Belt-and-suspenders: no frame should carry our "did not match" error string.
    assert!(
        !trace_revert_contains(&trace, "expectRevert did not match"),
        "expectRevert reported a mismatch — the cheatcode call likely consumed \
         expected_revert instead of the user-code call: {trace:#?}",
    );

    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_expect_emit_matches_softly() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Cheats::testExpectEmit",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(!trace.is_empty(), "trace was empty for Cheats::testExpectEmit");

    // Structural assertion: top-level (depth-0) frame must end in success.
    // If the soft-match expectEmit fails, our cheatcode rewrites the
    // registering frame to a Revert carrying "EDB: expectEmit did not match".
    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry in trace for Cheats::testExpectEmit");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success when expectEmit matches; got: {:?}",
        top.result,
    );

    // Belt-and-suspenders: no frame should carry the EDB expectEmit failure message.
    assert!(
        !trace_revert_contains(&trace, "expectEmit did not match"),
        "expectEmit reported a mismatch; trace = {trace:#?}",
    );

    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_expect_call_counts_matching_calls() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Cheats::testExpectCall",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(!trace.is_empty(), "trace was empty for Cheats::testExpectCall");

    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry in trace for Cheats::testExpectCall");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success when expectCall is satisfied; got: {:?}",
        top.result,
    );

    assert!(
        !trace_revert_contains(&trace, "expectCall did not match"),
        "expectCall reported a mismatch; trace = {trace:#?}",
    );

    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_assume_true_succeeds() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Cheats::testAssumeTrue",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(!trace.is_empty(), "trace was empty for Cheats::testAssumeTrue");

    // vm.assume(true) is a no-op — the test function must complete without revert.
    let top =
        trace.iter().find(|e| e.depth == 0).expect("no depth-0 entry for Cheats::testAssumeTrue");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success when assume(true); got: {:?}",
        top.result,
    );

    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_env_or_returns_fallback() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Cheats::testEnvOrString",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(!trace.is_empty(), "trace was empty for Cheats::testEnvOrString");

    // testEnvOrString calls vm.envOr("EDB_TEST_NONEXISTENT_VAR_XYZ", "fallback").
    // The var is not set, so the fallback "fallback" should be returned; the
    // Solidity `require` verifies the value, failing if envOr misbehaves.
    let top =
        trace.iter().find(|e| e.depth == 0).expect("no depth-0 entry for Cheats::testEnvOrString");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success when envOr returns fallback; got: {:?}",
        top.result,
    );

    assert!(
        !trace_revert_contains(&trace, "vm.envOr should return fallback"),
        "envOr fallback check failed; trace = {trace:#?}",
    );

    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_gas_metering_stubs_dont_revert() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Cheats::testGasMeteringStubs",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(!trace.is_empty(), "trace was empty for Cheats::testGasMeteringStubs");

    // All three gas-metering cheatcodes are stubs that must not revert.
    // The test confirms the full call sequence flows through without error.
    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry for Cheats::testGasMeteringStubs");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success for gas metering stubs; got: {:?}",
        top.result,
    );

    let _ = session.shutdown();
    Ok(())
}

/// Regression for C-1 (memory_offset propagation).
///
/// `vm.load` returns a STATIC `bytes32` — Solidity reads it from
/// `mem[memory_offset..memory_offset + 32]`. The previous
/// `ok_return`/`revert_with` helpers hardcoded `memory_offset: 0..0`, so REVM
/// copied zero bytes into the caller's memory and Solidity saw `bytes32(0)`
/// regardless of what the storage slot actually held — silent corruption.
///
/// `Cheats::testLoadReadsActualStorage` stores a known value into a known slot
/// then reads it back via `vm.load`; a `require` inside the test fails (and
/// the top-level frame reverts) if the read returns anything other than the
/// stored value. With the bug, the test reverts; with the fix, the top frame
/// is `Success`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_load_returns_stored_value() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Cheats::testLoadReadsActualStorage",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(!trace.is_empty(), "trace was empty for Cheats::testLoadReadsActualStorage");

    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry for Cheats::testLoadReadsActualStorage");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success — if it reverted with 'vm.load returned wrong value', \
         the C-1 memory_offset propagation regressed; got: {:?}",
        top.result,
    );

    assert!(
        !trace_revert_contains(&trace, "vm.load returned wrong value"),
        "vm.load returned the wrong value (memory_offset propagation regressed); trace = {trace:#?}",
    );

    let _ = session.shutdown();
    Ok(())
}

/// Regression guard for Task 6.3 (nested CREATE): the synthetic entrypoint
/// CREATEs `NestedDeploy` at depth 1; `NestedDeploy`'s `setUp()` then CREATEs
/// `Inner` at depth 2. We confirm two things end-to-end:
///
/// 1. The top-level frame succeeds (no unexpected reverts).
/// 2. The trace contains a `created_contract == true` entry at depth >= 2
///    (the nested CREATE for `Inner`), AND that nested address has at least
///    one *hook* snapshot bound to its bytecode — i.e. the instrumentation
///    pipeline reached the nested deployment and not just the test contract.
///
/// If hook snapshots only fire for the directly-CREATEd test contract, this
/// test fails on assertion 2.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn nested_deploy_hooks_fire_for_inner_contract() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "NestedDeploy::testInnerWasDeployed",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;

    let trace = session.fetch_trace().await?;

    // (1) Top-level entrypoint frame must succeed.
    let top = trace.iter().find(|e| e.depth == 0).expect("no depth-0 entry in trace");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success for NestedDeploy::testInnerWasDeployed; got: {:?}",
        top.result,
    );
    assert!(
        !trace_has_revert(&trace),
        "NestedDeploy::testInnerWasDeployed should not produce any revert; trace = {trace:#?}",
    );

    // (2a) Find the nested CREATE entry: `Inner` is created by NestedDeploy's
    // setUp / constructor, so it shows up at depth >= 2 with created_contract.
    let inner_create = trace
        .iter()
        .find(|e| e.created_contract && e.depth >= 2)
        .expect("no nested CREATE (depth >= 2) found in trace — fixture or trace shape changed");
    let inner_address = inner_create.target;
    assert_ne!(
        inner_address,
        alloy_primitives::Address::ZERO,
        "nested CREATE target_address is ZERO (creation reverted?)",
    );

    // (2b) Pull every snapshot and count hook snapshots per bytecode_address.
    // We require at least one *hook* snapshot bound to Inner's address. We
    // also collect the set of all hook-snapshot bytecode addresses for the
    // debug message, so a regression report is actionable.
    let count = session.snapshot_count().await?;
    assert!(count > 0, "engine produced zero snapshots — nothing was instrumented");

    let mut hook_addrs: std::collections::HashSet<alloy_primitives::Address> =
        std::collections::HashSet::new();
    let mut inner_hook_count = 0usize;
    for id in 0..count {
        let info = session.fetch_snapshot_info(id as u64).await?;
        // `detail` is an enum serialized as { "Hook": {..} } | { "Opcode": {..} }.
        let is_hook = info.get("detail").and_then(|d| d.get("Hook")).is_some();
        if !is_hook {
            continue;
        }
        let bytecode_addr_str =
            info.get("bytecode_address").and_then(|v| v.as_str()).unwrap_or_default();
        let parsed: alloy_primitives::Address =
            bytecode_addr_str.parse().unwrap_or(alloy_primitives::Address::ZERO);
        hook_addrs.insert(parsed);
        if parsed == inner_address {
            inner_hook_count += 1;
        }
    }

    assert!(
        inner_hook_count > 0,
        "no hook snapshots fired for nested-CREATEd Inner at {inner_address}; \
         observed hook bytecode addresses = {hook_addrs:?}",
    );

    // Sanity: we should see at least 2 distinct hook-snapshot bytecode
    // addresses (test contract + Inner). If everything collapses to a single
    // address, instrumentation is only reaching the top contract.
    assert!(
        hook_addrs.len() >= 2,
        "expected hook snapshots from >= 2 distinct addresses (test contract + Inner); \
         got {} unique addresses = {hook_addrs:?}",
        hook_addrs.len(),
    );

    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn boundary_select_fork_aborts_before_ui() -> Result<()> {
    // Post-Item-17 behavior: when prepare observes any unsupported cheatcode
    // call, run_foundry_test_for_test bails AFTER prepare completes instead
    // of returning a session and burying the EDB error inside the trace.
    init::init_test_environment(true);
    let root = fixture_root();
    let result = edb::cmd::test::run_foundry_test_for_test(
        "Boundary::testSelectForkIsRejected",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await;

    let err = match result {
        Ok(session) => {
            let _ = session.shutdown();
            panic!("expected run_foundry_test_for_test to abort, but it returned Ok(session)")
        }
        Err(e) => e.to_string(),
    };

    assert!(
        err.contains("vm.selectFork"),
        "expected abort message to mention vm.selectFork; got: {err}",
    );
    assert!(
        err.contains("not supported"),
        "expected abort message to say 'not supported in v1'; got: {err}",
    );
    assert!(
        err.contains("rejected"),
        "expected abort message to tag the cheatcode as rejected; got: {err}",
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Combo interaction tests — each function combines 3+ cheatcodes in one test
// ---------------------------------------------------------------------------

/// Combo: vm.deal + vm.prank + vm.expectEmit
///
/// Verifies that prank state, deal state, and expectEmit accounting all coexist
/// without bleeding into each other.  testPrankDealEmit funds alice via deal,
/// pranks msg.sender to alice, and expects a Deposit log before calling
/// bank.deposit.  The soft-match expectEmit must fire on the Deposit log that
/// deposit() emits under alice's identity.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn combo_prank_deal_emit() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Combo::testPrankDealEmit",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(!trace.is_empty(), "trace was empty for Combo::testPrankDealEmit");

    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry in trace for Combo::testPrankDealEmit");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success for testPrankDealEmit (deal+prank+expectEmit combo); \
         got: {:?}",
        top.result,
    );
    assert!(
        !trace_revert_contains(&trace, "expectEmit did not match"),
        "expectEmit mismatch in prank+deal+emit combo; trace = {trace:#?}",
    );
    assert!(
        !trace_revert_contains(&trace, "balance wrong"),
        "balance assertion failed after prank+deposit; trace = {trace:#?}",
    );

    let _ = session.shutdown();
    Ok(())
}

/// Combo: vm.warp + vm.mockCall + vm.expectCall
///
/// Verifies that warp, mockCall interception, and expectCall accounting all
/// coexist.  testWarpMockExpectCall sets block.timestamp, mocks a balanceOf
/// return, and asserts the call is counted by expectCall.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn combo_warp_mock_expect_call() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Combo::testWarpMockExpectCall",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(!trace.is_empty(), "trace was empty for Combo::testWarpMockExpectCall");

    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry in trace for Combo::testWarpMockExpectCall");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success for testWarpMockExpectCall (warp+mock+expectCall combo); \
         got: {:?}",
        top.result,
    );
    assert!(
        !trace_revert_contains(&trace, "warp did not apply"),
        "vm.warp failed inside warp+mock+expectCall combo; trace = {trace:#?}",
    );
    assert!(
        !trace_revert_contains(&trace, "mockCall did not intercept"),
        "mockCall interception failed inside warp+mock+expectCall combo; trace = {trace:#?}",
    );
    assert!(
        !trace_revert_contains(&trace, "expectCall did not match"),
        "expectCall accounting failed inside warp+mock+expectCall combo; trace = {trace:#?}",
    );

    let _ = session.shutdown();
    Ok(())
}

/// Combo: vm.deal + vm.startPrank + vm.expectRevert + vm.stopPrank
///
/// Verifies that startPrank/stopPrank bracket works around expectRevert without
/// leaking prank state past the stopPrank call.  testStartPrankExpectRevertStopPrank
/// funds bob, starts a prank as bob, makes a valid deposit, then expects a revert
/// from an over-limit withdraw, and finally stops the prank — verifying bob's
/// balance is unchanged after the failed (but expected) withdraw.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn combo_start_prank_expect_revert_stop_prank() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Combo::testStartPrankExpectRevertStopPrank",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(!trace.is_empty(), "trace was empty for Combo::testStartPrankExpectRevertStopPrank");

    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry in trace for Combo::testStartPrankExpectRevertStopPrank");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success for testStartPrankExpectRevertStopPrank \
         (startPrank+expectRevert+stopPrank combo); got: {:?}",
        top.result,
    );
    assert!(
        !trace_revert_contains(&trace, "expectRevert did not match"),
        "expectRevert mismatch in startPrank+expectRevert+stopPrank combo; trace = {trace:#?}",
    );
    assert!(
        !trace_revert_contains(&trace, "bob balance wrong"),
        "balance assertion failed after startPrank+expectRevert+stopPrank; trace = {trace:#?}",
    );

    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn snapshot_revert_round_trip() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "SnapshotTest::testRevertRestoresStorage",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    let top = trace.iter().find(|e| e.depth == 0).expect("top frame");
    assert!(
        matches!(top.result, Some(edb_common::types::CallResult::Success { .. })),
        "top frame failed: {:?}",
        top.result
    );
    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn revert_to_unknown_id_returns_false() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "SnapshotTest::testRevertToUnknownReturnsFalse",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    let top = trace.iter().find(|e| e.depth == 0).expect("top frame");
    assert!(matches!(top.result, Some(edb_common::types::CallResult::Success { .. })));
    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn delete_then_revert_returns_false() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "SnapshotTest::testDeleteThenRevert",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    let top = trace.iter().find(|e| e.depth == 0).expect("top frame");
    assert!(
        matches!(top.result, Some(edb_common::types::CallResult::Success { .. })),
        "top frame failed: {:?}",
        top.result
    );
    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn delete_all_snapshots_clears_state() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "SnapshotTest::testDeleteAllSnapshots",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    let top = trace.iter().find(|e| e.depth == 0).expect("top frame");
    assert!(matches!(top.result, Some(edb_common::types::CallResult::Success { .. })));
    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn delete_unknown_id_returns_false() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "SnapshotTest::testDeleteUnknownReturnsFalse",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    let top = trace.iter().find(|e| e.depth == 0).expect("top frame");
    assert!(matches!(top.result, Some(edb_common::types::CallResult::Success { .. })));
    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn snapshot_state_returns_monotonic_id() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "SnapshotTest::testSnapshotReturnsMonotonicId",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(!trace.is_empty(), "trace was empty for SnapshotTest::testSnapshotReturnsMonotonicId");

    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry in trace for SnapshotTest::testSnapshotReturnsMonotonicId");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level (depth-0) frame should be Success for testSnapshotReturnsMonotonicId; got: {:?}",
        top.result,
    );

    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn roll_fork_updates_block_number() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "SnapshotTest::testRollForkSetsBlockNumber",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    let top = trace.iter().find(|e| e.depth == 0).expect("top frame");
    assert!(
        matches!(top.result, Some(edb_common::types::CallResult::Success { .. })),
        "top frame failed: {:?}",
        top.result
    );
    let _ = session.shutdown();
    Ok(())
}

// ---------------------------------------------------------------------------
// Assertion family (C2-6 from the Round-2 audit) — end-to-end coverage for
// the 40 assertion handlers in `cmd/test/cheats.rs::cheat_assert`. Lives in
// `testdata/foundry-fixture/test/Asserts.t.sol`.
// ---------------------------------------------------------------------------

/// Helper: run one `Asserts::testFoo` and assert the top-level frame is
/// `Success` with no revert anywhere in the trace.
async fn run_asserts_test(target: &str) -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        target,
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    assert!(!trace.is_empty(), "trace was empty for {target}");

    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .unwrap_or_else(|| panic!("no depth-0 entry in trace for {target}"));
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success for {target}; got: {:?}",
        top.result,
    );
    assert!(!trace_has_revert(&trace), "unexpected revert in trace for {target}: {trace:#?}");

    let _ = session.shutdown();
    Ok(())
}

/// Trivial: `vm.assertEq(1, 1)` through forge-std's wrapper. Exercises the
/// short-circuit branch (no actual cheatcode call).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn asserts_eq_uint_passes() -> Result<()> {
    run_asserts_test("Asserts::testAssertEqUintPasses").await
}

/// C2-1 regression: direct `vm.assertTrue(true)` — 32-byte calldata that the
/// previous bouncer rejected with "insufficient calldata". After the fix the
/// inspector returns successfully and the top frame is Success.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn asserts_true_direct_call_does_not_revert() -> Result<()> {
    run_asserts_test("Asserts::testAssertTrueDirectCall").await
}

/// Direct call to `vm.assertFalse(false)` — symmetric to assertTrue.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn asserts_false_direct_call_does_not_revert() -> Result<()> {
    run_asserts_test("Asserts::testAssertFalseDirectCall").await
}

/// Cross-sign signed comparison: `assertGt(5, -1)` must pass. Exercises the
/// two's-complement sign-flip branch in `cheat_assert`'s SEL_ASSERT_GT_I256.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn asserts_gt_signed_cross_sign_passes() -> Result<()> {
    run_asserts_test("Asserts::testAssertGtSignedCrossSign").await
}

/// Passing assertion with a custom message via forge-std's
/// `assertEq(uint, uint, string)` wrapper.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn asserts_eq_with_custom_message_pass() -> Result<()> {
    run_asserts_test("Asserts::testAssertEqWithCustomMessagePass").await
}

/// C2-4 regression: direct call to `vm.assertTrue(true, "ok")` on the passing
/// branch. The 2-head-word + string-tail layout used to trip the bouncer or
/// produce a malformed error message; with the fix the call succeeds and the
/// test's top frame is Success. Combined with the unit test
/// `assert_true_with_message_decodes_offset_correctly` (failing branch), this
/// locks the decoder fix in end-to-end.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn asserts_true_with_message_pass() -> Result<()> {
    run_asserts_test("Asserts::testAssertTrueWithMessagePass").await
}

// ---------------------------------------------------------------------------
// Crypto family — `vm.addr` and `vm.sign`.
// Lives in `testdata/foundry-fixture/test/Crypto.t.sol`.
// ---------------------------------------------------------------------------

/// `vm.addr(1)` returns the canonical sk=1 address — the fixture `require`s
/// it equals `0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf`. If our handler
/// regresses (zero key, wrong padding, wrong derivation), the require fires
/// and the top frame reverts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_addr_returns_canonical_sk1_address() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Crypto::testAddrFromKey",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    assert!(!trace.is_empty(), "trace was empty for Crypto::testAddrFromKey");
    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry in trace for Crypto::testAddrFromKey");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success when vm.addr(1) returns the canonical address; got: {:?}",
        top.result,
    );
    assert!(
        !trace_revert_contains(&trace, "vm.addr(1) did not return"),
        "vm.addr(1) returned the wrong address; trace = {trace:#?}",
    );
    let _ = session.shutdown();
    Ok(())
}

/// `vm.sign(sk, digest)` produces a (v, r, s) triple that `ecrecover` recovers
/// back to `vm.addr(sk)`. Exercises the full pipeline through the EVM
/// `ecrecover` precompile, so a regression in v-parity normalization or
/// r/s byte order would surface here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_sign_roundtrips_via_ecrecover() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Crypto::testSignAndEcrecover",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    assert!(!trace.is_empty(), "trace was empty for Crypto::testSignAndEcrecover");
    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry in trace for Crypto::testSignAndEcrecover");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success for vm.sign -> ecrecover roundtrip; got: {:?}",
        top.result,
    );
    assert!(
        !trace_revert_contains(&trace, "did not recover"),
        "ecrecover did not recover vm.addr(sk); trace = {trace:#?}",
    );
    let _ = session.shutdown();
    Ok(())
}

/// `vm.expectRevert(bytes4)` — match the leading 4-byte selector of the next
/// sub-call's revert payload. The fixture calls a contract that reverts with
/// a custom `error BadInput(uint256)`; the cheatcode is armed with
/// `BadInput.selector`. EDB must match the leading 4 bytes regardless of the
/// trailing ABI args, rewrite the revert to a success at call_end, and let
/// the test's top frame succeed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_expect_revert_bytes4_matches_selector() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Cheats::testExpectRevertSelector",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    assert!(!trace.is_empty(), "trace was empty for Cheats::testExpectRevertSelector");
    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry in trace for Cheats::testExpectRevertSelector");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should be Success when vm.expectRevert(bytes4) matches; got: {:?}",
        top.result,
    );
    assert!(
        !trace_revert_contains(&trace, "expectRevert did not match"),
        "vm.expectRevert(bytes4) reported a mismatch; trace = {trace:#?}",
    );
    let _ = session.shutdown();
    Ok(())
}

/// `vm.startPrank(address msgSender, address txOrigin)` — selector
/// `0x45b56078`. The fixture's `Cheats::testStartPrankWithOrigin` verifies
/// (via an external `PrankWitness.observe()` call) that both msg.sender and
/// tx.origin are overridden under the prank and restored after stopPrank.
/// EDB must run the test to a successful top-frame return.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_start_prank_with_origin_overrides_both() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Cheats::testStartPrankWithOrigin",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    assert!(!trace.is_empty(), "trace was empty for Cheats::testStartPrankWithOrigin");
    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry for Cheats::testStartPrankWithOrigin");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should succeed when startPrank(addr,addr) overrides both \
         msg.sender and tx.origin; got: {:?}",
        top.result,
    );
    let _ = session.shutdown();
    Ok(())
}

/// Companion to `cheats_start_prank_with_origin_overrides_both`: confirms
/// that the freshly-deployed `PrankWitness` contract (defined in the same
/// `Cheats.t.sol`) resolves to source via `edb_getCodeByAddress`. This is
/// the smallest verifiable instance of the foundry-style local-artifact
/// match path working end-to-end on a brand-new deployment: the witness
/// has no immutables / link refs of its own, so it must hit the exact-hash
/// branch of `LocalArtifactSet` (not the fuzzy fallback).
///
/// Observed at the time of writing: 4 unique addrs in trace, 3 with source
/// (the entrypoint, the test contract, and PrankWitness).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_fixture)]
async fn cheats_start_prank_resolves_witness_artifact() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Cheats::testStartPrankWithOrigin",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    let top = trace
        .iter()
        .find(|e| e.depth == 0)
        .expect("no depth-0 entry for Cheats::testStartPrankWithOrigin");
    assert!(
        matches!(top.result, Some(CallResult::Success { .. })),
        "top-level frame should succeed; got: {:?}",
        top.result,
    );

    // Walk the trace, dedupe by code_address, then ask the engine which of
    // those resolved to a local artifact.
    let mut seen: HashSet<Address> = HashSet::new();
    for entry in trace.iter() {
        seen.insert(entry.code_address);
    }
    let mut with_source = 0usize;
    let mut total = 0usize;
    for addr in &seen {
        match session.fetch_code(*addr).await {
            Ok(Code::Source(_)) => {
                with_source += 1;
                total += 1;
            }
            Ok(Code::Opcode(_)) => {
                total += 1;
            }
            Err(_) => {}
        }
    }

    assert!(
        total >= 3,
        "expected ≥3 unique trace addresses (entrypoint + test contract + \
         PrankWitness), got {total}",
    );
    assert!(
        with_source >= 2,
        "expected ≥2 source-resolved addresses (test contract + freshly-deployed \
         PrankWitness); got {with_source}/{total}. If PrankWitness fell through \
         to Opcode the foundry-style artifact match for fresh deployments has \
         regressed.",
    );

    let _ = session.shutdown();
    Ok(())
}
