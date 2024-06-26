//! Utils

use alloy_primitives::Bytes;
use alloy_sol_types::SolError;
use revm::{
    inspector_handle_register,
    interpreter::OpCode,
    primitives::{EnvWithHandlerCfg, SpecId},
    Context, Database, Evm, EvmContext, Handler, Inspector,
};

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

/// Creates a new EVM with the given inspector.
#[inline]
pub fn new_evm_with_inspector<'a, DB, I>(
    db: DB,
    env: EnvWithHandlerCfg,
    inspector: I,
) -> revm::Evm<'a, I, DB>
where
    DB: Database,
    I: Inspector<DB>,
{
    let EnvWithHandlerCfg { env, handler_cfg } = env;

    let context = Context::new(EvmContext::new_with_env(db, env), inspector);
    let mut handler = Handler::new(handler_cfg);
    handler.append_handler_register_plain(inspector_handle_register);
    Evm::new(context, handler)
}
