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
//! Each project artifact is inserted along with its deployed-bytecode
//! template AND the immutable-references / link-references that solc emits
//! for it. The engine then uses foundry-style masked + fuzzy matching
//! (`get_by_runtime`) to resolve on-chain runtime bytecode against those
//! templates, correctly handling contracts that use `immutable` state vars,
//! linked libraries, library call-protection, or that carry slightly
//! different CBOR metadata trailers than what solc emitted locally.
//!
//! Synthetic addresses don't exist on Etherscan, and locally-compiled
//! artifacts are the ground truth we want to debug against anyway.

use alloy_primitives::Bytes;
use edb_engine::{Artifact, LocalArtifactSet};
use eyre::Result;
use foundry_compilers::ProjectCompileOutput;
use std::path::Path;
use std::sync::Arc;

/// Build an index of every contract in the project, plus the synthesized
/// entrypoint, keyed for foundry-style template matching.
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
    libraries: &foundry_compilers::artifacts::Libraries,
) -> Result<LocalArtifactSet> {
    let mut set = LocalArtifactSet::default();

    // Flatten the forge-resolved library map into `(file, name, address)`
    // link triples once. `BytecodeObject::link_all` consumes these to patch
    // the `__$<hash>$__` placeholders solc leaves for unlinked libraries.
    // With an empty map this is empty and `link_all` is a no-op.
    let link_triples: Vec<(String, String, alloy_primitives::Address)> = libraries
        .libs
        .iter()
        .flat_map(|(path, libs)| {
            let p = path.to_string_lossy().to_string();
            libs.iter().filter_map(move |(name, addr)| {
                addr.parse().ok().map(|a| (p.clone(), name.clone(), a))
            })
        })
        .collect();

    for (id, art) in output.artifact_ids() {
        // Clone the deployed bytecode object and link it against the resolved
        // libraries before extracting bytes. The old path called
        // `art.get_deployed_bytecode_bytes()`, which returns `None` for
        // contracts that still carry unresolved library placeholders — those
        // contracts were silently skipped and never debuggable. Linking first
        // means library-using contracts get indexed too.
        let mut deployed_obj =
            match art.deployed_bytecode.as_ref().and_then(|d| d.bytecode.as_ref()) {
                Some(bc) => bc.object.clone(),
                None => continue,
            };
        deployed_obj.link_all(link_triples.iter().map(|(f, n, a)| (f, n, *a)));
        let Some(bytes) = deployed_obj.into_bytes() else { continue };
        if bytes.is_empty() {
            continue;
        }

        // Extract immutable / link references for masked matching. We pull
        // them directly off `ConfigurableContractArtifact::deployed_bytecode`
        // because foundry's `Artifact::get_deployed_bytecode_bytes()` only
        // returns the raw bytes and doesn't surface the side metadata.
        let (immutable_refs, link_refs) = match &art.deployed_bytecode {
            Some(dbc) => {
                let imm = dbc.immutable_references.clone();
                let link =
                    dbc.bytecode.as_ref().map(|b| b.link_references.clone()).unwrap_or_default();
                (imm, link)
            }
            None => Default::default(),
        };

        // Solc marks libraries via the AST `contractKind` field. The simplest
        // robust proxy at the bytecode level is the call-protection prefix
        // itself: `compute_mask` already gates the 20-byte mask on the
        // template's first byte being PUSH20 (0x73), so passing `is_library = false`
        // here is safe even for libraries — the mask helper detects it
        // independently when needed. Keep this explicit anyway so future
        // readers don't think this is a bug.
        let is_library = matches!(bytes.first(), Some(&0x73)) && bytes.len() >= 21;

        let mut lifted = Artifact::from_foundry(&id, art, libraries)
            .map_err(|e| eyre::eyre!("lift artifact for {}: {e}", id.name))?;
        backfill_source_contents(&mut lifted, project_root);
        set.insert_with_template(&bytes, &immutable_refs, &link_refs, is_library, lifted);
    }

    // The synthesized entrypoint isn't part of `output` (it's compiled in a
    // separate `compile_file` pass — see `cmd::test::entrypoint`). Its
    // runtime bytecode has no immutables / linked libraries to mask, but
    // we still register it with the template so `get_by_runtime` can find
    // it (the engine resolves touched addresses by their observed runtime
    // bytes, not by a pre-computed codehash).
    let mut entrypoint_artifact = entrypoint_artifact;
    backfill_source_contents(&mut entrypoint_artifact, project_root);
    set.insert_with_template(
        entrypoint_bytecode.as_ref(),
        &Default::default(),
        &Default::default(),
        false,
        entrypoint_artifact,
    );

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
                // Normalize line endings to LF. EDB's instrumenter slices source at AST-reported
                // byte offsets; foundry-compilers may produce offsets against a normalized
                // (LF-only) source even when the file on disk uses CRLF (Windows). Forcing LF
                // here keeps the offsets and the source-bytes in lockstep.
                let s = if s.contains("\r\n") { s.replace("\r\n", "\n") } else { s };
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
