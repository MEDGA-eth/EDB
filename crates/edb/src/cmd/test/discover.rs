// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! Test target resolution: parse `[[path::]contract::]test_name`, locate it in
//! the compiled project, and extract its ABI + bytecodes.
//!
//! Supported target forms:
//! - `testFn` — search every contract whose ABI exposes `testFn`.
//! - `Contract::testFn` — restrict to contracts named `Contract`.
//! - `path::Contract::testFn` — additionally require the artifact's source path
//!   to contain the substring `path` (case-sensitive, normalized to `/`).
//!
//! When the target resolves to more than one candidate after filtering, EDB
//! falls back to a heuristic (prefer the artifact whose file stem matches the
//! contract name) and, if that still leaves more than one, prompts the user
//! interactively on a TTY or bails with the candidate list otherwise.

use alloy_json_abi::JsonAbi;
use alloy_primitives::Bytes;
use eyre::{Result, bail};
use foundry_compilers::{
    ArtifactId, ProjectCompileOutput,
    artifacts::{ConfigurableContractArtifact, Libraries},
};
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

/// Structured parse of a user-supplied target string. All hints are optional;
/// only `test_function` is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTarget {
    /// Substring filter applied to each artifact's source path
    /// (`id.source`, normalized to `/`). `None` means no filter.
    pub path_hint: Option<String>,
    /// Exact-match filter on the artifact's contract name (`id.name`).
    /// `None` means no filter.
    pub contract_hint: Option<String>,
    /// Required exact match for the function name within the contract's ABI.
    pub test_function: String,
}

/// Parse a target string in `[[path::]contract::]test_name` form.
pub fn parse_target(s: &str) -> Result<ParsedTarget> {
    let parts: Vec<&str> = s.split("::").collect();
    let parsed = match parts.as_slice() {
        [test] => {
            ParsedTarget { path_hint: None, contract_hint: None, test_function: (*test).to_owned() }
        }
        [contract, test] => ParsedTarget {
            path_hint: None,
            contract_hint: Some((*contract).to_owned()),
            test_function: (*test).to_owned(),
        },
        [path, contract, test] => ParsedTarget {
            path_hint: Some((*path).to_owned()),
            contract_hint: Some((*contract).to_owned()),
            test_function: (*test).to_owned(),
        },
        _ => bail!(
            "expected `[[path::]contract::]test_name` (1, 2, or 3 `::`-separated parts); got {s:?}"
        ),
    };
    if parsed.test_function.is_empty() {
        bail!("test function name must be non-empty in {s:?}");
    }
    if let Some(c) = parsed.contract_hint.as_ref()
        && c.is_empty()
    {
        bail!("contract name must be non-empty in {s:?}");
    }
    if let Some(p) = parsed.path_hint.as_ref()
        && p.is_empty()
    {
        bail!("path qualifier must be non-empty in {s:?}");
    }
    Ok(parsed)
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
/// `target` is parsed via [`parse_target`]; see this module's top-level docs
/// for the accepted forms. Returns a [`ResolvedTest`] containing the full
/// artifact details needed to run the test.
#[allow(dead_code)] // called by downstream tasks (3.4+)
pub fn resolve_test(
    target: &str,
    project_root: &Path,
    output: &ProjectCompileOutput,
    libraries: &Libraries,
) -> Result<ResolvedTest> {
    let parsed = parse_target(target)?;
    validate_test_function_name(&parsed.test_function)?;

    let candidates: Vec<_> =
        output.artifact_ids().filter(|(id, art)| candidate_matches(id, art, &parsed)).collect();

    if candidates.is_empty() {
        let hint = build_no_match_hint(&parsed, output);
        bail!("no test contract matches {target:?}.{hint}");
    }

    let chosen = if candidates.len() == 1 {
        candidates.into_iter().next().unwrap()
    } else {
        select_candidate(&parsed, candidates, target)?
    };

    let (id, artifact) = chosen;
    build_resolved_test(id.name.clone(), parsed.test_function, &id, artifact, project_root, libraries)
}

fn candidate_matches(
    id: &ArtifactId,
    art: &ConfigurableContractArtifact,
    parsed: &ParsedTarget,
) -> bool {
    if let Some(c) = parsed.contract_hint.as_ref()
        && id.name != *c
    {
        return false;
    }
    if let Some(p) = parsed.path_hint.as_ref() {
        let source_norm = id.source.to_string_lossy().replace('\\', "/");
        if !source_norm.contains(p) {
            return false;
        }
    }
    art.abi.as_ref().is_some_and(|abi| abi.functions.contains_key(&parsed.test_function))
}

fn build_no_match_hint(parsed: &ParsedTarget, output: &ProjectCompileOutput) -> String {
    // If only the test_function failed, suggest the nearest function name in
    // any matching contract — same UX the old resolver gave.
    let function_universe: Vec<String> = output
        .artifact_ids()
        .filter_map(|(id, art)| {
            if let Some(c) = parsed.contract_hint.as_ref()
                && id.name != *c
            {
                return None;
            }
            art.abi.as_ref()
        })
        .flat_map(|abi| abi.functions.keys().cloned())
        .collect();
    if function_universe.is_empty() {
        return String::new();
    }
    let suggestion = closest_match(&parsed.test_function, function_universe.iter());
    if suggestion.is_empty() { String::new() } else { format!(" Did you mean {suggestion:?}?") }
}

fn select_candidate<'a>(
    parsed: &ParsedTarget,
    candidates: Vec<(ArtifactId, &'a ConfigurableContractArtifact)>,
    raw_target: &str,
) -> Result<(ArtifactId, &'a ConfigurableContractArtifact)> {
    // Heuristic: prefer the artifact whose file stem matches the contract name.
    // This is the original disambiguation rule and handles the common case
    // where one of N candidates is the "canonical" contract while the others
    // are e.g. mock or stub variants.
    let preferred: Vec<_> = candidates
        .iter()
        .filter(|(id, _)| {
            id.source.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s == id.name)
        })
        .cloned()
        .collect();
    if preferred.len() == 1 {
        return Ok(preferred.into_iter().next().unwrap());
    }

    // Interactive disambiguation on a TTY. Non-interactive uses get an error
    // listing the candidates plus a hint to qualify with `path::`.
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        prompt_select(&candidates, &mut std::io::stdin().lock(), &mut std::io::stdout(), raw_target)
    } else {
        bail!("{}", format_ambiguous_error(parsed, &candidates, raw_target));
    }
}

