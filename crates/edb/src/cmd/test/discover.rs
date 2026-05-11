// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! Test target resolution: parse `Contract::testFn`, locate it in the compiled
//! project, and extract its ABI + bytecodes.

use alloy_json_abi::JsonAbi;
use alloy_primitives::Bytes;
use eyre::{Result, bail};
use foundry_compilers::{Artifact, ProjectCompileOutput, artifacts::ConfigurableContractArtifact};
use std::path::{Path, PathBuf};

/// Parse a "Contract::testFn" target string.
pub fn parse_target(s: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = s.split("::").collect();
    if parts.len() != 2 {
        bail!("expected `Contract::testFn`, got {s:?}");
    }
    if parts[0].is_empty() || parts[1].is_empty() {
        bail!("contract and function names must be non-empty in {s:?}");
    }
    Ok((parts[0].to_owned(), parts[1].to_owned()))
}

/// Validate that the function name matches a forge test-function pattern.
pub fn validate_test_function_name(name: &str) -> Result<()> {
    if name.starts_with("testFail") {
        bail!("legacy `testFail*` is no longer accepted by foundry; rewrite using vm.expectRevert");
    }
    let is_test = name.starts_with("test")
        || name.starts_with("testFuzz")
        || name.starts_with("testFork")
        || name.starts_with("invariant_");
    if !is_test {
        bail!(
            "{name:?} is not a recognized forge test function name (expected test*, testFuzz*, testFork*, invariant_*)"
        );
    }
    Ok(())
}

/// Located test artifact and its compiled form.
#[allow(dead_code)] // consumed by downstream tasks (3.4+)
pub struct ResolvedTest {
    /// Name of the test contract.
    pub contract_name: String,
    /// Name of the test function to invoke.
    pub test_function: String,
    /// Path to the artifact source file.
    pub artifact_path: PathBuf,
    /// ABI of the test contract.
    pub abi: JsonAbi,
    /// Deployed bytecode of the test contract.
    pub deployed_bytecode: Bytes,
    /// Creation bytecode of the test contract.
    pub creation_bytecode: Bytes,
    /// Whether the contract has a `setUp` function.
    pub has_setup: bool,
    /// Solidity compiler version used.
    pub compiler_version: String,
}

/// Locate a test function in a compiled Foundry project output.
///
/// `target` must be in `Contract::testFn` form. The function name must follow
/// forge naming conventions. Returns a [`ResolvedTest`] containing the full
/// artifact details needed to run the test.
#[allow(dead_code)] // called by downstream tasks (3.4+)
pub fn resolve_test(
    target: &str,
    project_root: &Path,
    output: &ProjectCompileOutput,
) -> Result<ResolvedTest> {
    let (contract_name, test_function) = parse_target(target)?;
    validate_test_function_name(&test_function)?;

    // artifact_ids() yields (ArtifactId, &ConfigurableContractArtifact) for
    // both cached and freshly compiled artifacts.
    let mut candidates: Vec<_> =
        output.artifact_ids().filter(|(id, _)| id.name == contract_name).collect();

    if candidates.is_empty() {
        bail!("contract {contract_name:?} not found in compiled output");
    }
    if candidates.len() > 1 {
        // Prefer the artifact whose source file stem matches the contract name.
        candidates.retain(|(id, _)| {
            id.source.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s == contract_name)
        });
        if candidates.len() != 1 {
            bail!(
                "multiple contracts named {contract_name:?}; ambiguity not resolved automatically"
            );
        }
    }
    let (id, artifact) = candidates.into_iter().next().unwrap();
    build_resolved_test(contract_name, test_function, &id, artifact, project_root)
}

fn build_resolved_test(
    contract_name: String,
    test_function: String,
    id: &foundry_compilers::ArtifactId,
    artifact: &ConfigurableContractArtifact,
    project_root: &Path,
) -> Result<ResolvedTest> {
    let abi = artifact.abi.clone().ok_or_else(|| eyre::eyre!("compiled artifact has no ABI"))?;

    let has_setup = abi.functions.contains_key("setUp");
    let has_target = abi.functions.contains_key(&test_function);
    if !has_target {
        let suggestion = closest_match(&test_function, abi.functions.keys());
        bail!("function {test_function:?} not in {contract_name:?}. Did you mean {suggestion:?}?");
    }

    // Both helpers are trait methods from foundry_compilers::Artifact, which is
    // blanket-implemented for ConfigurableContractArtifact via the Into<Compact*>
    // impls. They return Option<Cow<'_, Bytes>>.
    let deployed = artifact
        .get_deployed_bytecode_bytes()
        .ok_or_else(|| eyre::eyre!("artifact missing deployed bytecode"))?
        .into_owned();
    let creation = artifact
        .get_bytecode_bytes()
        .ok_or_else(|| eyre::eyre!("artifact missing creation bytecode"))?
        .into_owned();

    // Metadata.compiler is Compiler (not Option<Compiler>), so no inner Option unwrap.
    let compiler_version = artifact
        .metadata
        .as_ref()
        .map(|m| m.compiler.version.clone())
        .ok_or_else(|| eyre::eyre!("artifact missing compiler metadata"))?;

    Ok(ResolvedTest {
        contract_name,
        test_function,
        artifact_path: project_root.join(&id.source),
        abi,
        deployed_bytecode: deployed,
        creation_bytecode: creation,
        has_setup,
        compiler_version,
    })
}

fn closest_match<'a, I: IntoIterator<Item = &'a String>>(needle: &str, hay: I) -> String {
    hay.into_iter().min_by_key(|h| levenshtein(needle, h)).cloned().unwrap_or_default()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_qualified_name() {
        let (c, f) = parse_target("MyTest::testFoo").unwrap();
        assert_eq!(c, "MyTest");
        assert_eq!(f, "testFoo");
    }

    #[test]
    fn rejects_unqualified() {
        let err = parse_target("testFoo").unwrap_err();
        assert!(err.to_string().contains("Contract::testFn"));
    }

    #[test]
    fn rejects_extra_colons() {
        assert!(parse_target("A::B::C").is_err());
    }

    #[test]
    fn rejects_legacy_test_fail_names() {
        assert!(validate_test_function_name("testFailFoo").is_err());
    }

    #[test]
    fn accepts_test_prefixes() {
        for n in ["test_", "testFoo", "testFuzzBar", "invariant_x", "testForkBaz"] {
            validate_test_function_name(n).unwrap_or_else(|e| panic!("{n} rejected: {e}"));
        }
    }
}
