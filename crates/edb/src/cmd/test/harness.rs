// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! Programmatic test-harness API for `edb test`.
//!
//! This module is gated behind the `test-harness` Cargo feature on the `edb`
//! crate and is intended exclusively for use by integration tests (see
//! `crates/integration-tests/tests/foundry_test_*.rs`). It duplicates a small
//! amount of the body of [`run_foundry_test`](super::run_foundry_test) so that
//! tests can:
//!
//! - run the same compile / synthetic-entrypoint / cheats / local-artifact /
//!   engine-preparation pipeline,
//! - skip launching the TUI / browser UI,
//! - inspect the resulting RPC server (trace / snapshots / etc.) via a
//!   minimal JSON-RPC client,
//! - shut the engine down deterministically.
//!
//! The mild duplication with `run_foundry_test` is intentional — sharing more
//! would require an architecture pass that's out of scope for this task.

use eyre::Result;
use std::net::SocketAddr;
use std::time::Duration;

use alloy_primitives::{Address, TxHash};
use edb_common::types::{Code, Trace};
use edb_engine::{Engine, EngineConfig};
use foundry_compilers::artifacts::output_selection::OutputSelection;
use serde_json::Value;

use super::{artifacts, cheats, discover, entrypoint, project, synth};

/// Live programmatic test session. Wraps the [`Engine`] that's been driven
/// through the full preparation pipeline plus a handle to the RPC server's
/// `SocketAddr`. Created via [`run_foundry_test_for_test`].
///
/// Callers should invoke [`Self::shutdown`] when done to release the RPC
/// server. Dropping the handle without shutting down leaks the listener.
pub struct TestSessionHandle {
    engine: Engine,
    rpc_addr: SocketAddr,
    tx_hash: TxHash,
}

impl TestSessionHandle {
    /// Address of the engine's JSON-RPC server.
    pub fn rpc_addr(&self) -> SocketAddr {
        self.rpc_addr
    }

    /// Synthetic tx hash this session is registered under.
    pub fn tx_hash(&self) -> TxHash {
        self.tx_hash
    }

    /// Issue a JSON-RPC request to the engine's RPC server and return the raw
    /// `result` field. Errors if the response carries a non-null `error`
    /// field.
    async fn call_rpc(&self, method: &str, params: Value) -> Result<Value> {
        let url = format!("http://{}/", self.rpc_addr);
        let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;
        let resp: Value = client
            .post(&url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": params,
            }))
            .send()
            .await?
            .json()
            .await?;
        if let Some(err) = resp.get("error").filter(|e| !e.is_null()) {
            eyre::bail!("RPC {method} returned error: {err}");
        }
        resp.get("result")
            .cloned()
            .ok_or_else(|| eyre::eyre!("RPC {method} response missing `result`: {resp}"))
    }

    /// Fetch the complete execution trace from the engine via `edb_getTrace`.
    pub async fn fetch_trace(&self) -> Result<Trace> {
        let result = self.call_rpc("edb_getTrace", serde_json::json!([])).await?;
        let trace: Trace = serde_json::from_value(result)?;
        Ok(trace)
    }

    /// Fetch the total number of snapshots collected during execution
    /// via `edb_getSnapshotCount`.
    pub async fn snapshot_count(&self) -> Result<usize> {
        let result = self.call_rpc("edb_getSnapshotCount", serde_json::json!([])).await?;
        result
            .as_u64()
            .map(|n| n as usize)
            .ok_or_else(|| eyre::eyre!("edb_getSnapshotCount result not a number: {result}"))
    }

    /// Fetch a single snapshot's detail (opcode-level or hook-level) via
    /// `edb_getSnapshotInfo`. Returns the raw JSON so callers can pick which
    /// variant fields to inspect without depending on internal types.
    pub async fn fetch_snapshot_info(&self, id: u64) -> Result<Value> {
        self.call_rpc("edb_getSnapshotInfo", serde_json::json!([id])).await
    }

    /// Fetch the resolved code for a specific address via `edb_getCodeByAddress`.
    ///
    /// Returns [`Code::Source`] when the address resolved to a local artifact
    /// (i.e. the foundry-style two-stage matcher in `LocalArtifactSet`
    /// produced a hit), or [`Code::Opcode`] when only the raw deployed
    /// bytecode is available.
    ///
    /// Returns an error if the engine has never seen the address (i.e. the
    /// underlying RPC returned `CODE_NOT_FOUND`).
    pub async fn fetch_code(&self, address: Address) -> Result<Code> {
        let result = self.call_rpc("edb_getCodeByAddress", serde_json::json!([address])).await?;
        let code: Code = serde_json::from_value(result)?;
        Ok(code)
    }

    /// Shut down the RPC server associated with this session. Idempotent
    /// (returns `Ok(false)` if the server was already shut down).
    pub fn shutdown(self) -> Result<bool> {
        self.engine.shutdown_rpc_server(&self.tx_hash).map_err(|e| eyre::eyre!("{e}"))
    }
}

