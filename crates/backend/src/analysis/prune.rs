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
///  
pub struct ASTPruner {}

impl ASTPruner {
    pub fn convert(ast: &mut Ast) -> Result<SourceUnit> {
        Self::prune(ast)?;
        let serialized = serde_json::to_string(ast)?;

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
    use edb_utils::onchain_compiler::OnchainCompiler;
    use eyre::Result;
    use foundry_block_explorers::Client;
    use serial_test::serial;

    use super::*;

    async fn download_and_compile(chain: Chain, addr: Address) -> Result<()> {
        let cache_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/cache/etherscan")
            .join(chain.to_string());
        let cache_ttl = Duration::from_secs(u32::MAX as u64); // we don't want the cache to expire
        let client =
            Client::builder().chain(chain)?.with_cache(Some(cache_root), cache_ttl).build()?;

        let compiler_cache_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/cache/solc")
            .join(chain.to_string());
        let compiler = OnchainCompiler::new(Some(compiler_cache_root))?;

        let (_, _, mut output) =
            compiler.compile(&client, addr).await?.ok_or_eyre("missing compiler output")?;
        for (_, contract) in output.sources.iter_mut() {
            ASTPruner::convert(contract.ast.as_mut().ok_or_eyre("AST does not exist")?)?;
        }

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_solidity_external_library() {
        let addr = Address::from_str("0x0F6E8eF18FB5bb61D545fEe60f779D8aED60408F").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_solidity_v0_8_18() {
        let addr = Address::from_str("0xe45dfc26215312edc131e34ea9299fbca53275ca").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_solidity_v0_8_17() {
        let addr = Address::from_str("0x1111111254eeb25477b68fb85ed929f73a960582").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_solidity_v0_7_6() {
        let addr = Address::from_str("0x1f98431c8ad98523631ae4a59f267346ea31f984").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_solidity_v0_6_12() {
        let addr = Address::from_str("0x1eb4cf3a948e7d72a198fe073ccb8c7a948cd853").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_solidity_v0_5_17() {
        let addr = Address::from_str("0xee39E4A6820FFc4eDaA80fD3b5A59788D515832b").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_solidity_v0_4_24() {
        let addr = Address::from_str("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48").unwrap();
        download_and_compile(Chain::default(), addr).await.unwrap();
    }
}
