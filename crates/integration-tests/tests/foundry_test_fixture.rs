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
use std::path::PathBuf;

use edb_common::types::CallResult;
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
async fn boundary_select_fork_reverts_with_edb_message() -> Result<()> {
    init::init_test_environment(true);
    let root = fixture_root();
    let session = edb::cmd::test::run_foundry_test_for_test(
        "Boundary::testSelectForkIsRejected",
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    // The hand-rolled cheatcode shim rejects multi-fork cheatcodes with an
    // ABI-encoded Error(string) whose message contains "EDB: cheatcode vm."
    // and "selectFork".  We collect all Revert outputs, ABI-decode them as
    // Error(string), and assert that at least one decodes to the expected
    // rejection message — verifying both the ABI encoding and the content.
    let revert_outputs: Vec<Vec<u8>> = trace
        .iter()
        .filter_map(|entry| match &entry.result {
            Some(CallResult::Revert { output, .. }) => Some(output.to_vec()),
            _ => None,
        })
        .collect();
    assert!(
        !revert_outputs.is_empty(),
        "expected at least one revert in trace for testSelectForkIsRejected; got: {trace:#?}",
    );

    let any_edb_error = revert_outputs.iter().any(|bytes| {
        decode_error_string(bytes)
            .is_some_and(|s| s.contains("EDB: cheatcode vm.") && s.contains("selectFork"))
    });
    assert!(
        any_edb_error,
        "expected ABI-decoded Error(string) containing EDB selectFork rejection; \
         got revert outputs: {:?}",
        revert_outputs
            .iter()
            .map(|b| decode_error_string(b).unwrap_or_else(|| format!("<raw {} bytes>", b.len())))
            .collect::<Vec<_>>(),
    );

    let _ = session.shutdown();
    Ok(())
}
