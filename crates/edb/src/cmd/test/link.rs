// EDB - Ethereum Debugger
//
// External Solidity library linking for `edb test`. Computes forge-exact
// library addresses (LIBRARY_DEPLOYER @ nonce 0, via foundry-linking) and the
// linked deployed bytecode to etch at those addresses.

use alloy_primitives::{Address, Bytes, address};
use eyre::{Result, WrapErr};
use foundry_compilers::{Artifact as _, ProjectCompileOutput, artifacts::Libraries};
use foundry_linking::Linker;
use std::path::Path;

/// Address Foundry deploys test libraries from (`forge`'s `LIBRARY_DEPLOYER`).
/// Matching it makes EDB's library addresses identical to a real `forge test`.
pub const LIBRARY_DEPLOYER: Address = address!("1F95D37F27EA0dEA9C252FC09D5A6eaA97647353");

/// Result of linking a compiled project's external libraries.
#[derive(Debug, Default, Clone)]
pub struct LinkedLibraries {
    /// `<source path>:<lib name> -> address` map for feeding solc
    /// `Settings.libraries` at every compile/lift site.
    pub libraries: Libraries,
    /// `(address, linked deployed bytecode)` to `vm.etch` into the fork DB.
    pub etch: Vec<(Address, Bytes)>,
}

/// Compute forge-exact library addresses + etch list for `output`.
///
/// `starting` seeds any user-pinned libraries (from `foundry.toml [libraries]`)
/// so they keep their addresses; the rest get `LIBRARY_DEPLOYER.create(nonce)`.
/// Returns an empty result when the project uses no external libraries.
pub fn link_libraries(
    output: &ProjectCompileOutput,
    project_root: &Path,
    starting: Libraries,
) -> Result<LinkedLibraries> {
    let linker = Linker::new(project_root, output.artifact_ids().collect());

    let link_out = linker
        .link_with_nonce_or_address(starting, LIBRARY_DEPLOYER, 0, linker.contracts.keys())
        .wrap_err("library linking failed")?;

    if link_out.libraries.libs.is_empty() {
        return Ok(LinkedLibraries::default());
    }

    // Linked deployed bytecode for every artifact, so we can pull each
    // library's runtime code to etch at its computed address.
    let linked =
        linker.get_linked_artifacts(&link_out.libraries).wrap_err("resolving linked artifacts")?;

    let mut etch = Vec::new();
    for (path, libs) in &link_out.libraries.libs {
        for (name, addr_str) in libs {
            let addr: Address = addr_str.parse().wrap_err("library address parse")?;
            // Find the linked artifact whose (source, name) matches this lib.
            let bytes = linked.iter().find_map(|(id, cbc)| {
                (id.name == *name && id.source.ends_with(path))
                    .then(|| cbc.get_deployed_bytecode_bytes())
                    .flatten()
                    .map(|b| b.into_owned())
            });
            if let Some(b) = bytes
                && !b.is_empty()
            {
                etch.push((addr, b));
            }
        }
    }

    Ok(LinkedLibraries { libraries: link_out.libraries, etch })
}
