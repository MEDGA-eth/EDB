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

use eyre::{OptionExt, Result};
use foundry_compilers::artifacts::{Ast, Node, NodeType, SourceUnit};

/// We prune the AST to remove or refine nodes that are not strongly related to analysis.
/// We do this because the Solidity compiler has changed the AST structure over time, but
/// we want to maintain a consistently parsable AST structure for debugging purposes.
///
/// Note that it does not mean we will not show the original source code to the user. The
/// pruned AST is only used for *source-byte alignment analysis*, and the original source
/// code will still be shown to the user.
///
/// Specifically, we will perform the following operations:
/// - Remove the `documentation` field from all nodes.
/// - If the node is an InlineAssembly node and does not have an AST field
///    - Add an empty YulBlock node to the AST field
///    - Set the `externalReferences` field to an empty array
///    - Remove the `operations` field
/// - If the node is an ImportDirective
///    - Set the `symbolAliases` as an empty array
pub struct ASTPruner {}

impl ASTPruner {
    /// Convert the AST to a SourceUnit.
    pub fn convert(ast: &mut Ast, prune: bool) -> Result<SourceUnit> {
        if prune {
            Self::prune(ast)?;
        }
        let serialized = serde_json::to_string(ast)?;

        Ok(serde_json::from_str(&serialized)?)
    }

    fn prune(ast: &mut Ast) -> Result<()> {
        for node in ast.nodes.iter_mut() {
            Self::prune_node(node)?;
        }

        for (field, value) in ast.other.iter_mut() {
            if field == "documentation" {
                // we nullify the documentation field as Solidity 0.4.0 does not support it
                *value = serde_json::Value::Null;
            } else {
                Self::prune_value(value)?;
            }
        }

        Ok(())
    }

    fn prune_node(node: &mut Node) -> Result<()> {
        // check InlineAssembly nodes
        if matches!(node.node_type, NodeType::InlineAssembly) && !node.other.contains_key("AST") {
            // this means that the InlineAssembly node comes from older versions of Solidity

            // we add an empty YulBlock node to the AST field
            let ast = serde_json::json!({
                "nodeType": "YulBlock",
                "src": node.src,
                "statements": [],
            });
            node.other.insert("AST".to_string(), ast);

            // we set the externalReferences field to an empty array
            node.other.insert("externalReferences".to_string(), serde_json::json!([]));

            // we remove the operations field
            node.other.remove("operations");
        }

        // check ImportDirective nodes
        if matches!(node.node_type, NodeType::ImportDirective) {
            // we set the symbolAliases field to an empty array
            node.other.insert("symbolAliases".to_string(), serde_json::json!([]));
        }

        // prune documentation
        for (field, value) in node.other.iter_mut() {
            if field == "documentation" {
                // we nullify the documentation field as Solidity 0.4.0 does not support it
                *value = serde_json::Value::Null;
            } else {
                Self::prune_value(value)?;
            }
        }

        if let Some(body) = &mut node.body {
            Self::prune_node(body)?;
        }

        for node in node.nodes.iter_mut() {
            Self::prune_node(node)?;
        }

        Ok(())
    }

