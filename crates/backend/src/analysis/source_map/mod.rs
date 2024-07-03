use std::sync::Arc;

use debug_unit::{PrimativeUnitVisitor, PrimitiveUnits};

use crate::{artifact::deploy::DeployArtifact, utils::ast::source_with_primative_statements};

use eyre::{eyre, Result};

use super::ast_visitor::Walk;

pub mod debug_unit;

/// A more reliable source map analysis.
pub struct SourceMapAnalysis {}

impl SourceMapAnalysis {
    /// Analyze the source map of a compilation artifact.
    pub fn analyze(artifact: &DeployArtifact) -> Result<PrimitiveUnits> {
        // Step 1. collect primitive debugging units.
        let mut visitor = PrimativeUnitVisitor::new();
        for (id, source) in artifact.sources.iter() {
            visitor.register(*id as usize, Arc::clone(&source.code));
            source.ast.walk(&mut visitor)?;
        }

        let units = visitor.produce()?;

        for (index, stmts) in &units {
            let source = artifact
                .sources
                .get(&(*index as u32))
                .ok_or(eyre!("missing source"))?
                .code
                .as_str();

            // println!("{}", source_with_primative_statements(source, stmts));
        }

        Ok(units)
    }
}
