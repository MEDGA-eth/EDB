use std::collections::{HashMap, HashSet};

use alloy_primitives::{Address, Bytes};
use alloy_sol_types::SolError;
use revm::{
    interpreter::{
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, Gas, InstructionResult,
        InterpreterResult,
    },
    primitives::CreateScheme,
    Database, EvmContext, Inspector,
};

#[derive(Debug)]
pub struct VisitedAddrInspector<'a> {
    pub addresses: &'a mut HashSet<Address>,
    pub creation_codes: &'a mut HashMap<Address, (Bytes, CreateScheme)>,
}

impl<'a> VisitedAddrInspector<'a> {
    pub fn new(
        addresses: &'a mut HashSet<Address>,
        creation_codes: &'a mut HashMap<Address, (Bytes, CreateScheme)>,
    ) -> Self {
        Self { addresses, creation_codes }
    }
}

impl<'a, DB> Inspector<DB> for VisitedAddrInspector<'a>
where
    DB: Database,
    DB::Error: std::error::Error,
{
    #[inline]
    fn call(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        let address = inputs.bytecode_address;

        // check whether it is an EoA
        if let Ok((account, _)) = context.load_account(address) {
            if account.info.is_empty_code_hash() {
                return None;
            }
        }

        // update addresses
        self.addresses.insert(address);

        None
    }

    #[inline]
    fn create(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        if let Err(err) = context.load_account(inputs.caller) {
            let gas = Gas::new(inputs.gas_limit);
            return Some(CreateOutcome::new(
                InterpreterResult {
                    result: InstructionResult::Revert,
                    output: alloy_sol_types::Revert::from(err.to_string()).abi_encode().into(),
                    gas,
                },
                None,
            ));
        }

        let nonce = context.journaled_state.account(inputs.caller).info.nonce;
        let address = inputs.created_address(nonce);

        self.addresses.insert(address);
        self.creation_codes.insert(address, (inputs.init_code.clone(), inputs.scheme));

        None
    }
}
