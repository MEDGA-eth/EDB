use alloy_primitives::Address;
use alloy_sol_types::SolError;
use arrayvec::ArrayVec;
use revm::{
    interpreter::{
        opcode, CallInputs, CallOutcome, CreateInputs, CreateOutcome, Gas, InstructionResult,
        Interpreter, InterpreterResult,
    },
    Database, EvmContext, Inspector,
};
use revm_inspectors::tracing::types::CallKind;

use crate::{
    artifact::debug::{DebugArena, DebugNode, DebugStep},
    utils::evm,
};

#[derive(Debug)]
pub struct DebugInspector<DB> {
    /// The arena of [DebugNode]s
    pub arena: DebugArena,
    /// The ID of the current [DebugNode].
    pub head: usize,
    /// The current execution address.
    pub context: Address,

    phantom: std::marker::PhantomData<DB>,
}

impl<DB> DebugInspector<DB>
where
    DB: Database,
{
    /// Creates a new [DebugInspector].
    pub fn new() -> Self {
        Self {
            arena: DebugArena::default(),
            head: 0,
            context: Address::default(),
            phantom: Default::default(),
        }
    }

    /// Enters a new execution context.
    pub fn enter(&mut self, depth: usize, address: Address, kind: CallKind) {
        self.context = address;
        self.head = self.arena.push_node(DebugNode { depth, address, kind, ..Default::default() });
    }

    /// Exits the current execution context, replacing it with the previous one.
    pub fn exit(&mut self) {
        if let Some(parent_id) = self.arena.arena[self.head].parent {
            let DebugNode { depth, address, kind, .. } = self.arena.arena[parent_id];
            self.enter(depth, address, kind);
        }
    }
}

impl<DB> Inspector<DB> for DebugInspector<DB>
where
    DB: Database,
    DB::Error: std::error::Error,
{
    fn step(&mut self, interp: &mut Interpreter, ecx: &mut EvmContext<DB>) {
        let pc = interp.program_counter();
        let op = interp.current_opcode();

        // Extract the push bytes
        let push_size = if (opcode::PUSH1..=opcode::PUSH32).contains(&op) {
            (op - opcode::PUSH0) as usize
        } else {
            0
        };
        let push_bytes = (push_size > 0).then(|| {
            let start = pc + 1;
            let end = start + push_size;
            let slice = &interp.contract.bytecode.bytecode()[start..end];
            debug_assert!(slice.len() <= 32);
            let mut array = ArrayVec::new();
            array.try_extend_from_slice(slice).unwrap();
            array
        });

        let total_gas_used = evm::gas_used(
            ecx.spec_id(),
            interp.gas.limit().saturating_sub(interp.gas.remaining()),
            interp.gas.refunded() as u64,
        );

        // Reuse the memory from the previous step if the previous opcode did not modify it.
        let memory = self.arena.arena[self.head]
            .steps
            .last()
            .filter(|step| !step.opcode_modifies_memory())
            .map(|step| step.memory.clone())
            .unwrap_or_else(|| interp.shared_memory.context_memory().to_vec().into());

        self.arena.arena[self.head].steps.push(DebugStep {
            pc,
            stack: interp.stack().data().clone(),
            memory,
            calldata: interp.contract().input.clone(),
            returndata: interp.return_data_buffer.clone(),
            instruction: op,
            push_bytes: push_bytes.unwrap_or_default(),
            total_gas_used,
        });
    }

    fn call(&mut self, ecx: &mut EvmContext<DB>, inputs: &mut CallInputs) -> Option<CallOutcome> {
        self.enter(
            ecx.journaled_state.depth() as usize,
            inputs.bytecode_address,
            inputs.scheme.into(),
        );

        None
    }

    fn call_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        self.exit();

        outcome
    }

    fn create(
        &mut self,
        ecx: &mut EvmContext<DB>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        if let Err(err) = ecx.load_account(inputs.caller) {
            let gas = Gas::new(inputs.gas_limit);
            return Some(CreateOutcome::new(
                InterpreterResult {
                    result: InstructionResult::Revert,
                    output: alloy_sol_types::Revert::from(err.to_string()).abi_encode().into(),
                    // output: evm::abi_encode_revert(&err),
                    gas,
                },
                None,
            ));
        }

        let nonce = ecx.journaled_state.account(inputs.caller).info.nonce;
        self.enter(
            ecx.journaled_state.depth() as usize,
            inputs.created_address(nonce),
            CallKind::Create,
        );

        None
    }

    fn create_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        self.exit();

        outcome
    }
}
