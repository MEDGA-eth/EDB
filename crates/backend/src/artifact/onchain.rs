use revm::interpreter::OpCode;

use crate::utils::opcode::{IcPcMap, PcIcMap};

/// Analyzed Bytecode which containt the mapping between the instruction counter and the program
/// counter.
#[derive(Debug, Clone)]
pub struct AnalyzedBytecode {
    pub code: Vec<u8>,
    pub pc_ic_map: PcIcMap,
    pub ic_pc_map: IcPcMap,
}

impl AnalyzedBytecode {
    pub fn new(code: &[u8]) -> Self {
        let pc_ic_map = PcIcMap::new(code);
        let ic_pc_map = IcPcMap::new(code);

        Self { code: code.to_vec(), pc_ic_map, ic_pc_map }
    }

    pub fn len(&self) -> usize {
        self.code.len()
    }

    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }

    pub fn inst_n(&self) -> usize {
        self.ic_pc_map.len()
    }

    pub fn get_opcode_at_pc(&self, pc: usize) -> Option<OpCode> {
        self.code.get(pc).and_then(|&byte| OpCode::new(byte))
    }

    pub fn get_opcode_at_ic(&self, ic: usize) -> Option<OpCode> {
        let pc = self.ic_pc_map.get(ic)?;
        self.get_opcode_at_pc(pc)
    }

    pub fn next_insn_pc(&self, pc: usize) -> Option<usize> {
        if pc >= self.code.len() {
            return None;
        }

        self.pc_ic_map.get(pc).and_then(|ic| self.ic_pc_map.get(ic + 1))
    }

    pub fn prev_insn_pc(&self, pc: usize) -> Option<usize> {
        if pc == 0 {
            return None;
        }

        self.pc_ic_map.get(pc).and_then(|ic| self.ic_pc_map.get(ic - 1))
    }
}
