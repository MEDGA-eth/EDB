use std::{
    fmt::{self, Debug},
    ops::{Deref, DerefMut},
    sync::Arc,
};

use eyre::{ensure, OptionExt, Result};
use revm::interpreter::OpCode;

use super::{debug_unit::UnitLocation, AnalysisStore};
use crate::{
    analysis::source_map::{debug_unit::DebugUnit, CONSTRUCTOR_IDX, DEPLOYED_IDX},
    artifact::{deploy::DeployArtifact, onchain::AnalyzedBytecode},
    utils::opcode::{is_jump_related_opcode, is_stack_operation_opcode},
};

const IGNORE_FUNC: fn(OpCode) -> bool =
    |opcode| is_stack_operation_opcode(opcode) || is_jump_related_opcode(opcode);

/// Source Label are the information we extracted from the inaccurate source map.
/// It provides a more reliable way to associate the source code with the bytecode.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
                write!(f, "SourceStatement({})", stmt.loc())
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
                (Some(scope), Some(_)) => write!(f, "Located({})", scope),
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

    /// Get the statement associated with the source label. Note that only the source label that
    /// represents a statement (not the inline assembly) has a statement associated with it.
    pub fn statement(&self) -> Option<&DebugUnit> {
        match self {
            Self::PrimitiveStmt { stmt, .. } => Some(stmt),
            _ => None,
        }
    }

    /// Get the inline assembly block associated with the source label. Note that only the source
    /// label that represents an inline assembly block has an inline assembly block associated with
    /// it.
    pub fn inline_assembly_block(&self) -> Option<&DebugUnit> {
        match self {
            Self::InlineAssembly { block, .. } => Some(block),
            _ => None,
        }
    }

    /// Get the function associated with the source label. Note that only the source label that
    /// represents a statement or an inline assembly block has a function associated with it.
    pub fn function(&self) -> Option<&DebugUnit> {
        match self {
            Self::PrimitiveStmt { func, .. } | Self::InlineAssembly { func, .. } => Some(func),
            _ => None,
        }
    }

    /// Get the contract associated with the source label. Note that only the source label that
    /// represents a statement or an inline assembly block has a contract associated with it.
    pub fn contract(&self) -> Option<&DebugUnit> {
        match self {
            Self::PrimitiveStmt { cntr, .. } | Self::InlineAssembly { cntr, .. } => Some(cntr),
            _ => None,
        }
    }

    /// Get the statement tag associated with the source label. Note that inlined assembly block
    /// does not have a statement tag.
    pub fn statement_tag(&self) -> Option<&DebugUnit> {
        match self {
            Self::PrimitiveStmt { stmt, .. } => Some(stmt),
            Self::Tag { tag } if matches!(tag, DebugUnit::Primitive(..)) => Some(tag),
            _ => None,
        }
    }

    /// Get the function tag associated with the source label. Note that all the source and
    /// non-source labels could potentially have a function tag.
    pub fn function_tag(&self) -> Option<&DebugUnit> {
        match self {
            Self::PrimitiveStmt { func, .. } | Self::InlineAssembly { func, .. } => Some(func),
            Self::Tag { tag } if matches!(tag, DebugUnit::Function(..)) => Some(tag),
            Self::Others { scope, .. } if matches!(scope, Some(DebugUnit::Function(..))) => {
                scope.as_ref()
            }
            _ => None,
        }
    }

    /// Get the contract tag associated with the source label. Note that all the source and
    /// non-source labels could potentially have a contract tag.
    pub fn contract_tag(&self) -> Option<&DebugUnit> {
        match self {
            Self::PrimitiveStmt { cntr, .. } | Self::InlineAssembly { cntr, .. } => Some(cntr),
            Self::Tag { tag } if matches!(tag, DebugUnit::Contract(..)) => Some(tag),
            Self::Others { scope, .. } if matches!(scope, Some(DebugUnit::Contract(..))) => {
                scope.as_ref()
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SourceLabels(Vec<SourceLabel>);

impl Deref for SourceLabels {
    type Target = [SourceLabel];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SourceLabels {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Vec<SourceLabel>> for SourceLabels {
    fn from(labels: Vec<SourceLabel>) -> Self {
        Self(labels)
    }
}

impl SourceLabels {
    pub fn refine(&mut self, bytecode: &AnalyzedBytecode) -> Result<()> {
        let mut reverse_map = std::collections::HashMap::new();
        let code = &bytecode.code;
        let ic_pc_map = &bytecode.ic_pc_map;

        // Calculate the reverse map.
        for (ic, label) in self.iter().enumerate().filter(|(.., l)| l.is_source()) {
            let pc = ic_pc_map.get(ic).ok_or_eyre(format!("no pc found at {ic}"))?;
            let opcode =
                OpCode::new(code[pc]).ok_or_eyre(format!("invalid opcode: {}", code[pc]))?;
            reverse_map.entry(label.clone()).or_insert_with(Vec::new).push((opcode, ic));
        }

        for (label, mut opcodes) in reverse_map.into_iter() {
            // If all the opcodes are stack operations or jump opcode, then we cannot refine the
            // source label.
            if opcodes.iter().all(|(opcode, _)| IGNORE_FUNC(*opcode)) {
                let label = format!("{label}");
                let opcode_vec =
                    opcodes.iter().map(|(op, ic)| (ic, op.as_str())).collect::<Vec<_>>();
                trace!(label=label, opcode=?opcode_vec, "cannot refine the source label");

                // JUMP, JUMPI, and JUMPDEST will still be ignored.
                opcodes.retain(|(opcode, _)| {
                    *opcode == OpCode::JUMP ||
                        *opcode == OpCode::JUMPI ||
                        *opcode == OpCode::JUMPDEST
                });
            }

            // We change the source label to a tag if the opcode is a stack operation or jump
            // opcode.
            for (opcode, ic) in opcodes {
                match label {
                    SourceLabel::PrimitiveStmt { ref stmt, .. } if IGNORE_FUNC(opcode) => {
                        // We change the source label to a tag.
                        self[ic] = SourceLabel::Tag { tag: stmt.clone() };
                    }
                    SourceLabel::InlineAssembly { ref block, .. } if IGNORE_FUNC(opcode) => {
                        // We change the source label to a tag.
                        self[ic] = SourceLabel::Tag { tag: block.clone() };
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }
}

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

        store.source_labels = Some(
            labels
                .into_iter()
                .map(SourceLabels::from)
                .collect::<Vec<_>>()
                .try_into()
                .expect("this cannot happen"),
        );

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
                    DebugUnit::Primitive(..) => {
                        let function = units.function(unit).ok_or_eyre("no function found")?;
                        let contract = units.contract(unit).ok_or_eyre("no contract found")?;
                        *source_labels.last_mut().expect("this cannot happen") =
                            SourceLabel::PrimitiveStmt {
                                stmt: unit.clone(),
                                func: function.clone(),
                                cntr: contract.clone(),
                            };
                    }
                    DebugUnit::Function(..) | DebugUnit::Contract(..) => {
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

        Ok(source_labels)
    }
}
