use crate::{artifact::deploy::DeployArtifact, utils::opcode::IcPcMap};
use eyre::{OptionExt, Result};
use foundry_compilers::artifacts::{
    sourcemap::{Jump, SourceElement},
    Bytecode,
};
use revm::interpreter::opcode::{JUMP, JUMPI};

use super::{AnalysisStore, CONSTRUCTOR_IDX, DEPLOYED_IDX};

pub struct IntegrityAnalsysis {}

impl IntegrityAnalsysis {
    pub fn analyze(artifact: &DeployArtifact, store: &mut AnalysisStore<'_>) -> Result<()> {
        let source_maps = store.source_map.as_ref().ok_or_eyre("no source map found")?;
        let bytecodes = store.bytecode.as_ref().ok_or_eyre("no bytecode found")?;

        let mut is_corrupted = [false; 2];

        is_corrupted[CONSTRUCTOR_IDX] = Self::check::<CONSTRUCTOR_IDX>(source_maps, bytecodes)?;
        is_corrupted[DEPLOYED_IDX] = Self::check::<DEPLOYED_IDX>(source_maps, bytecodes)?;

        if is_corrupted.iter().any(|&x| x) {
            warn!(addr=?artifact.onchain_address, "source map is corrupted");
        }

        store.is_corrupted = Some(is_corrupted);

        Ok(())
    }

    pub fn check<const IDX: usize>(
        source_maps: &[Vec<SourceElement>; 2],
        bytecodes: &[&Bytecode; 2],
    ) -> Result<bool> {
        let source_map: &[SourceElement] = source_maps[IDX].as_ref();
        let bytecode: &[u8] = bytecodes[IDX].bytes().ok_or_eyre("no bytecode found")?;
        let ic_pc_map = IcPcMap::new(bytecode);

        // Check: if there is any jump label in the source map whose corresponding bytecode is not
        // a JUMP or JUMPI instruction, then the source map is corrupted.
        for (ic, elem) in source_map.iter().enumerate() {
            if elem.jump() != Jump::Regular {
                let pc = ic_pc_map.get(ic).ok_or_eyre("invalid instruction counter")?;
                let opcode = *bytecode.get(pc).ok_or_eyre("invalid program counter")?;
                if opcode != JUMP && opcode != JUMPI {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }
}
