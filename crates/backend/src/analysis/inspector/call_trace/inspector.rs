//! Inspector to construct the dynamic call graph.

use std::collections::BTreeMap;

use alloy_primitives::U256;
use revm::{
    interpreter::{
        opcode::{JUMP, JUMPI},
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, EOFCreateInputs, InstructionResult,
        Interpreter, OpCode,
    },
    Database, EvmContext, Inspector,
};

use crate::{
    analysis::inspector::{
        call_trace::{BlockNode, Callsite, FuncNode},
        push_jump::{JumpHint, PJHint},
    },
    artifact::onchain::AnalyzedBytecode,
    RuntimeAddress,
};

use super::{AnalyzedCallTrace, Edge};

const VALIDATION_CALL_DEPTH: usize = 25;

#[derive(Debug)]
pub struct CallTraceInspector<'a, DB> {
    // The information needed to construct the call graph.
    //
    // The information about the push-jump instructions.
    push_jump_hint: &'a BTreeMap<RuntimeAddress, PJHint>,
    // The analyzed bytecodes.
    bytecodes: &'a BTreeMap<RuntimeAddress, AnalyzedBytecode>,

    // The call graph to be constructed.
    call_trace: AnalyzedCallTrace,

    // Runtime state.
    //
    // The current function node.
    cur_node: Option<usize>,
    // The start `ic` (instruction count) of the current block.
    cur_block_start: usize,
    // The current `ic` (instruction count).
    cur_ic: usize,
    // The current step in the entire process.
    cur_step: usize,

    // Phantom data.
    phantom: std::marker::PhantomData<DB>,
}

