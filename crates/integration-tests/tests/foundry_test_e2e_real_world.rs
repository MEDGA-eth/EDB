// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! E2E integration tests against vendored real-world Foundry projects in
//! `testdata/foundry-e2e/`.
//!
//! These projects are fetched on-demand by
//! `scripts/fetch-e2e-foundry-projects.sh`. Run with:
//!
//! ```sh
//! ./scripts/fetch-e2e-foundry-projects.sh
//! cargo test --package edb-integration-tests \
//!     --test foundry_test_e2e_real_world
//! ```

use eyre::Result;
use std::path::PathBuf;

use edb_common::types::CallResult;
use edb_integration_tests::test_utils::{init, paths};
use serial_test::serial;

fn e2e_root() -> PathBuf {
    std::env::var("EDB_E2E_FIXTURES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| paths::workspace_root().join("testdata").join("foundry-e2e"))
}

fn require_fixture(name: &str) -> PathBuf {
    let p = e2e_root().join(name);
    assert!(
        p.join("foundry.toml").exists(),
        "fixture {name} missing at {}. Run scripts/fetch-e2e-foundry-projects.sh first.",
        p.display(),
    );
    p
}

fn trace_has_revert(trace: &edb_common::types::Trace) -> bool {
    trace.iter().any(|entry| matches!(entry.result, Some(CallResult::Revert { .. })))
}

fn top_frame_is_success(trace: &edb_common::types::Trace) -> bool {
    trace
        .iter()
        .find(|e| e.depth == 0)
        .map(|e| matches!(e.result, Some(CallResult::Success { .. })))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Existing tests (forge-template + solady smoke)
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn forge_template_runs_a_test() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("forge-template");

    // forge-template ships `test/Contract.t.sol` with contract `TestContract`
    // and a pure assertion test `testBar`. (See task plan; pinned at the
    // commit referenced by scripts/fetch-e2e-foundry-projects.sh.)
    let target = "TestContract::testBar";
    let session = edb::cmd::test::run_foundry_test_for_test(
        target,
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    assert!(
        !trace_has_revert(&trace),
        "forge-template {target} should not revert; got: {trace:#?}",
    );
    let _ = session.shutdown();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn solady_runs_a_pure_lib_test() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("solady");

    // solady's `test/LibString.t.sol` contains the pure assertion-only
    // `LibStringTest::testToStringZero`, which doesn't depend on any vm
    // cheatcode or external state. Solid choice for an e2e smoke test.
    let target = "LibStringTest::testToStringZero";
    let session = edb::cmd::test::run_foundry_test_for_test(
        target,
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    assert!(!trace_has_revert(&trace), "solady {target} should not revert; got: {trace:#?}",);
    let _ = session.shutdown();
    Ok(())
}

// ---------------------------------------------------------------------------
// Solady — additional pure lib tests
// ---------------------------------------------------------------------------

/// Solady LibStringTest::testToStringPositiveNumber — converts a small
/// positive integer to its decimal string representation. Pure string
/// manipulation with no cheatcodes. LibStringTest is a large contract
/// (> EIP-3860 initcode limit), so EDB replays the snapshot pass with
/// relaxed constraints; we assert no test-logic revert occurs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn solady_lib_string_to_string_positive() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("solady");

    let target = "LibStringTest::testToStringPositiveNumber";
    let session = edb::cmd::test::run_foundry_test_for_test(
        target,
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;
    // LibStringTest exceeds the EIP-3860 initcode limit so the initial replay
    // uses relaxed EVM constraints; the test logic itself must not revert.
    assert!(!trace_has_revert(&trace), "solady {target} should not revert; got: {trace:#?}",);
    let _ = session.shutdown();
    Ok(())
}

// ---------------------------------------------------------------------------
// Uniswap V4 — pure tick-math tests (no fork, no unsupported cheatcodes)
// ---------------------------------------------------------------------------

/// Uniswap V4 TickTest::testTick_tickSpacingToMaxLiquidityPerTick_returnsTheCorrectValueForLowFee
///
/// A `pure` function that verifies tickSpacingToMaxLiquidityPerTick returns
/// the correct constant for the low-fee (10-spacing) tier. No state writes,
/// no cheatcodes — ideal smoke test for a real-world DeFi library.
/// TickTest init bytecode is ~26 kB (well under EIP-3860's 49 kB limit).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn uniswap_v4_tick_max_liquidity_low_fee() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("uniswap-v4-core");

    let target =
        "TickTest::testTick_tickSpacingToMaxLiquidityPerTick_returnsTheCorrectValueForLowFee";
    let session = edb::cmd::test::run_foundry_test_for_test(
        target,
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(
        top_frame_is_success(&trace),
        "top-level frame should be Success for {target}; trace: {trace:#?}",
    );
    let _ = session.shutdown();
    Ok(())
}

/// Uniswap V4 TickTest::testTick_getFeeGrowthInside_returnsAllForTwoUninitializedTicksIfTickIsInside
///
/// A state-mutation test that writes fee-growth accumulators into a pool's
/// tick storage and then reads them back. Exercises EDB's ability to replay
/// SSTORE/SLOAD across multiple sub-calls without any fork or cheatcode.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn uniswap_v4_tick_fee_growth_inside() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("uniswap-v4-core");

    let target =
        "TickTest::testTick_getFeeGrowthInside_returnsAllForTwoUninitializedTicksIfTickIsInside";
    let session = edb::cmd::test::run_foundry_test_for_test(
        target,
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(
        top_frame_is_success(&trace),
        "top-level frame should be Success for {target}; trace: {trace:#?}",
    );
    let _ = session.shutdown();
    Ok(())
}

/// Uniswap V4 TickTest::testTick_getFeeGrowthInside_returns0ForTwoUninitializedTicksIfTickIsBelow
///
/// Verifies that fee-growth-inside returns (0, 0) for two uninitialized ticks
/// when the current tick is below the range. Pure state-read with no cheatcodes.
/// Exercises the same `getFeeGrowthInside` code path as the "inside" variant
/// but from a different tick position.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn uniswap_v4_tick_fee_growth_below() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("uniswap-v4-core");

    let target =
        "TickTest::testTick_getFeeGrowthInside_returns0ForTwoUninitializedTicksIfTickIsBelow";
    let session = edb::cmd::test::run_foundry_test_for_test(
        target,
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(
        top_frame_is_success(&trace),
        "top-level frame should be Success for {target}; trace: {trace:#?}",
    );
    let _ = session.shutdown();
    Ok(())
}

// ---------------------------------------------------------------------------
// Solmate — ERC-20 and fixed-point math unit tests
// ---------------------------------------------------------------------------

/// Solmate ERC20Test::testMint — mints tokens to a recipient and asserts
/// totalSupply and balanceOf. A classic state-mutation test with no fork
/// dependencies. Uses DSTestPlus (the DSTest-derived framework with `hevm`).
/// ERC20Test init bytecode is ~31 kB (under EIP-3860 limit).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn solmate_erc20_mint() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("solmate");

    let target = "ERC20Test::testMint";
    let session = edb::cmd::test::run_foundry_test_for_test(
        target,
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(
        top_frame_is_success(&trace),
        "top-level frame should be Success for {target}; trace: {trace:#?}",
    );
    let _ = session.shutdown();
    Ok(())
}

/// Solmate ERC20Test::testTransfer — mints tokens to the test contract itself,
/// then transfers them to a recipient and asserts both balances.
/// Exercises EDB's cross-contract call replay for a real-world ERC-20.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn solmate_erc20_transfer() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("solmate");

    let target = "ERC20Test::testTransfer";
    let session = edb::cmd::test::run_foundry_test_for_test(
        target,
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(
        top_frame_is_success(&trace),
        "top-level frame should be Success for {target}; trace: {trace:#?}",
    );
    let _ = session.shutdown();
    Ok(())
}

/// Solmate FixedPointMathLibTest::testMulWadDown — pure fixed-point
/// arithmetic with no cheatcodes. FixedPointMathLibTest init bytecode is only
/// ~7 kB, making it ideal for a fast pure-math test.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn solmate_fixed_point_math_mul_wad() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("solmate");

    let target = "FixedPointMathLibTest::testMulWadDown";
    let session = edb::cmd::test::run_foundry_test_for_test(
        target,
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;
    let trace = session.fetch_trace().await?;

    assert!(
        top_frame_is_success(&trace),
        "top-level frame should be Success for {target}; trace: {trace:#?}",
    );
    let _ = session.shutdown();
    Ok(())
}
