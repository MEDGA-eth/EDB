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

    // Semantic assertion: vm.expectRevert() matched the revert from
    // `revertingFn()` and rewrote it to a success.  The overall
    // testExpectRevert frame must therefore NOT show up as a revert.
    assert!(
        !trace_has_revert(&trace),
        "vm.expectRevert should have swallowed the revert; top-level trace still shows a revert: \
         {trace:#?}",
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

    // The hand-rolled cheatcode shim groups multi-fork cheatcodes together and
    // rejects them with a single shared message.
    let needle = "EDB: cheatcode vm.multi-fork";
    let edb_needle = "selectFork";
    assert!(
        trace_revert_contains(&trace, needle) && trace_revert_contains(&trace, edb_needle),
        "expected revert with EDB rejection message mentioning {needle:?} and \
         {edb_needle:?}; got: {trace:#?}",
    );

    let _ = session.shutdown();
    Ok(())
}
