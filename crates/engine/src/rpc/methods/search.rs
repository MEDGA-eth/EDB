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

//! Source-text search across every contract artifact in the session.

use super::super::types::RpcError;
use crate::{EngineContext, error_codes};
use revm::{Database, DatabaseCommit, DatabaseRef, database::CacheDB};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};
use tracing::debug;

/// Cap on total matched lines returned, to bound payload size on large traces.
const MAX_MATCHES: usize = 1000;
/// Cap on a single matched line's length (characters), to bound payload size.
const MAX_LINE_LEN: usize = 240;

#[derive(Debug, Serialize, PartialEq)]
struct LineMatch {
    /// 1-based line number within the file.
    line: usize,
    /// The matching line, trimmed of trailing whitespace and truncated.
    text: String,
}

#[derive(Debug, Serialize, PartialEq)]
struct FileMatches {
    /// Source path as the compiler saw it (e.g. `src/Foo.sol`).
    path: String,
    /// Lowercase `0x` addresses whose artifact contains this exact source path.
    /// The frontend can open the file under any of them.
    addresses: Vec<String>,
    matches: Vec<LineMatch>,
}

#[derive(Debug, Serialize, PartialEq)]
struct SearchResult {
    query: String,
    /// True when the match cap was hit and results were cut short.
    truncated: bool,
    total_matches: usize,
    files: Vec<FileMatches>,
}

/// Truncate `line` to at most `MAX_LINE_LEN` characters on a char boundary,
/// appending an ellipsis when cut. Trailing whitespace is dropped first.
fn clip_line(line: &str) -> String {
    let trimmed = line.trim_end();
    if trimmed.chars().count() <= MAX_LINE_LEN {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(MAX_LINE_LEN).collect();
    out.push('…');
    out
}

/// Pure search core: scan `(address, path, content)` triples for `query`
/// (case-insensitive substring), deduplicating shared source files by path.
///
/// Each path's matching lines are computed once (the first time the path is
/// seen); every address that carries that path is recorded so the caller can
/// open the file under any of them. Results are capped at `max_matches`.
fn search_in_sources<'a>(
    query: &str,
    sources: impl Iterator<Item = (String, String, &'a str)>,
    max_matches: usize,
) -> SearchResult {
    let needle = query.to_lowercase();
    let mut by_path: BTreeMap<String, FileMatches> = BTreeMap::new();
    let mut computed: HashSet<String> = HashSet::new();
    let mut total = 0usize;
    let mut truncated = false;

    for (addr, path, content) in sources {
        let entry = by_path.entry(path.clone()).or_insert_with(|| FileMatches {
            path: path.clone(),
            addresses: Vec::new(),
            matches: Vec::new(),
        });
        if !entry.addresses.contains(&addr) {
            entry.addresses.push(addr);
        }

        // Compute line matches for a given path only once.
        if !computed.insert(path.clone()) {
            continue;
        }
        if total >= max_matches {
            truncated = true;
            continue;
        }
        for (idx, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                if total >= max_matches {
                    truncated = true;
                    break;
                }
                entry.matches.push(LineMatch { line: idx + 1, text: clip_line(line) });
                total += 1;
            }
        }
    }

    let mut files: Vec<FileMatches> =
        by_path.into_values().filter(|f| !f.matches.is_empty()).collect();
    for f in &mut files {
        f.addresses.sort();
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    SearchResult { query: query.to_string(), truncated, total_matches: total, files }
}

/// `edb_searchSources`: full-text search across every artifact's source files.
///
/// Params: `[query: string]`. Returns files (deduped by path) with their
/// matching lines and the addresses that carry them.
pub fn search_sources<DB>(
    context: &Arc<EngineContext<DB>>,
    params: Option<Value>,
) -> Result<Value, RpcError>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    let query = params
        .as_ref()
        .and_then(|p| p.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| RpcError {
            code: error_codes::INVALID_PARAMS,
            message: "Invalid params: expected [query]".to_string(),
            data: None,
        })?;

    if query.trim().is_empty() {
        return Err(RpcError {
            code: error_codes::INVALID_PARAMS,
            message: "Search query must not be empty".to_string(),
            data: None,
        });
    }

    let triples = context.artifacts.iter().flat_map(|(addr, artifact)| {
        let addr_hex = format!("{addr:#x}");
        artifact.input.sources.iter().map(move |(path, source)| {
            (addr_hex.clone(), path.to_string_lossy().into_owned(), source.content.as_str())
        })
    });

    let result = search_in_sources(&query, triples, MAX_MATCHES);

    let json = serde_json::to_value(&result).map_err(|e| RpcError {
        code: error_codes::INTERNAL_ERROR,
        message: format!("Failed to serialize search result: {e}"),
        data: None,
    })?;
    debug!(
        "edb_searchSources('{}') -> {} files, {} matches (truncated={})",
        query,
        result.files.len(),
        result.total_matches,
        result.truncated
    );
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(query: &str, triples: Vec<(String, String, &'static str)>) -> SearchResult {
        search_in_sources(query, triples.into_iter(), MAX_MATCHES)
    }

    #[test]
    fn matches_are_case_insensitive_and_one_based() {
        let r = run(
            "transfer",
            vec![(
                "0xaaa".to_string(),
                "src/Token.sol".to_string(),
                "pragma solidity ^0.8;\nfunction Transfer() {}\n  // TRANSFER hook\n",
            )],
        );
        assert_eq!(r.files.len(), 1);
        let f = &r.files[0];
        assert_eq!(f.path, "src/Token.sol");
        assert_eq!(f.matches.len(), 2);
        assert_eq!(f.matches[0].line, 2);
        assert_eq!(f.matches[1].line, 3);
        assert_eq!(r.total_matches, 2);
        assert!(!r.truncated);
    }

    #[test]
    fn shared_path_is_deduped_and_records_all_addresses() {
        // Two contracts share lib/ERC20.sol; matches computed once, both
        // addresses recorded and sorted.
        let content = "contract ERC20 { function mint() {} }\n";
        let r = run(
            "mint",
            vec![
                ("0xbbb".to_string(), "lib/ERC20.sol".to_string(), content),
                ("0xaaa".to_string(), "lib/ERC20.sol".to_string(), content),
            ],
        );
        assert_eq!(r.files.len(), 1);
        let f = &r.files[0];
        assert_eq!(f.addresses, vec!["0xaaa".to_string(), "0xbbb".to_string()]);
        assert_eq!(f.matches.len(), 1, "shared path matched once, not per-address");
        assert_eq!(r.total_matches, 1);
    }

    #[test]
    fn no_matches_yields_empty_files() {
        let r = run(
            "doesnotexist",
            vec![("0xaaa".to_string(), "a.sol".to_string(), "contract A {}\n")],
        );
        assert!(r.files.is_empty());
        assert_eq!(r.total_matches, 0);
    }

    #[test]
    fn respects_match_cap() {
        let many: String = (0..50).map(|_| "needle\n").collect();
        let leaked: &'static str = Box::leak(many.into_boxed_str());
        let r = search_in_sources(
            "needle",
            vec![("0xaaa".to_string(), "big.sol".to_string(), leaked)].into_iter(),
            10,
        );
        assert_eq!(r.total_matches, 10);
        assert!(r.truncated);
        assert_eq!(r.files[0].matches.len(), 10);
    }
}
