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

pub mod cheats;
pub mod discover;
pub mod entrypoint;
pub mod project;
pub mod synth;

use eyre::Result;

/// Run a single Foundry test function inside the EDB debugger.
///
/// Currently only fork-free tests (no `--fork-url`) are supported.
pub async fn run_foundry_test(
    target: &str,
    root: Option<&str>,
    profile: Option<&str>,
    fork_url: Option<&str>,
    _fork_block_number: Option<u64>,
    cli: &crate::Cli,
) -> Result<()> {
    if fork_url.is_some() {
        eyre::bail!("--fork-url is not yet implemented; coming in a later phase");
    }

    let project_ctx = project::resolve_project(root, profile)?;
    tracing::info!("Resolved project at {}", project_ctx.root.display());

    let project = project_ctx.config.project().map_err(|e| eyre::eyre!("project setup: {e}"))?;
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

    let fork_result = synth::build_clean_fork_result(
        compiled_entry.deployed_bytecode.clone(),
        &resolved.contract_name,
        &resolved.test_function,
        compiled_entry.run_selector,
    )?;

    let engine_config = edb_engine::EngineConfig::default().with_quick_mode(cli.quick);
    let engine = edb_engine::Engine::new(engine_config);

    let cheats_config =
        cheats::CheatsConfig { project_root: project_ctx.root.clone(), upstream_rpc: None };
    let cheats_factory = cheats::build_cheats_factory(cheats_config);

    let rpc_server_addr = engine
        .prepare_with_router_and_cheats::<_, cheats::EdbCheatcodes<foundry_evm_core::evm::EthEvmNetwork>>(
            fork_result,
            None,
            Some(edb_web::router()),
            Some(Box::new(cheats_factory)),
            None,
        )
        .await?;

    crate::utils::launch_ui_and_wait(cli, rpc_server_addr).await?;

    let tx_hash = synth::synthetic_tx_hash(&resolved.contract_name, &resolved.test_function);
    let _ = engine.shutdown_rpc_server(&tx_hash);
    Ok(())
}