impl<'a, DB> CallTraceInspector<'a, DB>
where
    DB: Database,
{
    pub fn extract(mut self) -> AnalyzedCallTrace {
        self.call_trace.apply_lazy_updates();
        self.call_trace
    }

    pub fn new(
        push_jump_info: &'a BTreeMap<RuntimeAddress, PJHint>,
        bytecodes: &'a BTreeMap<RuntimeAddress, AnalyzedBytecode>,
    ) -> Self {
        Self {
            push_jump_hint: push_jump_info,
            bytecodes,

            call_trace: AnalyzedCallTrace::default(),

            cur_node: None,
            cur_block_start: 0,
            cur_ic: 0,
            cur_step: 0,

            phantom: Default::default(),
        }
    }

    /// Exit from the current function.
    /// - ret: The return status of the function. Only `Some` if it is a message call.
    /// - ic: The instruction count of the return. Only `Some` if it is an intra-contract return.
    fn exit(&mut self, ret: Option<InstructionResult>, ic: Option<usize>) {
        trace!(addr=?self.get_current_address(), ret=?ret, ic=?ic, "exit");

        debug_assert!(ic.is_some() ^ ret.is_some());

        let cur_node = self.cur_node.expect("exit without entering");
        let cur_node = &mut self.call_trace.nodes[cur_node];

        // Update the current node.
        debug_assert!(cur_node.ret.is_none());
        cur_node.ret = ret;
        cur_node.trace.push(BlockNode::new(
            cur_node.addr,
            self.cur_block_start,
            self.cur_ic,
            self.cur_step,
        ));

        if cur_node.parent.is_none() {
            // We are at the root node.
            return;
        }

        if let Some(ic) = ic {
            // This is an intra-contract return.
            let (parent_id, callsite) =
                cur_node.parent.expect("intra-contract return without parent");
            debug_assert!(callsite.edge.is_intra_contract());

            self.cur_node = Some(parent_id);
            self.cur_ic = ic;
            self.cur_block_start = ic;
        } else {
            // This is a message call return.
            let mut cur_id = cur_node.loc;
            while let Some((parent_id, callsite)) = self.call_trace.nodes[cur_id].parent {
                if callsite.edge.is_message_call() {
                    self.cur_node = Some(parent_id);
                    self.cur_block_start = self.call_trace.nodes[parent_id]
                        .trace
                        .last()
                        .expect("message call return without trace")
                        .next_block_ic();
                    self.cur_ic = self.cur_block_start;
                    break;
                }

                cur_id = parent_id;
            }
        }
    }

    fn enter(&mut self, addr: RuntimeAddress, ic: usize, edge: Edge) {
        trace!(addr=?addr, cur_ic=self.cur_ic, ic=ic, edge=?edge, "enter");

        if let Some(parent_id) = self.cur_node {
            // Get the new node.
            let parent = &self.call_trace.nodes[parent_id];
            let loc = self.call_trace.nodes.len();
            let node = FuncNode {
                loc,
                parent: Some((parent_id, Callsite::new(self.cur_ic, edge))),
                child_index: parent.children.len(),
                addr,
                ..Default::default()
            };

            // Insert the new node.
            self.call_trace.nodes.push(node);

            // Update the parent node's children.
            let parent = &mut self.call_trace.nodes[parent_id];
            parent.children.push((loc, Callsite::new(self.cur_ic, edge)));

            // Update the parent node's trace.
            let mut block =
                BlockNode::new(parent.addr, self.cur_block_start, self.cur_ic, self.cur_step);
            block.call_to = Some(loc);
            parent.trace.push(block);

            // Update the current node.
            self.cur_node = Some(loc);
            self.cur_block_start = ic;
            self.cur_ic = ic;
        } else {
            // If the current node is None, then this is the first node.
            debug_assert!(ic == 0);
            debug_assert!(edge.is_message_call());

            let node = FuncNode { addr, ..Default::default() };
            self.call_trace.nodes.push(node);

            self.cur_node = Some(0);
            self.cur_block_start = ic;
            self.cur_ic = ic;
        }
    }

    #[inline]
    fn get_current_address(&self) -> RuntimeAddress {
        self.call_trace.nodes[self.cur_node.expect("get_current_address without entering")].addr
    }

    #[inline]
    fn get_current_bytecode(&self) -> &AnalyzedBytecode {
        self.bytecodes
            .get(&self.get_current_address())
            .expect("get_current_bytecode without entering")
    }

    #[inline]
    fn get_jump_hint(&self, addr: RuntimeAddress, pc: usize) -> JumpHint {
        let push_jump_hint = self.push_jump_hint.get(&addr).expect("invalid address");
        *push_jump_hint.jump_hints.get(&pc).expect("invalid ic")
    }

    #[inline]
    fn validate_call(&self, interp: &mut Interpreter, jmp_pc: usize) -> bool {
        for i in 0..VALIDATION_CALL_DEPTH {
            let ret_pc = U256::from(jmp_pc + 1);
            if let Ok(val) = interp.stack().peek(i) {
                if val == ret_pc {
                    return true;
                }
            } else {
                break;
            }
        }

        trace!(addr=?self.get_current_address(), jmp_pc=jmp_pc, "incorrent call hint");
        false
    }

    #[inline]
    fn validate_return(&self, dest_pc: usize) -> bool {
        let dest_ic = self.get_current_bytecode().pc_ic_map.get(dest_pc).expect("invalid pc");
        let mut cur_id = self.cur_node.expect("validate_return without entering");
        while let Some((parent_id, callsite)) = self.call_trace.nodes[cur_id].parent {
            if callsite.edge.is_message_call() {
                break;
            }

            if self.call_trace.nodes[parent_id]
                .trace
                .last()
                .expect("validate_return without trace")
                .next_block_ic() ==
                dest_ic
            {
                return true;
            }

            cur_id = parent_id;
        }

        false
    }

    #[inline]
    /// Flatten the call trace by the return instruction. This is used to refine the call trace.
    fn flatten_call_trace_by_return(&mut self, dest_pc: usize) {
        let dest_ic = self.get_current_bytecode().pc_ic_map.get(dest_pc).expect("invalid pc");

        debug_assert!(
            self.validate_return(dest_pc),
            "this function should be called after validation"
        );

        let cur_id = self.cur_node.expect("refine_call_trace_by_return without entering");
        let (parent_id, callsite) = self.call_trace.nodes[cur_id].parent.unwrap();

        if callsite.edge.is_message_call() {
            // If the parent is a message call, then we should not flatten the call trace.
            warn!(addr=?self.get_current_address(), dest_pc=dest_pc, "call trace can only be flattened by intra-contract return");
            return;
        }

        if self.call_trace.nodes[parent_id]
            .trace
            .last()
            .expect("validate_return without trace")
            .next_block_ic() !=
            dest_ic
        {
            warn!(addr=?self.get_current_address(), cur_ic=self.cur_ic, dest_ic=dest_ic, dest_pc=dest_pc, "flatten call trace by return");

            debug_assert!(parent_id < cur_id);

            let (left, right) = self.call_trace.nodes.split_at_mut(cur_id);
            let parent = &mut left[parent_id];
            let cur_node = &mut right[0];
            debug_assert!(parent.addr == cur_node.addr);

            // Remove the current node from the parent.
            parent.children.pop();
            cur_node.parent = None;
            cur_node.child_index = 0;

            // Discard the current node.
            cur_node.discard = true;

            // Move trace from the current node to the parent.
            parent.trace.append(&mut cur_node.trace);

            // Move children from the current node to the parent.
            parent.children.append(&mut cur_node.children);

            // Refine the parent and child_loc of the children.
            for (loc, (child_id, callsite)) in parent.children.iter_mut().enumerate() {
                if *child_id <= cur_id {
                    // Those children are not affected.
                    continue;
                }

                let child = &mut right[*child_id - cur_id];

                debug_assert!(parent.addr == child.addr);
                child.parent = Some((parent_id, *callsite));
                child.child_index = loc;
            }

            // Update the current node and continue to the parent.
            self.cur_node = Some(parent_id);
            self.flatten_call_trace_by_return(dest_pc);
        }
    }
}

