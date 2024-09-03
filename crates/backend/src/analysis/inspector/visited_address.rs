use std::collections::BTreeMap;

use alloy_primitives::Address;
use revm::{
    interpreter::{
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, EOFCreateInputs, Interpreter,
    },
    primitives::CreateScheme,
    Database, EvmContext, Inspector,
};

use super::AssertionUnwrap;
use crate::{artifact::onchain::AnalyzedBytecode, RuntimeAddress};

#[derive(Debug)]
pub struct VisitedAddrInspector<'a> {
    pub addresses: &'a mut BTreeMap<RuntimeAddress, AnalyzedBytecode>,
    pub creation_scheme: &'a mut BTreeMap<Address, CreateScheme>,
    pub stack: Vec<RuntimeAddress>,
}

impl<'a> VisitedAddrInspector<'a> {
    pub fn new(
        addresses: &'a mut BTreeMap<RuntimeAddress, AnalyzedBytecode>,
        creation_scheme: &'a mut BTreeMap<Address, CreateScheme>,
    ) -> Self {
        Self { addresses, creation_scheme, stack: Vec::new() }
    }
}

impl<'a, DB> Inspector<DB> for VisitedAddrInspector<'a>
where
    DB: Database,
    DB::Error: std::error::Error,
{
    #[inline]
    fn step(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        let Some(&addr) = self.stack.last() else {
            debug_assert!(false, "stack is empty");
            return;
        };

        if self.addresses.contains_key(&addr) {
            return;
        }

        trace!(addr=?addr, "analyze bytecode");
        let code = interp.bytecode.as_ref();
        self.addresses.insert(addr, AnalyzedBytecode::new(code));
        trace!(addr=?addr, "analyze bytecode done");
    }

    #[inline]
    fn call(
        &mut self,
        _context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        let address = inputs.bytecode_address;

        self.stack.push(RuntimeAddress::deployed(address));

        None
    }

    #[inline]
    fn call_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        trace!("call end");

        let addr = self.stack.pop().assert_unwrap("stack is empty (call)");
        debug_assert!(!addr.is_constructor, "constructor should not return here");

        outcome
    }

    #[inline]
    fn create(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        // pre-cache the caller account into journaled state
        if let Err(err) = context.load_account(inputs.caller) {
            // We cannot put `context.load_account` into the debug_assert! macro because this
            // assertion will not be triggered in the release mode.
            debug_assert!(false, "load caller account error during contract creation: {err}");
        }

        let nonce = context.journaled_state.account(inputs.caller).info.nonce;
        let addr = inputs.created_address(nonce);

        trace!(depth=?self.stack.len(), addr=?addr, "create contract");

        self.stack.push(RuntimeAddress::constructor(addr));
        self.creation_scheme.insert(addr, inputs.scheme);

        None
    }

    #[inline]
    fn create_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        trace!("create end");

        let addr = self.stack.pop().assert_unwrap("stack is empty (create)");
        debug_assert!(addr.is_constructor, "non-constructor call should not return here");

        outcome
    }

    #[inline]
    fn eofcreate(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &mut EOFCreateInputs,
    ) -> Option<CreateOutcome> {
        // XXX (ZZ): implement this after EOF is merged.
        unimplemented!("EOF create has not been merged into the mainnet");
    }

    #[inline]
    fn eofcreate_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &EOFCreateInputs,
        _outcome: CreateOutcome,
    ) -> CreateOutcome {
        // XXX (ZZ): implement this after EOF is merged.
        unimplemented!("EOF create has not been merged into the mainnet");
    }
}