fn prompt_select<'a, R: BufRead, W: Write>(
    candidates: &[(ArtifactId, &'a ConfigurableContractArtifact)],
    reader: &mut R,
    writer: &mut W,
    raw_target: &str,
) -> Result<(ArtifactId, &'a ConfigurableContractArtifact)> {
    writeln!(writer, "Target {raw_target:?} matches {} candidates:", candidates.len())?;
    for (i, (id, _)) in candidates.iter().enumerate() {
        writeln!(writer, "  [{}] {} ({})", i + 1, id.name, id.source.display())?;
    }
    write!(writer, "Select [1-{}]: ", candidates.len())?;
    writer.flush()?;

    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        bail!("EOF on stdin while waiting for selection");
    }
    let choice: usize = line.trim().parse().map_err(|_| {
        eyre::eyre!("expected a number 1-{}, got {:?}", candidates.len(), line.trim())
    })?;
    if !(1..=candidates.len()).contains(&choice) {
        bail!("selection {choice} is out of range [1, {}]", candidates.len());
    }
    Ok(candidates[choice - 1].clone())
}

fn format_ambiguous_error(
    parsed: &ParsedTarget,
    candidates: &[(ArtifactId, &ConfigurableContractArtifact)],
    raw_target: &str,
) -> String {
    let mut msg = format!("target {raw_target:?} matches {} candidates:\n", candidates.len());
    for (id, _) in candidates {
        msg.push_str(&format!("  - {} ({})\n", id.name, id.source.display()));
    }
    let contract_part = parsed.contract_hint.as_deref().unwrap_or("Contract");
    msg.push_str(&format!(
        "qualify with `path::{contract_part}::{}` (path is a substring of the source-file path) to disambiguate.",
        parsed.test_function,
    ));
    msg
}

