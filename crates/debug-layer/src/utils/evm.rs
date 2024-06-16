//! Utils

use alloy_primitives::Bytes;
use alloy_sol_types::SolError;
use revm::{interpreter::OpCode, primitives::SpecId};

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

/// Get the gas used, accounting for refunds
#[inline]
pub fn gas_used(spec: SpecId, spent: u64, refunded: u64) -> u64 {
    let refund_quotient = if SpecId::enabled(spec, SpecId::LONDON) { 5 } else { 2 };
    spent - (refunded).min(spent / refund_quotient)
}

/// Get the encoded revert data
#[inline]
pub fn abi_encode_revert<T: std::error::Error>(err: &T) -> Bytes {
    alloy_sol_types::Revert::from(err.to_string()).abi_encode().into()
}
