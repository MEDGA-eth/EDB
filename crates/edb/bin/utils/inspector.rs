use std::ops::{Deref, DerefMut};

use alloy_primitives::{Address, Log, U256};
use foundry_evm::InspectorExt;
use revm::{
    interpreter::{
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, EOFCreateInputs, Interpreter,
    },
    Database, EvmContext, Inspector,
};

/// Inspector wrapper that implements `InspectorExt` trait.
/// This is useful when you want to implement `InspectorExt` trait for a struct that already
/// implements `Inspector` trait. This code is mainly for the purpose of testing.
#[allow(unused)]
#[derive(Debug, Clone, Default)]
pub struct InspectorWrapper<I>(pub I);

impl<I> Deref for InspectorWrapper<I> {
    type Target = I;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<I> DerefMut for InspectorWrapper<I> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<DB: Database, I: Inspector<DB>> InspectorExt<DB> for InspectorWrapper<I> {}

impl<DB: Database, I: Inspector<DB>> Inspector<DB> for InspectorWrapper<I> {
    #[inline]
    fn initialize_interp(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        self.0.initialize_interp(interp, context)
    }

    #[inline]
    fn step(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        self.0.step(interp, context)
    }

    #[inline]
    fn step_end(&mut self, interp: &mut Interpreter, context: &mut EvmContext<DB>) {
        self.0.step_end(interp, context)
    }

    #[inline]
    fn call_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        self.0.call_end(context, inputs, outcome)
    }

    #[inline]
    fn create_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        self.0.create_end(context, inputs, outcome)
    }

    #[inline]
    fn call(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        self.0.call(context, inputs)
    }

    #[inline]
    fn create(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        self.0.create(context, inputs)
    }

    #[inline]
    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        self.0.selfdestruct(contract, target, value)
    }

    #[inline]
    fn log(&mut self, context: &mut EvmContext<DB>, log: &Log) {
        self.0.log(context, log)
    }

    #[inline]
    fn eofcreate(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &mut EOFCreateInputs,
    ) -> Option<CreateOutcome> {
        self.0.eofcreate(context, inputs)
    }

    #[inline]
    fn eofcreate_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &EOFCreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        self.0.eofcreate_end(context, inputs, outcome)
    }
}
