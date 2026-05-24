// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! `edb test` — Foundry test debugging command.

pub mod artifacts;
pub mod cheats;
pub mod discover;
pub mod entrypoint;
mod link;
pub mod project;
pub mod synth;

#[cfg(feature = "test-harness")]
pub mod harness;

#[cfg(feature = "test-harness")]
pub use harness::{TestSessionHandle, run_foundry_test_for_test};

use std::time::Duration;

use edb_common::types::{CallResult, Trace};
use eyre::Result;

/// EDB error prefix used in revert messages from unsupported cheatcodes.
const EDB_REJECTION_PREFIX: &str = "EDB: cheatcode vm.";

/// Classify a completed trace into a coverage status string.
///
/// Returns one of: `"ok"`, `"edb-rejected"`, `"test-revert"`, `"unknown"`.
fn classify_trace(trace: &Trace) -> &'static str {
    // Find the top-level frame (depth 0, no parent).
    let top = trace.iter().find(|e| e.parent_id.is_none());
    let Some(top) = top else {
        return "unknown";
    };

    let is_success = matches!(top.result, Some(CallResult::Success { .. }));
    if is_success {
        return "ok";
    }

    // Check every revert in the trace for the EDB rejection prefix.
    let has_edb_rejection = trace.iter().any(|entry| {
        if let Some(CallResult::Revert { output, .. }) = &entry.result {
            // Try to ABI-decode as Error(string) (4-byte selector 0x08c379a0 + offset + len + data)
            if output.len() >= 4 + 32 + 32
                && output[0] == 0x08
                && output[1] == 0xc3
                && output[2] == 0x79
                && output[3] == 0xa0
            {
                // offset is always 32, skip to length word
                let len_start = 4 + 32;
                let len = u64::from_be_bytes(
                    output[len_start + 24..len_start + 32].try_into().unwrap_or([0u8; 8]),
                ) as usize;
                let data_start = len_start + 32;
                if output.len() >= data_start + len
                    && let Ok(s) = std::str::from_utf8(&output[data_start..data_start + len])
                    && s.starts_with(EDB_REJECTION_PREFIX)
                {
                    return true;
                }
            }
            // Also check raw UTF-8 in case the revert reason is a bare string.
            if let Ok(s) = std::str::from_utf8(output)
                && s.contains(EDB_REJECTION_PREFIX)
            {
                return true;
            }
        }
        false
    });

    if has_edb_rejection {
        return "edb-rejected";
    }

    // Top frame reverted and no EDB prefix → real test failure.
    if matches!(top.result, Some(CallResult::Revert { .. })) {
        return "test-revert";
    }

    "unknown"
}

/// Fetch the execution trace from the engine's local RPC server and compute a
/// one-line JSON summary. Used by `--no-ui` mode.
async fn fetch_and_print_no_ui_summary(rpc_addr: std::net::SocketAddr, target: &str) -> Result<()> {
    let url = format!("http://{rpc_addr}/");
    let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;

    // Fetch trace
    let resp: serde_json::Value = client
        .post(&url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "edb_getTrace",
            "params": [],
        }))
        .send()
        .await?
        .json()
        .await?;
    let trace: Trace = serde_json::from_value(
        resp.get("result").cloned().ok_or_else(|| eyre::eyre!("edb_getTrace missing result"))?,
    )?;

    // Fetch snapshot count
    let resp2: serde_json::Value = client
        .post(&url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "edb_getSnapshotCount",
            "params": [],
        }))
        .send()
        .await?
        .json()
        .await?;
    let snapshot_count: usize =
        resp2.get("result").and_then(|v| v.as_u64()).map(|n| n as usize).unwrap_or(0);

    let status = classify_trace(&trace);
    let revert_count =
        trace.iter().filter(|e| matches!(e.result, Some(CallResult::Revert { .. }))).count();
    let edb_rejection_count = if status == "edb-rejected" { 1usize } else { 0 };

    let summary = serde_json::json!({
        "target": target,
        "status": status,
        "snapshots": snapshot_count,
        "trace_entries": trace.len(),
        "reverts": revert_count,
        "edb_rejections": edb_rejection_count,
    });
    println!("{summary}");
    Ok(())
}

