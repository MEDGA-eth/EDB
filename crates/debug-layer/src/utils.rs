//! Utils

use revm::interpreter::OpCode;

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
