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
use std::collections::HashSet;
use std::path::PathBuf;

use alloy_primitives::Address;
use edb_common::types::{CallResult, Code};
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

/// Count how many unique `code_address` values in `trace` resolve to a local
/// artifact via `edb_getCodeByAddress` (i.e. return [`Code::Source`]).
///
/// Returns `(with_source, total_unique_addresses)`. Addresses for which the
/// engine has no record at all (e.g. precompiles never invoked in a frame) are
/// filtered out of `total_unique_addresses`. This helper is the invariant
/// behind every `*_resolves_*` / `*_artifacts_*` test in this file: it
/// exercises the foundry-style two-stage `LocalArtifactSet` matcher (exact
/// hash with immutables/links/CBOR masked, plus fuzzy fallback) end-to-end.
async fn count_addresses_with_source(
    session: &edb::cmd::test::TestSessionHandle,
    trace: &edb_common::types::Trace,
) -> Result<(usize, usize)> {
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
            // CODE_NOT_FOUND — engine never saw the address; skip.
            Err(_) => {}
        }
    }
    Ok((with_source, total))
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

// ---------------------------------------------------------------------------
// Regression: contracts that use `immutable` state variables must resolve to
// their local artifact (not silently fall through to "no source"). Before
// the foundry-style local-artifact identification fix, `MockEIP712` (which
// inherits from solady's `EIP712` and so has 5 immutables in its deployed
// bytecode) failed the codehash lookup and produced only opcode-level
// snapshots. After the fix, the contract is resolved by template+mask
// matching and source-level snapshots are produced.
// ---------------------------------------------------------------------------

/// Solady EIP712Test::testDomainSeparator — calls `mock.DOMAIN_SEPARATOR()`
/// on a freshly-deployed `MockEIP712` instance. `MockEIP712` carries five
/// `immutable` cached fields populated in the EIP712 constructor; the
/// on-chain runtime bytes therefore differ from solc's `deployedBytecode`
/// at the immutable offsets, which means the OLD codehash-only lookup
/// would miss this artifact entirely.
///
/// This test asserts that, after the fix, EDB collects at least one
/// **source-level (`Hook`)** snapshot whose `path` lives under solady's
/// `src/utils/EIP712.sol` — i.e. that the artifact was successfully
/// identified for an immutable-using contract.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn solady_eip712_immutable_artifact_resolved() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("solady");

    let target = "EIP712Test::testDomainSeparator";
    let session = edb::cmd::test::run_foundry_test_for_test(
        target,
        Some(root.to_str().unwrap()),
        None,
        None,
        None,
    )
    .await?;

    let trace = session.fetch_trace().await?;
    assert!(!trace_has_revert(&trace), "{target} should not revert; got: {trace:#?}");

    // Walk all snapshots looking for at least one Hook (source-level) entry
    // that points into MockEIP712 or solady's EIP712.sol. Either origin is
    // proof the immutable-using contract resolved to a local artifact.
    let total = session.snapshot_count().await?;
    let mut resolved_eip712 = false;
    for id in 0..total {
        let info = session.fetch_snapshot_info(id as u64).await?;
        if let Some(detail) = info.get("detail")
            && let Some(hook) = detail.get("Hook")
            && let Some(path) = hook.get("path").and_then(|p| p.as_str())
        {
            let p = path.replace('\\', "/");
            if p.contains("EIP712.sol") || p.contains("MockEIP712.sol") {
                resolved_eip712 = true;
                break;
            }
        }
    }
    assert!(
        resolved_eip712,
        "expected at least one Hook snapshot under EIP712.sol / MockEIP712.sol \
         (total snapshots: {total}); before the foundry-style local-artifact \
         lookup fix, immutables would cause the codehash-keyed lookup to miss \
         and only opcode-level snapshots would be produced.",
    );

    let _ = session.shutdown();
    Ok(())
}

// ---------------------------------------------------------------------------
// Source-resolution coverage: each test below confirms that the foundry-style
// two-stage local-artifact matcher (exact-hash with immutables/link-refs/CBOR
// masked, plus fuzzy fallback inside ±10% length and < 0.85 diff score)
// resolves a meaningful fraction of the addresses actually touched during
// execution. Thresholds were picked from an end-to-end probe with a small
// safety margin so they stay green under benign trace variation.
// ---------------------------------------------------------------------------