/// Run a single Foundry test function inside the EDB debugger.
///
/// Supports both fork-free tests and forked tests (via `--fork-url` or
/// `foundry.toml`'s `eth_rpc_url`).
pub async fn run_foundry_test(
    target: &str,
    root: Option<&str>,
    profile: Option<&str>,
    fork_url: Option<&str>,
    fork_block_number: Option<u64>,
    no_ui: bool,
    cli: &crate::Cli,
) -> Result<()> {
    let project_ctx = project::resolve_project(root, profile)?;
    let resolved_fork_url = project::resolve_fork_url(fork_url, &project_ctx);
    tracing::info!("Resolved project at {}", project_ctx.root.display());

    let mut project =
        project_ctx.config.project().map_err(|e| eyre::eyre!("project setup: {e}"))?;
    // EDB needs AST in the compile output to drive source-level analysis /
    // instrumentation. `foundry-config`'s default `extra_output` doesn't request
    // AST, so we widen the selection here.
    project.update_output_selection(|sel| {
        *sel = foundry_compilers::artifacts::output_selection::OutputSelection::complete_output_selection();
    });
    let compile_output = project.compile().map_err(|e| eyre::eyre!("compile: {e}"))?;
    if compile_output.has_compiler_errors() {
        eyre::bail!("compilation errors:\n{compile_output}");
    }
    let compile_output = compile_output.with_stripped_file_prefixes(&project_ctx.root);

    let resolved = discover::resolve_test(target, &project_ctx.root, &compile_output, &Default::default())?;
    tracing::info!(
        "Resolved {}::{} (setUp={}, solc={})",
        resolved.contract_name,
        resolved.test_function,
        resolved.has_setup,
        resolved.compiler_version,
    );

    // Canonicalize both paths before computing the relative import path so that
    // symlinks (e.g. macOS /tmp → /private/tmp) do not produce spurious `../../`
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

    match resolved_fork_url {
        Some(upstream) => {
            tracing::info!("Forking from {upstream} at block {:?}", fork_block_number);
            let fork_result = synth::build_forked_fork_result(
                compiled_entry.deployed_bytecode.clone(),
                &resolved.contract_name,
                &resolved.test_function,
                compiled_entry.run_selector,
                &upstream,
                fork_block_number,
            )
            .await?;
            drive_engine_with_fork_result(
                fork_result,
                &project_ctx,
                &compile_output,
                &compiled_entry,
                &resolved,
                no_ui,
                cli,
            )
            .await
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
            drive_engine_with_fork_result(
                fork_result,
                &project_ctx,
                &compile_output,
                &compiled_entry,
                &resolved,
                no_ui,
                cli,
            )
            .await
        }
    }
}