    fn prune_value(value: &mut serde_json::Value) -> Result<()> {
        match value {
            serde_json::Value::Object(obj) => {
                // check for InlineAssembly nodes
                if let Some(node_type) = obj.get("nodeType")
                    && node_type.as_str() == Some("InlineAssembly")
                {
                    // this means that the InlineAssembly node comes from older versions of
                    // Solidity
                    if !obj.contains_key("AST") {
                        let ast = serde_json::json!({
                            "nodeType": "YulBlock",
                            "src": obj.get("src").ok_or_eyre("missing src")?.clone(),
                            "statements": [],
                        });
                        obj.insert("AST".to_string(), ast);
                    }

                    // we set the externalReferences field to an empty array
                    obj.insert("externalReferences".to_string(), serde_json::json!([]));

                    // we remove the operations field
                    obj.remove("operations");
                }

                // check for ImportDirective nodes
                if let Some(node_type) = obj.get("nodeType")
                    && node_type.as_str() == Some("ImportDirective")
                {
                    // we set the symbolAliases field to an empty array
                    obj.insert("symbolAliases".to_string(), serde_json::json!([]));
                }

                // prune documentation
                for (field, value) in obj.iter_mut() {
                    if field == "documentation" {
                        // we nullify the documentation field as Solidity 0.4.0 does not support it
                        *value = serde_json::Value::Null;
                    } else {
                        Self::prune_value(value)?;
                    }
                }
            }
            serde_json::Value::Array(arr) => {
                for value in arr.iter_mut() {
                    Self::prune_value(value)?;
                }
            }
            _ => {}
        }

        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test_utils {
    //! Test utilities for AST pruning and compilation
    //! This module provides helper functions to compile Solidity source code
    //! into AST SourceUnit structures for testing purposes.

    use eyre::Result;
    use foundry_compilers::{
        CompilationError as _,
        artifacts::{
            EvmVersion, Settings, Severity, SolcInput, Source, SourceUnit, Sources,
            output_selection::OutputSelection,
        },
        solc::SolcLanguage,
    };
    use semver::Version;
    use std::path::PathBuf;

    use crate::{ASTPruner, find_or_install_solc};

    /// Compile a string as Solidity source code to a SourceUnit
    pub fn compile_contract_source_to_source_unit(
        solc_version: Version,
        source: &str,
        prune: bool,
    ) -> Result<SourceUnit> {
        let phantom_file_name = PathBuf::from("Contract.sol");
        let sources = Sources::from_iter([(phantom_file_name.clone(), Source::new(source))]);
        let mut settings = Settings::new(OutputSelection::complete_output_selection());
        settings.evm_version = EvmVersion::default_version_solc(&solc_version);
        let solc_input = SolcInput::new(SolcLanguage::Solidity, sources, settings);
        let compiler = find_or_install_solc(&solc_version)?;
        let output = compiler.compile_exact(&solc_input)?;

        // return error if compiler error
        let errors = output
            .errors
            .iter()
            .filter(|e| e.severity() == Severity::Error)
            .map(|e| format!("{e}"))
            .collect::<Vec<_>>();
        if !errors.is_empty() {
            return Err(eyre::eyre!("Compiler error: {}", errors.join("\n")));
        }

        let mut ast = output
            .sources
            .get(&phantom_file_name)
            .expect("No AST found")
            .ast
            .clone()
            .expect("AST is not selected as output");

        let source_unit = ASTPruner::convert(&mut ast, prune)?;
        Ok(source_unit)
    }
}

#[cfg(test)]
mod tests {
    use std::{env, str::FromStr, time::Duration};

    use alloy_chains::Chain;
    use alloy_primitives::Address;
    use edb_common::{CachePath, EdbCachePath, test_utils::setup_test_environment};
    use eyre::Result;
    use foundry_block_explorers::Client;
    use semver::Version;

    use crate::{test_utils::compile_contract_source_to_source_unit, utils::OnchainCompiler};

    use super::*;

    async fn download_and_compile(chain: Chain, addr: Address) -> Result<()> {
        setup_test_environment(true);

        let cache_path = EdbCachePath::new(env::var(edb_common::env::EDB_CACHE_DIR).ok());
        let cache_ttl = Duration::from_secs(u32::MAX as u64); // we don't want the cache to expire

        let client = Client::builder()
            .chain(chain)?
            .with_cache(cache_path.etherscan_chain_cache_dir(chain), cache_ttl)
            .build()?;

        let compiler = OnchainCompiler::new(cache_path.compiler_chain_cache_dir(chain))?;

        let mut artifact =
            compiler.compile(&client, addr).await?.ok_or_eyre("missing compiler output")?;
        for (_, contract) in artifact.output.sources.iter_mut() {
            ASTPruner::convert(contract.ast.as_mut().ok_or_eyre("AST does not exist")?, true)?;
        }

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_solidity_external_library() {
        let addr = Address::from_str("0x0F6E8eF18FB5bb61D545fEe60f779D8aED60408F").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_solidity_v0_8_18() {
        let addr = Address::from_str("0xe45dfc26215312edc131e34ea9299fbca53275ca").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_solidity_v0_8_17() {
        let addr = Address::from_str("0x1111111254eeb25477b68fb85ed929f73a960582").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_solidity_v0_7_6() {
        let addr = Address::from_str("0x1f98431c8ad98523631ae4a59f267346ea31f984").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_solidity_v0_6_12() {
        let addr = Address::from_str("0x1eb4cf3a948e7d72a198fe073ccb8c7a948cd853").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_solidity_v0_5_17() {
        let addr = Address::from_str("0xee39E4A6820FFc4eDaA80fD3b5A59788D515832b").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_solidity_v0_4_24() {
        let addr = Address::from_str("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[test]
    fn test_compile_contract_source() {
        // Define a simple Solidity contract source code
        let source_code = r#"
        // SPDX-License-Identifier: MIT
        pragma solidity ^0.8.0;

        contract SimpleStorage {
            uint256 private storedData;

            function set(uint256 x) public {
                storedData = x;
            }

            function get() public view returns (uint256) {
                return storedData;
            }
        }
        "#;

        // Define the Solidity compiler version
        let solc_version = Version::parse("0.8.0").expect("Invalid version");

        // Compile the contract source code
        let result = compile_contract_source_to_source_unit(solc_version, source_code, true);

        // Assert that the compilation was successful
        assert!(result.is_ok(), "Compilation failed: {result:?}");

        // Extract the source unit
        let source_unit = result.unwrap();

        // Assert that the source unit has nodes (AST nodes)
        assert!(!source_unit.nodes.is_empty(), "No AST nodes found in source unit");
    }
}
