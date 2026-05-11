// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! E2E integration tests against vendored real-world Foundry projects in
//! `testdata/foundry-e2e/`.
//!
//! These projects are gitignored and fetched on-demand by
//! `scripts/fetch-e2e-foundry-projects.sh`; the tests are therefore
//! `#[ignore]`-gated by default. Run with:
//!
//! ```sh
//! ./scripts/fetch-e2e-foundry-projects.sh
//! cargo test --package edb-integration-tests \
//!     --test foundry_test_e2e_real_world -- --ignored
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(foundry_e2e_realworld)]
#[ignore = "requires external fixtures; run with --ignored"]
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
#[ignore = "requires external fixtures; run with --ignored"]
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