/// Programmatic equivalent of `edb test` for integration tests.
///
/// Runs the same compile + entrypoint-synthesis + cheats + local-artifact +
/// engine-preparation pipeline as the binary, but returns a
/// [`TestSessionHandle`] instead of launching the UI. Caller is responsible
/// for `.shutdown()`ing the handle.
pub async fn run_foundry_test_for_test(
    target: &str,
    root: Option<&str>,
    profile: Option<&str>,
    fork_url: Option<&str>,
    fork_block_number: Option<u64>,
) -> Result<TestSessionHandle> {
    let project_ctx = project::resolve_project(root, profile)?;
    let resolved_fork_url = project::resolve_fork_url(fork_url, &project_ctx);

    let mut project =
        project_ctx.config.project().map_err(|e| eyre::eyre!("project setup: {e}"))?;
    project.update_output_selection(|sel| {
        *sel = OutputSelection::complete_output_selection();
    });
    let compile_output = project.compile().map_err(|e| eyre::eyre!("compile: {e}"))?;
    if compile_output.has_compiler_errors() {
        eyre::bail!("compilation errors:\n{compile_output}");
    }
    let compile_output = compile_output.with_stripped_file_prefixes(&project_ctx.root);

    let resolved = discover::resolve_test(target, &project_ctx.root, &compile_output, &Default::default())?;

    // Canonicalize both paths before computing the relative import path so that
    // symlinks (e.g. macOS /tmp → /private/tmp) don't produce spurious `../../`
    // prefixes in the generated entrypoint `import` statement.
    let canonical_artifact =
        resolved.artifact_path.canonicalize().unwrap_or_else(|_| resolved.artifact_path.clone());
    let canonical_root =
        project_ctx.root.canonicalize().unwrap_or_else(|_| project_ctx.root.clone());
    let test_source_rel = pathdiff::diff_paths(&canonical_artifact, &canonical_root)
        .unwrap_or_else(|| resolved.artifact_path.clone());

    let compiled_entry = entrypoint::compile_entrypoint(
        &resolved.contract_name,
        &resolved.test_function,
        resolved.has_setup,
        &resolved.compiler_version,
        &project_ctx.root,
        &test_source_rel,
    )?;

    let local_artifacts = artifacts::build_local_artifact_set(
        &compile_output,
        &compiled_entry.deployed_bytecode,
        compiled_entry.artifact.clone(),
        &project_ctx.root,
        &Default::default(),
    )?;

    let engine_config = EngineConfig::default();
    let engine = Engine::new(engine_config);

    // Wrap the artifact set in an Arc for sharing with the cheats inspector
    // (specifically the `vm.getDeployedCode` handler) — same pattern as in
    // `run_foundry_test`. The engine still receives a full owned clone via
    // `(*Arc).clone()`; the cheats handlers read through the Arc.
    let local_artifacts_arc = std::sync::Arc::new(local_artifacts.clone());

    // Same Arc-share pattern as `run_foundry_test`: keep a binding to
    // `cheats_config` alive so we can inspect the shared hit tracker post-
    // prepare. Each `.clone()` here shares the inner Arcs, NOT a deep copy.
    let cheats_config = cheats::CheatsConfig {
        project_root: project_ctx.root.clone(),
        local_artifacts: Some(local_artifacts_arc.clone()),
        ..cheats::CheatsConfig::default()
    };

    let rpc_addr = match resolved_fork_url {
        Some(upstream) => {
            let fork_result = synth::build_forked_fork_result(
                compiled_entry.deployed_bytecode.clone(),
                &resolved.contract_name,
                &resolved.test_function,
                compiled_entry.run_selector,
                &upstream,
                fork_block_number,
            )
            .await?;
            let cheats_factory = Box::new(cheats::build_cheats_factory(cheats_config.clone()));
            let hits_for_hook = cheats_config.unsupported_hits.clone();
            let between_passes_hook: Box<dyn Fn() -> eyre::Result<()> + Send + Sync> =
                Box::new(move || cheats::ensure_no_unsupported_hits(&hits_for_hook));
            engine
                .prepare_with_router_and_cheats::<_, cheats::EdbCheatcodes<_>>(
                    fork_result,
                    None,
                    None,
                    Some(cheats_factory),
                    Some(local_artifacts),
                    Some(between_passes_hook),
                )
                .await?
        }
        None => {
            if fork_block_number.is_some() {
                eyre::bail!(
                    "--fork-block-number specified but no upstream RPC configured. \
                     Either pass --fork-url or set eth_rpc_url in foundry.toml."
                );
            }
            let fork_result = synth::build_clean_fork_result(
                compiled_entry.deployed_bytecode.clone(),
                &resolved.contract_name,
                &resolved.test_function,
                compiled_entry.run_selector,
            )?;
            let cheats_factory = Box::new(cheats::build_cheats_factory(cheats_config.clone()));
            let hits_for_hook = cheats_config.unsupported_hits.clone();
            let between_passes_hook: Box<dyn Fn() -> eyre::Result<()> + Send + Sync> =
                Box::new(move || cheats::ensure_no_unsupported_hits(&hits_for_hook));
            engine
                .prepare_with_router_and_cheats::<_, cheats::EdbCheatcodes<_>>(
                    fork_result,
                    None,
                    None,
                    Some(cheats_factory),
                    Some(local_artifacts),
                    Some(between_passes_hook),
                )
                .await?
        }
    };

    let tx_hash = synth::synthetic_tx_hash(&resolved.contract_name, &resolved.test_function);

    // Mirror the abort behavior of `run_foundry_test`: if any unsupported
    // cheatcode was called during prepare, tear down and bail with a clear
    // error so integration tests see the same surface the binary does.
    if let Some(err_msg) = super::check_unsupported_hits(&cheats_config) {
        let _ = engine.shutdown_rpc_server(&tx_hash);
        eyre::bail!("{err_msg}");
    }

    Ok(TestSessionHandle { engine, rpc_addr, tx_hash })
}
