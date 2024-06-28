use eyre::{eyre, Result};
use foundry_compilers::artifacts::{Ast, Node, NodeType, SourceUnit};

/// We prune the AST to remove or refine nodes that are not strongly related to debugging.
/// We do this because the Solidity compiler has changed the AST structure over time, but
/// we want to maintain a consistently parsaable AST structure for debugging purposes.
///
/// Note that it does not mean we will not show the original source code to the user. The
/// pruned AST is only used for analysis, and the original source code will still be shown
/// to the user.
///
/// Specifically, we will perform the following operations:
/// - Remove the `documentation` field from all nodes.
/// - If the node is an InlineAssembly node and does not have an AST field
///    - Add an empty YulBlock node to the AST field
///    - Set the `externalReferences` field to an empty array
///    - Remove the `operations` field
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
