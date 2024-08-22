use std::{
    fmt::{self, Debug},
    sync::Arc,
};

use eyre::{ensure, OptionExt, Result};

use super::{debug_unit::UnitLocation, AnalysisStore};
use crate::{
    analysis::source_map::{debug_unit::DebugUnit, CONSTRUCTOR_IDX, DEPLOYED_IDX},
    artifact::deploy::DeployArtifact,
};

/// Source Label are the information we extracted from the inaccurate source map.
/// It provides a more reliable way to associate the source code with the bytecode.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SourceLabel {
    PrimitiveStmt {
        stmt: DebugUnit,
        func: DebugUnit,
        cntr: DebugUnit,
    },
    InlineAssembly {
        stmt: Option<UnitLocation>,
        block: DebugUnit,
        func: DebugUnit,
        cntr: DebugUnit,
    },
    Tag {
        tag: DebugUnit,
    },
    Others {
        scope: Option<DebugUnit>,
        loc: Option<UnitLocation>,
    },
}

impl Default for SourceLabel {
    fn default() -> Self {
        Self::Others { scope: None, loc: None }
    }
}

impl fmt::Display for SourceLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PrimitiveStmt { stmt, .. } => {
                write!(f, "SourceStatment({})", stmt.loc())
            }
            Self::InlineAssembly { stmt, block, .. } => {
                if let Some(stmt) = stmt {
                    write!(f, "InlineAssemblyStmt({stmt})")
                } else {
                    write!(f, "InlineAssemblyBlock({})", block.loc())
                }
            }
            Self::Tag { tag } => {
                write!(f, "Tag({})", tag.loc())
            }
            Self::Others { scope, loc } => match (scope, loc) {
                (Some(scope), Some(loc)) => write!(f, "Located({}, {})", loc, scope.loc()),
                (Some(_), None) => write!(f, "Invalid"),
                (None, Some(loc)) => write!(f, "Unlocated({loc})"),
                (None, None) => write!(f, "Others"),
            },
        }
    }
}

impl SourceLabel {
    pub fn is_source(&self) -> bool {
        matches!(self, Self::PrimitiveStmt { .. }) || matches!(self, Self::InlineAssembly { .. })
    }

    pub fn is_tag(&self) -> bool {
        matches!(self, Self::Tag { .. })
    }
}

pub type SourceLabels = Vec<SourceLabel>;

#[derive(Debug, Clone)]
pub struct SourceLabelAnalysis {}

impl SourceLabelAnalysis {
    pub fn analyze(artifact: &DeployArtifact, store: &mut AnalysisStore<'_>) -> Result<()> {
        trace!(
            "analyzing source labels, with available file indice: {:?}",
            artifact.sources.keys()
        );

        // Analyze the construction bytecode.
        trace!("analyzing construction bytecode");
        let constructor = Self::analyze_bytecode::<CONSTRUCTOR_IDX>(store)?;

        // Analyze the deployed bytecode.
        trace!("analyzing deployed bytecode");
        let deployed = Self::analyze_bytecode::<DEPLOYED_IDX>(store)?;

        let mut labels = vec![vec![]; 2];
        labels[CONSTRUCTOR_IDX] = constructor;
        labels[DEPLOYED_IDX] = deployed;

        store.source_labels = Some(labels.try_into().expect("this cannot happen"));

        Ok(())
    }

