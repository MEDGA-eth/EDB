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
pub mod project;
pub mod synth;

#[cfg(feature = "test-harness")]
pub mod harness;

#[cfg(feature = "test-harness")]
pub use harness::{TestSessionHandle, run_foundry_test_for_test};

use eyre::Result;

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

    let resolved = discover::resolve_test(target, &project_ctx.root, &compile_output)?;
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
async fn drive_engine_with_fork_result<DB>(
    fork_result: edb_common::ForkResult<DB>,
    project_ctx: &project::ResolvedProject,
    compile_output: &foundry_compilers::ProjectCompileOutput,
    compiled_entry: &entrypoint::CompiledEntrypoint,
    resolved: &discover::ResolvedTest,
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
    )?;

    let engine_config = edb_engine::EngineConfig::default().with_quick_mode(cli.quick);
    let engine = edb_engine::Engine::new(engine_config);

    let cheats_config = cheats::CheatsConfig { project_root: project_ctx.root.clone() };
    let cheats_factory: Box<dyn Fn() -> cheats::EdbCheatcodes + Send + Sync> =
        Box::new(cheats::build_cheats_factory(cheats_config));

    let rpc_server_addr = engine
        .prepare_with_router_and_cheats::<_, cheats::EdbCheatcodes>(
            fork_result,
            None,
            Some(edb_web::router()),
            Some(cheats_factory),
            Some(local_artifacts),
        )
        .await?;

    crate::utils::launch_ui_and_wait(cli, rpc_server_addr).await?;

    let tx_hash = synth::synthetic_tx_hash(&resolved.contract_name, &resolved.test_function);
    let _ = engine.shutdown_rpc_server(&tx_hash);
    Ok(())
}
