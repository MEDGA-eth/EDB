// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! Synthetic entrypoint generation: builds a Solidity file that, when called,
//! deploys the user's test contract and invokes setUp + the chosen test function
//! within a single transaction.

use alloy_primitives::{Bytes, address};
use eyre::{Result, bail};

#[allow(dead_code)] // consumed by downstream tasks (4.2+)
pub const ENTRYPOINT_NAME: &str = "_EdbTestEntrypoint";
#[allow(dead_code)] // consumed by downstream tasks (4.2+)
pub const ENTRYPOINT_FILE: &str = "_EdbTestEntrypoint.sol";
#[allow(dead_code)] // consumed by downstream tasks (4.2+)
pub const ENTRYPOINT_ADDR: alloy_primitives::Address =
    address!("ED0100000000000000000000000000000000ED01");

/// Build the Solidity source for the synthetic entrypoint.
///
/// `import_path` should be a path resolvable by the project's solc remappings — typically
/// the test contract's source path relative to the project root.
pub fn generate_entrypoint_source(
    contract_name: &str,
    test_function: &str,
    has_setup: bool,
    compiler_version: &str,
    import_path: &str,
) -> String {
    // Strip leading 'v' and any build metadata suffix (e.g. "+commit.abc123"),
    // then keep only the major.minor.patch triple for the pragma constraint.
    let raw = compiler_version.trim_start_matches('v');
    let raw = raw.split('+').next().unwrap_or(raw);
    let major_minor = raw.split('.').take(3).collect::<Vec<_>>().join(".");
    let setup_call = if has_setup { "        t.setUp();\n" } else { "" };
    format!(
        r#"// SPDX-License-Identifier: UNLICENSED
pragma solidity ^{major_minor};

import "{import_path}";

contract {ENTRYPOINT_NAME} {{
    function run() external {{
        {contract_name} t = new {contract_name}();
{setup_call}        t.{test_function}();
    }}
}}
"#
    )
}

/// Result of compiling the synthetic entrypoint.
#[allow(dead_code)] // consumed by downstream tasks (4.2+)
pub struct CompiledEntrypoint {
    pub deployed_bytecode: Bytes,
    pub run_selector: [u8; 4],
    /// Foundry-lifted artifact for the synthesized entrypoint, ready to be
    /// inserted into a `LocalArtifactSet` for source-level analysis /
    /// instrumentation of the test contract.
    pub artifact: edb_engine::Artifact,
}

