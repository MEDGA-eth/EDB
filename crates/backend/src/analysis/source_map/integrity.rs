use std::fmt::{self, Display, Formatter};

use crate::{artifact::deploy::DeployArtifact, utils::opcode::IcPcMap};
use alloy_primitives::Address;
use eyre::{OptionExt, Result};
use foundry_compilers::artifacts::{
    sourcemap::{Jump, SourceElement},
    Bytecode,
};
use revm::interpreter::opcode::{JUMP, JUMPI};

use super::{source_label::SourceLabels, AnalysisStore, CONSTRUCTOR_IDX, DEPLOYED_IDX};

pub struct IntegrityAnalsysis {}

const SOURCE_RATIO_THRESHOLD: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntergrityLevel {
    Normal = 0,
    OverOptimized,
    Corrupted,
}

impl Display for IntergrityLevel {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::OverOptimized => write!(f, "over-optimized"),
            Self::Corrupted => write!(f, "corrupted"),
        }
    }
}

impl IntegrityAnalsysis {
    pub fn analyze(artifact: &DeployArtifact, store: &mut AnalysisStore<'_>) -> Result<()> {
        let source_maps = store.source_map.as_ref().ok_or_eyre("no source map found")?;
        let labels = store.source_labels.as_ref().ok_or_eyre("no source labels found")?;
        let bytecodes = store.bytecode.as_ref().ok_or_eyre("no bytecode found")?;
        let addr = artifact.onchain_address;

        let mut is_corrupted = [IntergrityLevel::Normal; 2];

        is_corrupted[CONSTRUCTOR_IDX] =
            Self::check::<CONSTRUCTOR_IDX>(addr, source_maps, labels, bytecodes)?;
        is_corrupted[DEPLOYED_IDX] =
            Self::check::<DEPLOYED_IDX>(addr, source_maps, labels, bytecodes)?;

        if is_corrupted.iter().any(|&x| x != IntergrityLevel::Normal) {
            warn!(addr=?addr, constructor=?is_corrupted[CONSTRUCTOR_IDX], deployed=?is_corrupted[DEPLOYED_IDX], "source map is abnormal");
        }

        store.intergrity_levels = Some(is_corrupted);

        Ok(())
    }

    pub fn check<const IDX: usize>(
        addr: Option<Address>,
        source_maps: &[Vec<SourceElement>; 2],
        labels: &[SourceLabels; 2],
        bytecodes: &[&Bytecode; 2],
    ) -> Result<IntergrityLevel> {
        trace!(addr=?addr, "checking source map integrity");

        let source_map: &[SourceElement] = source_maps[IDX].as_ref();
        let bytecode: &[u8] = bytecodes[IDX].bytes().ok_or_eyre("no bytecode found")?;
        let label = &labels[IDX];
        let ic_pc_map = IcPcMap::new(bytecode);

        // Check: if there is any jump label in the source map whose corresponding bytecode is not
        // a JUMP or JUMPI instruction, then the source map is corrupted.
        for (ic, elem) in source_map.iter().enumerate() {
            if elem.jump() != Jump::Regular {
                let pc = ic_pc_map.get(ic).ok_or_eyre("invalid instruction counter")?;
                let opcode = *bytecode.get(pc).ok_or_eyre("invalid program counter")?;
                if opcode != JUMP && opcode != JUMPI {
                    return Ok(IntergrityLevel::Corrupted);
                }
            }
        }

        // Check: if there are only a few source labels,  then the source map is over optimized.
        // It could happen when the code is compiled `via_ir` with a high optimization round.
        let source_stmts = label.iter().filter(|l| l.is_source()).count();
        if (source_stmts as f64 / source_map.len() as f64) < SOURCE_RATIO_THRESHOLD {
            return Ok(IntergrityLevel::OverOptimized);
        }

        Ok(IntergrityLevel::Normal)
    }
}