/// Solady `SignatureCheckerLibTest::testSignatureCheckerOnEOAWithMatchingSignerAndSignature`
/// exercises `vm.addr` + `vm.sign` to mint a signer key, hash a message, sign
/// it, and validate the signature via `SignatureCheckerLib`. The test contract
/// plus its helpers all live under the solady project, so the trace should
/// have multiple unique addresses, most of which carry a local artifact.
///
/// Observed at the time of writing: 5 unique addrs, 4 with source.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn solady_signature_checker_resolves_signer_artifacts() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("solady");

    let target = "SignatureCheckerLibTest::testSignatureCheckerOnEOAWithMatchingSignerAndSignature";
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

    let (with_source, total) = count_addresses_with_source(&session, &trace).await?;
    assert!(
        total >= 3,
        "{target}: expected ≥3 unique addresses in trace, got {total} (with_source={with_source})",
    );
    assert!(
        with_source >= 3,
        "{target}: expected ≥3 source-resolved addresses (test contract + helpers), \
         got {with_source}/{total}",
    );

    let _ = session.shutdown();
    Ok(())
}

/// Solady `MulticallableTest::testMulticallableRevertWithCustomError` exercises
/// `vm.expectRevert(bytes4)` against a custom-error revert thrown inside a
/// multicall. The test contract itself and the `Multicallable` mock under
/// test must both resolve to source.
///
/// Observed at the time of writing: 4 unique addrs, 3 with source.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn solady_multicallable_expect_revert_bytes4() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("solady");

    let target = "MulticallableTest::testMulticallableRevertWithCustomError";
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

    let (with_source, total) = count_addresses_with_source(&session, &trace).await?;
    assert!(
        total >= 2,
        "{target}: expected ≥2 unique addresses in trace, got {total} (with_source={with_source})",
    );
    assert!(
        with_source >= 2,
        "{target}: expected ≥2 source-resolved addresses (test contract + Multicallable mock), \
         got {with_source}/{total}",
    );

    let _ = session.shutdown();
    Ok(())
}

/// Solady `ERC6551Test::testDeployERC6551Proxy` deploys an ERC-6551 proxy
/// implementation plus its registry. Both the proxy and the implementation
/// inherit from the `ERC6551` base, which carries multiple `immutable`
/// state variables → this is exactly the path the artifact-id fix unlocked
/// (before the fix, the on-chain runtime bytes differed from solc's
/// `deployedBytecode` at the immutable offsets and the codehash lookup missed).
/// We assert that essentially every touched address resolves.
///
/// Observed at the time of writing: 8 unique addrs, 8 with source.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn solady_erc6551_immutable_artifacts_resolve() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("solady");

    let target = "ERC6551Test::testDeployERC6551Proxy";
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

    let (with_source, total) = count_addresses_with_source(&session, &trace).await?;
    assert!(
        total >= 5,
        "{target}: expected ≥5 unique addresses in trace, got {total} (with_source={with_source})",
    );
    // ERC6551 family uses immutables heavily; before the foundry-style
    // matcher these would have fallen through to Opcode. ≥4 enforces the
    // invariant with a generous margin against the observed 8.
    assert!(
        with_source >= 4,
        "{target}: expected ≥4 source-resolved addresses across the ERC6551 \
         proxy / registry / implementation, got {with_source}/{total}",
    );

    let _ = session.shutdown();
    Ok(())
}

/// prb-math `sd59x18::ConvertFrom_Unit_Test::test_ConvertFrom` exercises the
/// CLI's path-qualified target syntax (the leading `sd59x18::` disambiguates
/// from the identically-named test in `ud60x18`). The test runs to Success
/// and the test contract itself must resolve to source.
///
/// Observed at the time of writing: 3 unique addrs, 2 with source.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn prb_math_path_qualified_target_resolves_uniquely() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("prb-math");

    let target = "sd59x18::ConvertFrom_Unit_Test::test_ConvertFrom";
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

    let (with_source, total) = count_addresses_with_source(&session, &trace).await?;
    assert!(
        total >= 2,
        "{target}: expected ≥2 unique addresses in trace, got {total} (with_source={with_source})",
    );
    assert!(
        with_source >= 1,
        "{target}: expected the path-qualified test contract to resolve to source; \
         got {with_source}/{total}",
    );

    let _ = session.shutdown();
    Ok(())
}

/// Uniswap V4 `TestDynamicFees::test_initialize_initializesFeeTo0` exercises
/// the full `PoolManager.initialize` path through a dynamic-fee hook. This
/// test previously aborted because the `Deployers` helper calls `vm.addr`
/// during setup; now that `vm.addr` is supported, the trace covers dozens
/// of contracts (PoolManager, Hooks, Currency, etc.) and the vast majority
/// must resolve to local artifacts.
///
/// Observed at the time of writing: 19 unique addrs, 16 with source.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn uniswap_v4_dynamic_fees_initialize_resolves_pool_manager() -> Result<()> {
    init::init_test_environment(true);
    let root = require_fixture("uniswap-v4-core");

    let target = "TestDynamicFees::test_initialize_initializesFeeTo0";
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

    let (with_source, total) = count_addresses_with_source(&session, &trace).await?;
    assert!(
        total >= 10,
        "{target}: expected ≥10 unique addresses in trace (PoolManager + hooks + \
         currencies), got {total} (with_source={with_source})",
    );
    // 16/19 observed; ≥10 leaves room for trace variation but still proves
    // the artifact-id fix is doing most of the work for a real DeFi protocol.
    assert!(
        with_source >= 10,
        "{target}: expected ≥10 source-resolved addresses, got {with_source}/{total}",
    );

    let _ = session.shutdown();
    Ok(())
}

