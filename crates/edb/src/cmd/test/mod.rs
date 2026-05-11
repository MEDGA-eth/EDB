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

pub mod discover;
pub mod project;

use eyre::Result;

pub async fn run_foundry_test(
    target: &str,
    root: Option<&str>,
    profile: Option<&str>,
    fork_url: Option<&str>,
    fork_block_number: Option<u64>,
    _cli: &crate::Cli,
) -> Result<()> {
    let project_ctx = project::resolve_project(root, profile)?;
    tracing::info!("Resolved project at {}", project_ctx.root.display());

    let project = project_ctx
        .config
        .project()
        .map_err(|e| eyre::eyre!("foundry project setup failed: {e}"))?;
    let compile_output =
        project.compile().map_err(|e| eyre::eyre!("foundry compile failed: {e}"))?;
    if compile_output.has_compiler_errors() {
        eyre::bail!("compilation errors:\n{compile_output}");
    }
    let compile_output = compile_output.with_stripped_file_prefixes(&project_ctx.root);

    let resolved = discover::resolve_test(target, &project_ctx.root, &compile_output)?;
    tracing::info!(
        "Resolved {}::{}  (setUp={}, solc={})",
        resolved.contract_name,
        resolved.test_function,
        resolved.has_setup,
        resolved.compiler_version,
    );

    eyre::bail!(
        "compiled + resolved (entrypoint and synthetic tx pending). fork_url={:?} block={:?}",
        fork_url,
        fork_block_number
    )
}
