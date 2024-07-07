use std::fmt::Debug;

use eyre::{ensure, OptionExt, Result};
use foundry_compilers::artifacts::{sourcemap::Jump, Bytecode};
use revm::interpreter::OpCode;

use super::debug_unit::DebugUnits;
use crate::{
    analysis::source_map::debug_unit::DebugUnit, artifact::deploy::DeployArtifact,
    utils::opcode::IcPcMap,
};

#[derive(Debug, Clone)]
pub enum SourceLabel {
    InterproceduralJmp,
    IntermessageAct(OpCode),
    SourceStatment(DebugUnit, DebugUnit, DebugUnit),
    Others(Option<DebugUnit>),
}

#[derive(Debug, Clone)]
pub struct SourceLabels {
    pub deployed: Vec<SourceLabel>,
    pub construction: Vec<SourceLabel>,
}

impl SourceLabel {
    pub fn is_source_statement(&self) -> bool {
        matches!(self, SourceLabel::SourceStatment(_, _, _))
    }

    pub fn is_interprocedural_jmp(&self) -> bool {
        matches!(self, SourceLabel::InterproceduralJmp)
    }

    pub fn is_intermessage_action(&self) -> bool {
        matches!(self, SourceLabel::IntermessageAct(_))
    }

    pub fn is_others(&self) -> Option<&DebugUnit> {
        match self {
            SourceLabel::Others(Some(unit)) => Some(unit),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceLabelAnalysis {}

impl SourceLabelAnalysis {
    pub fn analyze(artifact: &DeployArtifact, units: &DebugUnits) -> Result<SourceLabels> {
        trace!(
            "analyzing source labels, with available file indice: {:?}",
            artifact.sources.keys()
        );
        ensure!(
            !units.iter().any(|unit| matches!(unit, DebugUnit::Hyper(_))),
            "there should not have any hyper unit in the debug units at this stage"
        );

        // Analyze the construction bytecode.
        trace!("analyzing construction bytecode");
        let construction = Self::analyze_bytecode(
            artifact.evm.bytecode.as_ref().ok_or_eyre("no construction bytecode found")?,
            units,
        )?;

        // Analyze the deployed bytecode.
        trace!("analyzing deployed bytecode");
        let deployed = Self::analyze_bytecode(
            artifact
                .evm
                .deployed_bytecode
                .as_ref()
                .and_then(|b| b.bytecode.as_ref())
                .ok_or_eyre("no deployed bytecode found")?,
            units,
        )?;

        Ok(SourceLabels { deployed, construction })
    }

    fn analyze_bytecode(bytecode: &Bytecode, units: &DebugUnits) -> Result<Vec<SourceLabel>> {
        let source_map = bytecode.source_map().ok_or_eyre("no source map found")??;
        trace!("the number of the original source map entries is {}", source_map.len());

        let mut source_labels = Vec::with_capacity(source_map.len());

        let code = bytecode.bytes().ok_or_eyre("no code found")?.as_ref();
        let ic_pc_map = IcPcMap::new(code);
        trace!("the number of instructions is {}", ic_pc_map.len());

        for (ic, src) in source_map.into_iter().enumerate() {
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

            // Check whether it is an interprocedural call/revert/stop
            let pc = ic_pc_map.get(ic).ok_or_eyre(format!("no pc found at {}", ic))?;
            let opcode =
                OpCode::new(code[pc]).ok_or_eyre(format!("invalid opcode: {}", code[pc]))?;
            trace!("pc: {}, opcode: {}", pc, opcode);
            match opcode {
                OpCode::CALL |
                OpCode::CALLCODE |
                OpCode::DELEGATECALL |
                OpCode::STATICCALL |
                OpCode::CREATE |
                OpCode::CREATE2 |
                OpCode::REVERT |
                OpCode::STOP |
                OpCode::INVALID |
                OpCode::RETURN => {
                    *source_labels.last_mut().expect("this cannot happen") =
                        SourceLabel::IntermessageAct(opcode);
                    continue;
                }
                _ => {}
            }

            // Check whether it is an interprocedural jump
            if src.jump() != Jump::Regular {
                // It is an interprocedural jump
                *source_labels.last_mut().expect("this cannot happen") =
                    SourceLabel::InterproceduralJmp;
                continue;
            }

            // Check the source statement
            if unit.matches(src.offset() as usize, src.length() as usize) &&
                !unit.is_execution_unit()
            {
                *source_labels.last_mut().expect("this cannot happen") =
                    SourceLabel::Others(Some(unit.clone()));
                continue
            }

            if unit.contains(src.offset() as usize, src.length() as usize) &&
                matches!(unit, DebugUnit::Primitive(_))
            {
                let function = units.function(&unit).ok_or_eyre("no function found")?;
                let contract = units.contract(&unit).ok_or_eyre("no contract found")?;
                *source_labels.last_mut().expect("this cannot happen") =
                    SourceLabel::SourceStatment(unit.clone(), function.clone(), contract.clone());
            }
        }

        Ok(source_labels)
    }
}