/// Wire a `ForkResult<DB>` through the full engine pipeline: cheats factory,
/// local artifact set, `prepare_with_router_and_cheats`, UI launch, shutdown.
///
/// Shared by both the fork-free and forked paths in `run_foundry_test`.
/// When `no_ui` is `true`, skips the UI and instead prints a one-line JSON
/// summary to stdout, then shuts down immediately.
async fn drive_engine_with_fork_result<DB>(
    fork_result: edb_common::ForkResult<DB>,
    project_ctx: &project::ResolvedProject,
    compile_output: &foundry_compilers::ProjectCompileOutput,
    compiled_entry: &entrypoint::CompiledEntrypoint,
    resolved: &discover::ResolvedTest,
    no_ui: bool,
    cli: &crate::Cli,
) -> Result<()>
where
    DB: revm::Database + revm::DatabaseCommit + revm::DatabaseRef + Clone + Send + Sync + 'static,
    <revm::database::CacheDB<DB> as revm::Database>::Error: Clone + Send + Sync,
    <DB as revm::Database>::Error: Clone + Send + Sync,
{
    let local_artifacts = artifacts::build_local_artifact_set(
        compile_output,
        &compiled_entry.deployed_bytecode,
        compiled_entry.artifact.clone(),
        &project_ctx.root,
        &Default::default(),
    )?;

    let engine_config = edb_engine::EngineConfig::default().with_quick_mode(cli.quick);
    let engine = edb_engine::Engine::new(engine_config);

    // Wrap the artifact set in an Arc so it can be shared with the cheats
    // inspector (for `vm.getDeployedCode`) AND moved into the engine without
    // a deep clone. We hand the engine `(*Arc).clone()` (full owned clone, as
    // the engine signature demands), but the cheats handlers only read from
    // the shared Arc — same data, two views.
    let local_artifacts_arc = std::sync::Arc::new(local_artifacts.clone());

    // Bind the cheats config to a name so the Arc-shared hit/warning trackers
    // remain accessible AFTER `prepare_with_router_and_cheats` returns. Each
    // factory invocation captures a clone of the config, but the inner Arcs
    // point at the SAME tracker — so every pass (tracer/opcode/hook) writes
    // into the same `unsupported_hits` list.
    let cheats_config = cheats::CheatsConfig {
        project_root: project_ctx.root.clone(),
        local_artifacts: Some(local_artifacts_arc.clone()),
        ..cheats::CheatsConfig::default()
    };
    let cheats_factory: Box<dyn Fn() -> cheats::EdbCheatcodes<DB> + Send + Sync> =
        Box::new(cheats::build_cheats_factory::<DB>(cheats_config.clone()));

    // Build a between-passes hook that short-circuits preparation the moment
    // an unsupported cheatcode is observed. The closure captures only an Arc
    // clone so there's no borrow across the await point.
    let hits_for_hook = cheats_config.unsupported_hits.clone();
    let between_passes_hook: Box<dyn Fn() -> eyre::Result<()> + Send + Sync> =
        Box::new(move || cheats::ensure_no_unsupported_hits(&hits_for_hook));

    let rpc_server_addr = engine
        .prepare_with_router_and_cheats::<_, cheats::EdbCheatcodes<DB>>(
            fork_result,
            None,
            Some(edb_web::router()),
            Some(cheats_factory),
            Some(local_artifacts),
            Some(between_passes_hook),
        )
        .await?;

    let target = format!("{}::{}", resolved.contract_name, resolved.test_function);
    let tx_hash = synth::synthetic_tx_hash(&resolved.contract_name, &resolved.test_function);

    // Defensive sanity: if prepare somehow returned Ok despite unsupported hits
    // (shouldn't happen after the between-passes hook fires), abort here too.
    if let Some(err_msg) = check_unsupported_hits(&cheats_config) {
        let _ = engine.shutdown_rpc_server(&tx_hash);
        eyre::bail!("{err_msg}");
    }

    if no_ui {
        fetch_and_print_no_ui_summary(rpc_server_addr, &target).await?;
        let _ = engine.shutdown_rpc_server(&tx_hash);
        return Ok(());
    }

    crate::utils::launch_ui_and_wait(cli, rpc_server_addr).await?;

    let _ = engine.shutdown_rpc_server(&tx_hash);
    Ok(())
}

/// Check the shared `unsupported_hits` tracker and, if non-empty, return a
/// human-readable abort message. Returns `None` when prepare succeeded
/// without any unsupported-cheatcode invocation.
///
/// Delegates to [`cheats::ensure_no_unsupported_hits`] so the formatting
/// logic lives in one place.
pub(crate) fn check_unsupported_hits(config: &cheats::CheatsConfig) -> Option<String> {
    cheats::ensure_no_unsupported_hits(&config.unsupported_hits).err().map(|e| e.to_string())
}
