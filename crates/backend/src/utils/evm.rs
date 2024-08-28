//! Utils

use alloy_primitives::Bytes;
use alloy_sol_types::SolError;
use revm::{
    inspector_handle_register,
    primitives::{EnvWithHandlerCfg, SpecId},
    Context, Database, Evm, EvmContext, Handler, Inspector,
};

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