/// Generate and compile the synthetic entrypoint using foundry-compilers.
///
/// Writes the source into `project_root/<ENTRYPOINT_FILE>` (best-effort cleanup
/// on exit), then invokes the project's solc to compile it together with the
/// test source via standard-json.
#[allow(dead_code)] // consumed by downstream tasks (4.2+)
pub fn compile_entrypoint(
    contract_name: &str,
    test_function: &str,
    has_setup: bool,
    compiler_version: &str,
    project_root: &std::path::Path,
    test_source_relative: &std::path::Path,
) -> Result<CompiledEntrypoint> {
    let import_path = test_source_relative
        .to_str()
        .ok_or_else(|| eyre::eyre!("non-utf8 test source path: {test_source_relative:?}"))?;

    let src = generate_entrypoint_source(
        contract_name,
        test_function,
        has_setup,
        compiler_version,
        import_path,
    );

    let entrypoint_path = project_root.join(ENTRYPOINT_FILE);
    std::fs::write(&entrypoint_path, &src)?;

    // Compile via foundry-config + foundry-compilers, mirroring `cmd::test::mod::run_foundry_test`.
    let config = foundry_config::Config::load_with_root(project_root)
        .map_err(|e| eyre::eyre!("foundry-config load_with_root: {e}"))?
        .sanitized();
    let mut project = config.project().map_err(|e| eyre::eyre!("project setup: {e}"))?;
    // EDB needs AST in the compile output to drive source-level analysis;
    // request the full output selection so the lifted Artifact carries it.
    project.update_output_selection(|sel| {
        *sel = foundry_compilers::artifacts::output_selection::OutputSelection::complete_output_selection();
    });
    let output = project
        .compile_file(&entrypoint_path)
        .map_err(|e| eyre::eyre!("compile entrypoint: {e}"))?;

    if output.has_compiler_errors() {
        let _ = std::fs::remove_file(&entrypoint_path);
        bail!("entrypoint compile errors:\n{output}");
    }

    // Strip the project-root prefix so artifact ids (and `id.source`) use the
    // same relative keys that solc emits in `metadata.sources` — otherwise
    // `Artifact::from_foundry` ends up with an absolute `id.source` in
    // `output.sources` while `input.sources` (lifted from metadata) keys are
    // relative, causing analysis to fail with `MissingSource`.
    let output = output.with_stripped_file_prefixes(project_root);

    let entry = output.artifact_ids().find(|(id, _)| id.name == ENTRYPOINT_NAME);
    let Some((entry_id, artifact)) = entry else {
        let _ = std::fs::remove_file(&entrypoint_path);
        bail!("entrypoint artifact not produced (compile output had {ENTRYPOINT_NAME}?)");
    };

    use foundry_compilers::Artifact as _;
    let deployed = artifact
        .get_deployed_bytecode_bytes()
        .ok_or_else(|| eyre::eyre!("entrypoint missing deployed bytecode"))?
        .into_owned();

    // Lift the foundry artifact into EDB's `Artifact` shape so the engine can
    // run source-level analysis / instrumentation against the synthesized
    // entrypoint without an Etherscan round-trip.
    let mut lifted = edb_engine::Artifact::from_foundry(&entry_id, artifact)
        .map_err(|e| eyre::eyre!("lift entrypoint artifact: {e}"))?;
    // The entrypoint file is deleted on exit (see the cleanup below), so
    // `cmd::test::artifacts::backfill_source_contents` cannot read it from
    // disk later. Patch the in-memory source body directly so analysis has
    // the entrypoint contents available.
    {
        use std::sync::Arc;
        let path_in_input = lifted
            .input
            .sources
            .0
            .keys()
            .find(|p| p.file_name().and_then(|n| n.to_str()) == Some(ENTRYPOINT_FILE))
            .cloned();
        if let Some(p) = path_in_input
            && let Some(s) = lifted.input.sources.0.get_mut(&p)
        {
            s.content = Arc::new(src);
        }
    }

    // Best-effort cleanup; if the user re-runs we'll just overwrite.
    let _ = std::fs::remove_file(&entrypoint_path);

    Ok(CompiledEntrypoint {
        deployed_bytecode: deployed,
        run_selector: alloy_primitives::keccak256(b"run()")[..4].try_into().unwrap(),
        artifact: lifted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrypoint_includes_setup_when_present() {
        let src =
            generate_entrypoint_source("MyTest", "testFoo", true, "0.8.20", "test/MyTest.t.sol");
        assert!(src.contains("t.setUp();"));
        assert!(src.contains("t.testFoo();"));
        assert!(src.contains("pragma solidity ^0.8.20;"));
        assert!(src.contains("import \"test/MyTest.t.sol\""));
    }

    #[test]
    fn entrypoint_omits_setup_when_absent() {
        let src = generate_entrypoint_source("MyTest", "testFoo", false, "0.8.20", "x.sol");
        assert!(!src.contains("setUp"));
        assert!(src.contains("t.testFoo();"));
    }

    #[test]
    fn entrypoint_strips_compiler_version_prefix() {
        let src = generate_entrypoint_source("MyTest", "testFoo", false, "v0.8.20", "x.sol");
        assert!(src.contains("pragma solidity ^0.8.20;"));
    }

    #[test]
    fn entrypoint_truncates_compiler_version_patch() {
        let src =
            generate_entrypoint_source("MyTest", "testFoo", false, "0.8.20+commit.xyz", "x.sol");
        assert!(src.contains("pragma solidity ^0.8.20;"));
    }
}
