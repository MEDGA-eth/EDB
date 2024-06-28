use std::{fs::File, io::Write};

use eyre::{eyre, Result};
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
///  
pub struct ASTPruner {}

impl ASTPruner {
    pub fn convert(ast: &mut Ast) -> Result<SourceUnit> {
        Self::prune(ast)?;
        let serialized = serde_json::to_string(ast)?;

        let mut file = File::create("/tmp/txt.json")?;
        file.write_all(serialized.as_bytes())?;

        Ok(serde_json::from_str(&serialized)?)
    }

    pub fn prune(ast: &mut Ast) -> Result<()> {
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

    pub fn prune_node(node: &mut Node) -> Result<()> {
        // check InlineAssembly nodes
        if matches!(node.node_type, NodeType::InlineAssembly) {
            if !node.other.contains_key("AST") {
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

    pub fn prune_value(value: &mut serde_json::Value) -> Result<()> {
        match value {
            serde_json::Value::Object(obj) => {
                // check for InlineAssembly nodes
                if let Some(node_type) = obj.get("nodeType") {
                    if node_type.as_str() == Some("InlineAssembly") {
                        // this means that the InlineAssembly node comes from older versions of
                        // Solidity
                        if !obj.contains_key("AST") {
                            let ast = serde_json::json!({
                                "nodeType": "YulBlock",
                                "src": obj.get("src").ok_or(eyre!("missing src"))?.clone(),
                                "statements": [],
                            });
                            obj.insert("AST".to_string(), ast);
                        }

                        // we set the externalReferences field to an empty array
                        obj.insert("externalReferences".to_string(), serde_json::json!([]));

                        // we remove the operations field
                        obj.remove("operations");
                    }
                }

                // check for ImportDirective nodes
                if let Some(node_type) = obj.get("nodeType") {
                    if node_type.as_str() == Some("ImportDirective") {
                        // we set the symbolAliases field to an empty array
                        obj.insert("symbolAliases".to_string(), serde_json::json!([]));
                    }
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
mod tests {
    use std::{path::PathBuf, str::FromStr, time::Duration};

    use alloy_chains::Chain;
    use alloy_primitives::Address;
    use eyre::Result;
    use foundry_block_explorers::Client;
    use foundry_compilers::{
        artifacts::{output_selection::OutputSelection, SolcInput, Source},
        solc::{Solc, SolcLanguage},
    };
    use serial_test::serial;

    use crate::etherscan_rate_limit_guard;

    use super::*;

    async fn download_and_compile(chain: Chain, addr: Address) -> Result<()> {
        let cache_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("etherscan_cache")
            .join(chain.to_string());
        let cache_ttl = Duration::from_secs(u32::MAX as u64); // we don't want the cache to expire
        let client =
            Client::builder().chain(chain)?.with_cache(Some(cache_root), cache_ttl).build()?;

        // download the source code
        let mut meta = etherscan_rate_limit_guard!(client.contract_source_code(addr).await)?;
        eyre::ensure!(meta.items.len() == 1, "contract not found or ill-formed");
        let meta = meta.items.remove(0);

        // prepare the input for solc
        let mut settings = meta.settings()?;
        // enforce compiler output all possible outputs
        settings.output_selection = OutputSelection::complete_output_selection();
        let sources =
            meta.sources().into_iter().map(|(k, v)| (k.into(), Source::new(v.content))).collect();
        let input = SolcInput::new(SolcLanguage::Solidity, sources, settings);

        // prepare the compiler
        let version = meta.compiler_version()?;
        let compiler = Solc::find_or_install(&version)?;

        let mut output = compiler.compile_exact(&input)?;
        for (_, contract) in output.sources.iter_mut() {
            ASTPruner::convert(contract.ast.as_mut().ok_or(eyre!("AST does not exist"))?)?;
        }

        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn test_solidity_v0_8_18() {
        let addr = Address::from_str("0xe45dfc26215312edc131e34ea9299fbca53275ca").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn test_solidity_v0_7_6() {
        let addr = Address::from_str("0x1f98431c8ad98523631ae4a59f267346ea31f984").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn test_solidity_v0_6_12() {
        let addr = Address::from_str("0x1eb4cf3a948e7d72a198fe073ccb8c7a948cd853").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn test_solidity_v0_5_17() {
        let addr = Address::from_str("0xee39E4A6820FFc4eDaA80fD3b5A59788D515832b").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn test_solidity_v0_4_24() {
        let addr = Address::from_str("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }
}