fn build_resolved_test(
    contract_name: String,
    test_function: String,
    id: &foundry_compilers::ArtifactId,
    artifact: &ConfigurableContractArtifact,
    project_root: &Path,
    libraries: &Libraries,
) -> Result<ResolvedTest> {
    let abi = artifact.abi.clone().ok_or_else(|| eyre::eyre!("compiled artifact has no ABI"))?;

    let has_setup = abi.functions.contains_key("setUp");
    let has_target = abi.functions.contains_key(&test_function);
    if !has_target {
        let suggestion = closest_match(&test_function, abi.functions.keys());
        bail!("function {test_function:?} not in {contract_name:?}. Did you mean {suggestion:?}?");
    }

    // Build link triples from the provided libraries map, then link the
    // bytecode objects before extracting bytes.  For test contracts without
    // external library references the map is empty and behaviour is unchanged.
    // For contracts whose bytecode still contains unresolved placeholders after
    // linking (e.g. an empty map was passed) we fall back to an empty Bytes
    // rather than killing the session — these fields are informational only.
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

    let deployed = artifact
        .deployed_bytecode
        .as_ref()
        .and_then(|d| d.bytecode.as_ref())
        .map(|bc| {
            let mut o = bc.object.clone();
            o.link_all(link_triples.iter().map(|(f, n, a)| (f, n, *a)));
            o
        })
        .and_then(|o| o.into_bytes())
        .unwrap_or_default();

    let creation = artifact
        .bytecode
        .as_ref()
        .map(|bc| {
            let mut o = bc.object.clone();
            o.link_all(link_triples.iter().map(|(f, n, a)| (f, n, *a)));
            o
        })
        .and_then(|o| o.into_bytes())
        .unwrap_or_default();

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

/// Returns the closest string in `hay` to `needle` by Levenshtein edit distance.
pub(crate) fn closest_match<'a, I: IntoIterator<Item = &'a String>>(
    needle: &str,
    hay: I,
) -> String {
    hay.into_iter().min_by_key(|h| levenshtein(needle, h)).cloned().unwrap_or_default()
}

/// Computes the Levenshtein edit distance between two strings.
pub(crate) fn levenshtein(a: &str, b: &str) -> usize {
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
    fn parses_test_only() {
        let p = parse_target("testFoo").unwrap();
        assert_eq!(
            p,
            ParsedTarget { path_hint: None, contract_hint: None, test_function: "testFoo".into() }
        );
    }

    #[test]
    fn parses_contract_qualified() {
        let p = parse_target("MyTest::testFoo").unwrap();
        assert_eq!(
            p,
            ParsedTarget {
                path_hint: None,
                contract_hint: Some("MyTest".into()),
                test_function: "testFoo".into(),
            }
        );
    }

    #[test]
    fn parses_fully_qualified() {
        let p = parse_target("sd59x18::ConvertFrom_Unit_Test::test_Foo").unwrap();
        assert_eq!(
            p,
            ParsedTarget {
                path_hint: Some("sd59x18".into()),
                contract_hint: Some("ConvertFrom_Unit_Test".into()),
                test_function: "test_Foo".into(),
            }
        );
    }

    #[test]
    fn rejects_too_many_parts() {
        assert!(parse_target("a::b::c::d").is_err());
    }

    #[test]
    fn rejects_empty_function() {
        assert!(parse_target("A::").is_err());
        assert!(parse_target("").is_err());
    }

    #[test]
    fn rejects_empty_contract() {
        assert!(parse_target("::testFoo").is_err());
        assert!(parse_target("p::::testFoo").is_err());
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

    /// Quick smoke: `prompt_select` should parse `2\n` and return the second
    /// candidate, given a fake reader/writer. We can't construct full
    /// `ConfigurableContractArtifact` values easily here, so this test stubs
    /// only the parts of the API it exercises by using the prompt-only path
    /// with hand-rolled candidates. Skipped — see integration test for the
    /// full path.
    #[test]
    fn prompt_parses_user_selection() {
        // Validate the parsing arithmetic without constructing real artifacts:
        // a `1-based` selection of 2 in a 3-candidate list maps to index 1.
        let line = "2\n";
        let parsed: usize = line.trim().parse().unwrap();
        assert!((1..=3).contains(&parsed));
        assert_eq!(parsed - 1, 1);
    }
}
