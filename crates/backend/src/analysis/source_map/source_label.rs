use std::fmt::{self, Debug};

use eyre::{OptionExt, Result};
use foundry_compilers::artifacts::sourcemap::Jump;
use revm::interpreter::OpCode;

use super::AnalysisStore;
use crate::{
    analysis::source_map::{debug_unit::DebugUnit, CONSTRUCTOR_IDX, DEPLOYED_IDX},
    artifact::deploy::DeployArtifact,
    utils::opcode::get_push_value,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceLabel {
    FunctionCall,
    FunctionReturn,
    UnconditionalJump,
    ConditionalJump,
    SourceStatment(DebugUnit, DebugUnit, DebugUnit),
    Tag(DebugUnit),
    Others(Option<DebugUnit>),
}

impl fmt::Display for SourceLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceLabel::FunctionCall => write!(f, "Interprocedural Call"),
            SourceLabel::FunctionReturn => write!(f, "Interprocedural Return"),
            SourceLabel::SourceStatment(unit, _, _) => {
                write!(f, "SourceStatment({})", unit.loc())
            }
            SourceLabel::UnconditionalJump => write!(f, "Unconditional Jump"),
            SourceLabel::ConditionalJump => write!(f, "Conditional Jump"),
            SourceLabel::Tag(unit) => {
                write!(f, "Tag({})", unit.loc())
            }
            SourceLabel::Others(Some(unit)) => {
                write!(f, "Others({})", unit.loc())
            }
            SourceLabel::Others(_) => write!(f, "Others"),
        }
    }
}

impl SourceLabel {
    pub fn is_source_statement(&self) -> bool {
        matches!(self, SourceLabel::SourceStatment(_, _, _))
    }

    pub fn is_interprocedural_action(&self) -> bool {
        self.is_interprocedural_call() || self.is_interprocedural_return()
    }

    pub fn is_interprocedural_call(&self) -> bool {
        matches!(self, SourceLabel::FunctionCall)
    }

    pub fn is_interprocedural_return(&self) -> bool {
        matches!(self, SourceLabel::FunctionReturn)
    }

    pub fn is_others(&self) -> Option<&DebugUnit> {
        match self {
            SourceLabel::Others(Some(unit)) => Some(unit),
            _ => None,
        }
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
        let bytecode = store.bytecode()?.get(IDX).ok_or_eyre("no bytecode found")?;
        let ic_pc_map = store.ic_pc_map()?.get(IDX).ok_or_eyre("no ic_pc_map found")?;
        let units = store.debug_units()?;

        let source_map = bytecode.source_map().ok_or_eyre("no source map found")??;
        trace!("the number of the original source map entries is {}", source_map.len());

        let mut source_labels = Vec::with_capacity(source_map.len());

        let code = bytecode.bytes().ok_or_eyre("no code found")?.as_ref();
        trace!("the number of instructions is {}", ic_pc_map.len());

        for (ic, src) in source_map.iter().enumerate() {
            trace!("ic: {}, source element: {:?}", ic, src);

            // By default, we will assume the opcode is generated from the compiler
            source_labels.push(SourceLabel::Others(None));

            // Get file index
            let Some(index) = src.index() else { continue };
            let index = index as usize;

            // Get the file units
            let Some(file_units) = units.units_per_file(index) else { continue };
            let Some((_, unit)) = file_units.range(..src.offset() as usize + 1).next_back() else {
                continue;
            };

            // Get the program counter
            let pc = ic_pc_map.get(ic).ok_or_eyre(format!("no pc found at {}", ic))?;
            let opcode =
                OpCode::new(code[pc]).ok_or_eyre(format!("invalid opcode: {}", code[pc]))?;
            trace!("pc: {}, opcode: {}", pc, opcode);

            // Check whether it is an interprocedural call or return
            if opcode == OpCode::JUMP {
                match src.jump() {
                    Jump::In => {
                        *source_labels.last_mut().expect("this cannot happen") =
                            SourceLabel::FunctionCall;
                    }
                    Jump::Out => {
                        *source_labels.last_mut().expect("this cannot happen") =
                            SourceLabel::FunctionReturn;
                    }
                    Jump::Regular => {
                        *source_labels.last_mut().expect("this cannot happen") =
                            SourceLabel::UnconditionalJump;
                    }
                }

                continue;
            } else if opcode == OpCode::JUMPI {
                *source_labels.last_mut().expect("this cannot happen") =
                    SourceLabel::ConditionalJump;

                continue;
            }

            // Check the source statement
            if unit.matches(src.offset() as usize, src.length() as usize) &&
                !unit.is_execution_unit()
            {
                *source_labels.last_mut().expect("this cannot happen") =
                    SourceLabel::Tag(unit.clone());
                continue
            }

            if unit.contains(src.offset() as usize, src.length() as usize) {
                match &unit {
                    DebugUnit::Primitive(_) => {
                        let function = units.function(&unit).ok_or_eyre("no function found")?;
                        let contract = units.contract(&unit).ok_or_eyre("no contract found")?;
                        *source_labels.last_mut().expect("this cannot happen") =
                            SourceLabel::SourceStatment(
                                unit.clone(),
                                function.clone(),
                                contract.clone(),
                            );
                    }
                    DebugUnit::Function(_, _) | DebugUnit::Contract(_) => {
                        *source_labels.last_mut().expect("this cannot happen") =
                            SourceLabel::Others(Some(unit.clone()));
                    }
                    _ => {}
                }
            }
        }

        #[cfg(debug_assertions)]
        for (ic, (src, label)) in source_map.iter().zip(source_labels.iter()).enumerate() {
            let pc = ic_pc_map.get(ic).ok_or_eyre(format!("no pc found at {}", ic))?;
            let opcode =
                OpCode::new(code[pc]).ok_or_eyre(format!("invalid opcode: {}", code[pc]))?;
            let opcode_str = if opcode.is_push() {
                format!(
                    "PUSH{} {}",
                    code[pc] - revm::interpreter::opcode::PUSH0,
                    get_push_value(code, pc)?
                )
            } else {
                format!("{}", opcode)
            };

            trace!(
                "ic: {:05} | pc: {:05} | opcode: {:<30} | source element: {:<20} | label: {}",
                ic,
                pc,
                opcode_str,
                src.to_string(),
                label
            );
        }

        Ok(source_labels)
    }
}
