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

//! Build a [`edb_engine::LocalArtifactSet`] from a foundry
//! [`ProjectCompileOutput`] plus the synthesized entrypoint contract.
//!
//! The set is keyed by deployed-bytecode `keccak256`. It is fed into
//! [`edb_engine::Engine::prepare_with_router_and_cheats`] so the engine can
//! satisfy `load_local_artifacts` for every address touched during the test
//! transaction without an Etherscan round-trip — synthetic addresses don't
//! exist on Etherscan, and locally-compiled artifacts are the ground truth
//! we want to debug against anyway.

use alloy_primitives::{Bytes, keccak256};
use edb_engine::{Artifact, LocalArtifactSet};
use eyre::Result;
use foundry_compilers::ProjectCompileOutput;
use std::path::Path;
use std::sync::Arc;

/// Build a codehash-keyed index of every contract in the project, plus the
/// synthesized entrypoint.
///
/// Contracts whose deployed bytecode is empty (e.g. interfaces, libraries
/// with no deployable code, abstract contracts) are silently skipped — they
/// have no on-chain presence to debug.
///
/// `project_root` is used to backfill source contents that solc's metadata
/// elides (solc only embeds keccak/url for imported files by default; the
/// engine's analyzer requires full source bodies in `input.sources`).
#[allow(dead_code)] // consumed by cmd::test::run_foundry_test
pub fn build_local_artifact_set(
    output: &ProjectCompileOutput,
    entrypoint_bytecode: &Bytes,
    entrypoint_artifact: Artifact,
    project_root: &Path,
) -> Result<LocalArtifactSet> {
    use foundry_compilers::Artifact as FoundryArtifact;

    let mut set = LocalArtifactSet::default();

    // Index every contract in the project by its deployed-bytecode keccak.
    for (id, art) in output.artifact_ids() {
        let Some(bytes) = art.get_deployed_bytecode_bytes() else { continue };
        if bytes.is_empty() {
            continue;
        }
        let ch = keccak256(bytes.as_ref());
        let mut lifted = Artifact::from_foundry(&id, art)
            .map_err(|e| eyre::eyre!("lift artifact for {}: {e}", id.name))?;
        backfill_source_contents(&mut lifted, project_root);
        set.insert(ch, lifted);
    }

    // The synthesized entrypoint isn't part of `output` (it's compiled in a
    // separate `compile_file` pass — see `cmd::test::entrypoint`), so we add
    // it explicitly here.
    let mut entrypoint_artifact = entrypoint_artifact;
    backfill_source_contents(&mut entrypoint_artifact, project_root);
    let entry_ch = keccak256(entrypoint_bytecode.as_ref());
    set.insert(entry_ch, entrypoint_artifact);

    Ok(set)
}

/// Walk `art.input.sources` and, for any entry whose content is empty, attempt
/// to read the file from disk (first as an absolute path, then relative to the
/// project root). Sources that can't be located are left empty; downstream
/// analysis will skip them with a non-fatal error.
fn backfill_source_contents(art: &mut Artifact, project_root: &Path) {
    for (path, src) in art.input.sources.0.iter_mut() {
        if !src.content.is_empty() {
            continue;
        }
        let candidates = [path.clone(), project_root.join(path)];
        let mut filled = false;
        for candidate in &candidates {
            if candidate.is_file()
                && let Ok(s) = std::fs::read_to_string(candidate)
            {
                src.content = Arc::new(s);
                filled = true;
                tracing::debug!(
                    target: "edb::cmd::test::artifacts",
                    "backfilled source content for {} from {}",
                    path.display(),
                    candidate.display()
                );
                break;
            }
        }
        if !filled {
            tracing::warn!(
                target: "edb::cmd::test::artifacts",
                "could not backfill empty source content for {} (tried {:?})",
                path.display(),
                candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()
            );
        }
    }
}

// Note on testing: `ProjectCompileOutput` is non-trivial to construct in
// isolation (it expects a working foundry project on disk), and
// `Artifact::test_stub` is gated behind `#[cfg(test)]` inside the engine
// crate so it's not reachable here. Integration tests in Phase 9 give
// end-to-end coverage of the full indexing path; we deliberately rely on
// the type-check and the unit tests already covering `Artifact::from_foundry`
// and `LocalArtifactSet` insertion within the engine crate.
