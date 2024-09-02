use alloy_primitives::U256;
use eyre::{bail, OptionExt, Result};
use revm::interpreter::{
    opcode::{DUP1, DUP16, POP, PUSH0, PUSH1, PUSH32, SWAP1, SWAP16},
    OpCode,
};
use rustc_hash::FxHashMap;

/// Returns true if the opcode modifies memory.
/// <https://bluealloy.github.io/revm/crates/interpreter/memory.html#opcodes>
/// <https://github.com/crytic/evm-opcodes>
#[inline]
pub const fn is_memory_modifying_opcode(opcode: OpCode) -> bool {
    matches!(
        opcode,
        OpCode::EXTCODECOPY |
            OpCode::MLOAD |
            OpCode::MSTORE |
            OpCode::MSTORE8 |
            OpCode::MCOPY |
            OpCode::CODECOPY |
            OpCode::CALLDATACOPY |
            OpCode::RETURNDATACOPY |
            OpCode::CALL |
            OpCode::CALLCODE |
            OpCode::DELEGATECALL |
            OpCode::STATICCALL
    )
}

#[inline]
pub const fn is_stack_operation_opcode(opcode: OpCode) -> bool {
    matches!(
        opcode.get(),
        POP..=POP | PUSH0..=PUSH32 | DUP1..=DUP16 | SWAP1..=SWAP16
    )
}

#[inline]
pub fn get_push_value(code: &[u8], pc: usize) -> Result<U256> {
    let push_size = (code[pc] - PUSH0) as usize;

    let push_start = pc + 1;
    let push_end = push_start + push_size;
    if push_end > code.len() {
        bail!("push out of bounds")
    } else {
        U256::try_from_be_slice(&code[push_start..push_end])
            .ok_or_eyre(format!("invalid bytes: {:?}", &code[push_start..push_end]))
    }
}

/// Maps from program counter to instruction counter.
///
/// Inverse of [`IcPcMap`].
#[derive(Debug, Clone)]
pub struct PcIcMap {
    pub inner: FxHashMap<usize, usize>,
}

impl PcIcMap {
    /// Creates a new `PcIcMap` for the given code.
    pub fn new(code: &[u8]) -> Self {
        Self { inner: make_map::<true>(code) }
    }

    /// Returns the length of the map.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns the instruction counter for the given program counter.
    pub fn get(&self, pc: usize) -> Option<usize> {
        self.inner.get(&pc).copied()
    }
}

/// Map from instruction counter to program counter.
///
/// Inverse of [`PcIcMap`].
#[derive(Debug, Clone)]
pub struct IcPcMap {
    pub inner: FxHashMap<usize, usize>,
}

impl IcPcMap {
    /// Creates a new `IcPcMap` for the given code.
    pub fn new(code: &[u8]) -> Self {
        Self { inner: make_map::<false>(code) }
    }

    /// Returns the length of the map.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns the program counter for the given instruction counter.
    pub fn get(&self, ic: usize) -> Option<usize> {
        self.inner.get(&ic).copied()
    }
}

fn make_map<const PC_FIRST: bool>(code: &[u8]) -> FxHashMap<usize, usize> {
    let mut map = FxHashMap::default();

    let mut pc = 0;
    let mut cumulative_push_size = 0;
    while pc < code.len() {
        let ic = pc - cumulative_push_size;
        if PC_FIRST {
            map.insert(pc, ic);
        } else {
            map.insert(ic, pc);
        }

        if (PUSH1..=PUSH32).contains(&code[pc]) {
            // Skip the push bytes.
            let push_size = (code[pc] - PUSH0) as usize;
            pc += push_size;
            cumulative_push_size += push_size;
        }

        pc += 1;
    }
    map
}
