//! # edb-debug-backend
//!
//! EDB's core debugging backend.

#[macro_use]
extern crate tracing;

pub mod analysis;
pub mod artifact;
mod core;
pub mod utils;

pub use core::DebugBackend;
use std::fmt::Display;

use alloy_primitives::Address;
use revm::interpreter::OpCode;
use utils::opcode::{IcPcMap, PcIcMap};

/// Runtime Address. It can be either a constructor address or a deployed address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct RuntimeAddress {
    pub address: Address,
    pub is_constructor: bool,
}

impl Display for RuntimeAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_constructor {
            write!(f, "constructor@{}", self.address)
        } else {
            write!(f, "runtime@{}", self.address)
        }
    }
}

impl RuntimeAddress {
    pub fn new(address: Address, is_constructor: bool) -> Self {
        Self { address, is_constructor }
    }

    pub fn constructor(address: Address) -> Self {
        Self::new(address, true)
    }

    pub fn deployed(address: Address) -> Self {
        Self::new(address, false)
    }
}

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