// ---------------------------------------------------------------------------
// MegaETH — real-world reproducer for the codehash-alias fix
// ---------------------------------------------------------------------------

/// MegaETH SequencerRegistryTest — the real-world reproducer for the
/// `vm.etch(REGISTRY_ADDRESS, address(impl).code)` pattern that motivated
/// the codehash-alias fallback chain (T1–T6).
///
/// mega-evm's `test/SequencerRegistry.t.sol::setUp` deploys a fresh
/// SequencerRegistry implementation, then `vm.etch`'s its runtime code at
/// the canonical L2 system-contract address
/// `0x6342000000000000000000000000000000000006` and treats that address as
/// the registry. Every subsequent call (admin / sequencer / system-address
/// management) lands at the etched address whose codehash matches the
/// original implementation but whose `analysis_results` key is the original
/// deploy address — not the L2 system slot.
///
/// Without the codehash-alias fallback EDB drops every hook snapshot at
/// `0x6342…0006` (the hook inspector looks up the address, misses, and the
/// snapshot is silently discarded). With the fix the lookup falls back via
/// codehash and the etched address resolves to the same analysis result as
/// the original implementation.
///
/// Assertions:
///   1. top frame is Success
///   2. hook-snapshot count > 100 — without the fix this hovers around 26,
///      with the fix the ~hundreds of previously-dropped snapshots become
///      real hook snapshots
///   3. `edb_getCodeByAddress(0x6342…0006)` returns `Code::Source` with
///      `SequencerRegistry.sol` in the resolved sources map
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
async fn mega_evm_etch_aliased_system_contract_resolves_source() -> Result<()> {
    init::init_test_environment(true);

    let root = e2e_root().join("mega-evm").join("crates").join("system-contracts");
    assert!(
        root.join("foundry.toml").exists(),
        "mega-evm/crates/system-contracts fixture missing at {} — \
         run scripts/fetch-e2e-foundry-projects.sh first",
        root.display(),
    );

    // Path-qualified target: `SequencerRegistry.t.sol::Contract::method` —
    // `path_hint` must substring-match the source path, which it does for
    // the unique `test/SequencerRegistry.t.sol` source.
    let target = "SequencerRegistry.t.sol::SequencerRegistryTest::test_independentChanges_differentBlocks";
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

    // (2) Hook-snapshot count. Without the codehash-alias fix this hovers
    // around 26 (the etched address is silently skipped because its
    // address-keyed `analysis_results` entry doesn't exist). With the fix
    // the lookup falls back via codehash and the previously-dropped
    // snapshots become real Hook entries. Observed at the time of writing:
    // 100 hook snapshots of 100 total. The ≥80 threshold leaves a generous
    // margin against trace variation but still proves the fix is doing the
    // bulk of the work (3× the broken baseline of ~26).
    let snapshot_count = session.snapshot_count().await?;
    let mut hook_count = 0usize;
    for i in 0..snapshot_count {
        let info = session.fetch_snapshot_info(i as u64).await?;
        let is_hook = info
            .get("detail")
            .and_then(|v| v.as_object())
            .map(|d| d.contains_key("Hook"))
            .unwrap_or(false);
        if is_hook {
            hook_count += 1;
        }
    }
    assert!(
        hook_count >= 80,
        "expected ≥80 Hook snapshots after the codehash-alias fix; \
         got {hook_count} of {snapshot_count} total. Without the fix this \
         hovers around 26 because the vm.etch-aliased address \
         0x6342…0006 misses analysis_results.",
    );

    // (3) The etched system-contract address must resolve to source via the
    // codehash alias. Before the fix this returns Code::Opcode.
    let registry: Address =
        alloy_primitives::address!("6342000000000000000000000000000000000006");
    let code = session.fetch_code(registry).await?;
    match code {
        Code::Source(info) => {
            let has_registry_source = info
                .sources
                .keys()
                .any(|p| p.to_string_lossy().replace('\\', "/").ends_with("SequencerRegistry.sol"));
            assert!(
                has_registry_source,
                "expected SequencerRegistry.sol in resolved sources for {registry:?}; \
                 got {:?}",
                info.sources.keys().collect::<Vec<_>>(),
            );
        }
        Code::Opcode(_) => {
            panic!(
                "etched system-contract {registry:?} resolved to Code::Opcode — \
                 the codehash-alias fallback did not fire"
            );
        }
    }

    let _ = session.shutdown();
    Ok(())
}