    fn analyze_bytecode<const IDX: usize>(store: &AnalysisStore<'_>) -> Result<Vec<SourceLabel>> {
        let units = store.debug_units()?;

        let source_map = store.source_map()?.get(IDX).ok_or_eyre("no source map found")?;
        trace!("the number of the original source map entries is {}", source_map.len());

        let mut source_labels = Vec::with_capacity(source_map.len());

        for (ic, src) in source_map.iter().enumerate() {
            trace!("ic: {}, source element: {:?}", ic, src);

            // By default, we will assume this is a meaningless source label.
            source_labels.push(SourceLabel::default());

            // Get file index
            let Some(index) = src.index() else { continue };
            let index = index as usize;

            // Get the file units
            let Some(file_units) = units.units_per_file(index) else { continue };
            let Some((_, unit)) = file_units.range(..src.offset() as usize + 1).next_back() else {
                continue;
            };

            // Check the potential tags
            if unit.matches(src.offset() as usize, src.length() as usize) &&
                !unit.is_execution_unit()
            {
                *source_labels.last_mut().expect("this cannot happen") =
                    SourceLabel::Tag { tag: unit.clone() };
                continue
            }

            if unit.contains(src.offset() as usize, src.length() as usize) {
                match &unit {
                    DebugUnit::Primitive(_) => {
                        let function = units.function(unit).ok_or_eyre("no function found")?;
                        let contract = units.contract(unit).ok_or_eyre("no contract found")?;
                        *source_labels.last_mut().expect("this cannot happen") =
                            SourceLabel::PrimitiveStmt {
                                stmt: unit.clone(),
                                func: function.clone(),
                                cntr: contract.clone(),
                            };
                    }
                    DebugUnit::Function(_, _) | DebugUnit::Contract(_) => {
                        *source_labels.last_mut().expect("this cannot happen") =
                            SourceLabel::Others {
                                scope: Some(unit.clone()),
                                loc: Some(UnitLocation {
                                    start: src.offset() as usize,
                                    length: src.length() as usize,
                                    index,
                                    code: Arc::clone(&unit.code),
                                }),
                            };
                    }
                    DebugUnit::InlineAssembly(_, asm_stmts) => {
                        let stmt = asm_stmts
                            .iter()
                            .find(|stmt| {
                                stmt.contains(src.offset() as usize, src.length() as usize)
                            })
                            .cloned();
                        let function = units.function(unit).ok_or_eyre("no function found")?;
                        let contract = units.contract(unit).ok_or_eyre("no contract found")?;
                        *source_labels.last_mut().expect("this cannot happen") =
                            SourceLabel::InlineAssembly {
                                stmt,
                                block: unit.clone(),
                                func: function.clone(),
                                cntr: contract.clone(),
                            };
                    }
                }
            }
        }
        ensure!(
            source_map.len() == source_labels.len(),
            "source map and source labels have different lengths"
        );

        #[cfg(debug_assertions)]
        {
            let bytecode = store.bytecode()?.get(IDX).ok_or_eyre("no bytecode found")?;
            let code = bytecode.bytes().ok_or_eyre("no code found")?.as_ref();
            let ic_pc_map = crate::utils::opcode::IcPcMap::new(code);
            for (ic, (src, label)) in source_map.iter().zip(source_labels.iter()).enumerate() {
                let pc = ic_pc_map.get(ic).ok_or_eyre(format!("no pc found at {ic}"))?;
                let opcode = revm::interpreter::OpCode::new(code[pc])
                    .ok_or_eyre(format!("invalid opcode: {}", code[pc]))?;

                let mut opcode_str = if opcode.is_push() {
                    format!(
                        "PUSH{} {}",
                        code[pc] - revm::interpreter::opcode::PUSH0,
                        crate::utils::opcode::get_push_value(code, pc)?
                    )
                } else {
                    format!("{opcode}")
                };
                if opcode_str.len() > 30 {
                    opcode_str = format!("{}...", &opcode_str[..27]);
                }

                trace!(
                    "ic: {:05} | pc: {:05} | opcode: {:<30} | source element: {:<20} | label: {}",
                    ic,
                    pc,
                    opcode_str,
                    src.to_string(),
                    label
                );
            }

            // A debugging check to see if there are any labels that only have push opcodes.
            let mut reverse_map = std::collections::HashMap::new();
            for (ic, label) in source_labels.iter().enumerate().filter(|(.., l)| l.is_source()) {
                let pc = ic_pc_map.get(ic).ok_or_eyre(format!("no pc found at {ic}"))?;
                let opcode = revm::interpreter::OpCode::new(code[pc])
                    .ok_or_eyre(format!("invalid opcode: {}", code[pc]))?;
                reverse_map.entry(label.clone()).or_insert_with(Vec::new).push((opcode, pc, ic));
            }
            for (label, opcodes) in reverse_map {
                if opcodes.iter().all(|(opcode, ..)| opcode.is_push()) {
                    trace!("find a label with only push opcodes: {} ({:?})", label, opcodes);
                }
            }
        }

        Ok(source_labels)
    }
}