impl<'a, DB> Inspector<DB> for CallTraceInspector<'a, DB>
where
    DB: Database,
    DB::Error: std::error::Error,
{
    fn step_end(&mut self, _interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        self.cur_step += 1;
    }

    fn step(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        let pc = interp.program_counter();
        let ic = self.get_current_bytecode().pc_ic_map.get(pc).expect("invalid pc");
        let op = interp.current_opcode();

        // Update the runtime state.
        self.cur_ic = ic;
        let cur_addr = self.get_current_address();
        let cur_bytecode = self.get_current_bytecode();

        if op == JUMP {
            let dest_pc = interp.stack().peek(0).expect("call without dest").to();
            let dest_ic = cur_bytecode.pc_ic_map.get(dest_pc).expect("invalid pc");
            trace!(addr=?cur_addr, ic=ic, op=?OpCode::new(op), cur_block_start=self.cur_block_start, dest_ic=dest_ic, "JUMP");

            if self.get_jump_hint(cur_addr, pc) == JumpHint::Call && self.validate_call(interp, pc)
            {
                self.enter(cur_addr, dest_ic, Edge::IntraContract);
            } else if self.validate_return(dest_pc) {
                self.flatten_call_trace_by_return(dest_pc);
                self.exit(None, Some(dest_ic));
            } else {
                // Push the current block to the trace.
                let cur_node = &mut self.call_trace.nodes
                    [self.cur_node.expect("get_current_node_mut without entering")];
                cur_node.trace.push(BlockNode::new(
                    cur_node.addr,
                    self.cur_block_start,
                    self.cur_ic,
                    self.cur_step,
                ));
                self.cur_block_start = dest_ic;
            }
        } else if op == JUMPI {
            let dest_pc = interp.stack().peek(0).expect("call without dest").to();
            let dest_ic = cur_bytecode.pc_ic_map.get(dest_pc).expect("invalid pc");
            let cond = interp.stack().peek(1).expect("call without cond");
            trace!(addr=?cur_addr, ic=ic, op=?OpCode::new(op), cur_block_start=self.cur_block_start, dest_ic=dest_ic, cond=?cond, "JUMPI");

            let cur_node = &mut self.call_trace.nodes
                [self.cur_node.expect("get_current_node_mut without entering")];
            cur_node.trace.push(BlockNode::new(
                cur_node.addr,
                self.cur_block_start,
                self.cur_ic,
                self.cur_step,
            ));

            self.cur_block_start = if cond.is_zero() {
                // The jump is not taken.
                self.cur_ic + 1
            } else {
                // The jump is taken.
                dest_ic
            };
        }
    }

    fn call(
        &mut self,
        _context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        let addr = RuntimeAddress::deployed(inputs.bytecode_address);
        let edge = Edge::MessageCall(inputs.scheme.into());

        self.enter(addr, 0, edge);

        None
    }

    fn call_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        self.exit(Some(outcome.result.result), None);
        outcome
    }

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
        let addr = RuntimeAddress::constructor(inputs.created_address(nonce));
        let edge = Edge::MessageCall(inputs.scheme.into());

        self.enter(addr, 0, edge);

        None
    }

    fn create_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        self.exit(Some(outcome.result.result), None);
        outcome
    }

    fn eofcreate(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &mut EOFCreateInputs,
    ) -> Option<CreateOutcome> {
        unimplemented!("EOF create has not been merged into the mainnet");
    }

    fn eofcreate_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &EOFCreateInputs,
        _outcome: CreateOutcome,
    ) -> CreateOutcome {
        unimplemented!("EOF create has not been merged into the mainnet");
    }
}
