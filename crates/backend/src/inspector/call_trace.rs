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
use revm_inspectors::tracing::types::CallKind;

use crate::{AnalyzedBytecode, RuntimeAddress};

use super::push_jmp::{JumpHint, PJHint};

const VALIDATION_CALL_DEPTH: usize = 25;

#[derive(Default, Debug)]
pub struct CallTrace {
    nodes: Vec<FuncNode>,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Depth {
    pub message: usize,
    pub intra_contract: usize,
}

impl Depth {
    pub fn new_msg(inter_depth: usize) -> Self {
        Self { message: inter_depth, intra_contract: 0 }
    }

    pub fn new_from_parent(parent: &Self, edge: Edge) -> Self {
        match edge {
            Edge::MessageCall(_) => Self::new_msg(parent.message + 1),
            Edge::IntraContract(_) => {
                Self { message: parent.message, intra_contract: parent.intra_contract + 1 }
            }
        }
    }
}

#[derive(Default, Debug)]
pub struct FuncNode {
    /// Location in the whole graph.
    pub loc: usize,
    /// Parent node index in the graph.
    pub parent: Option<(usize, Edge)>,
    /// Children node indexes in the graph.
    pub children: Vec<(usize, Edge)>,
    /// Location in the parent node (i.e., which child is this node).
    pub child_loc: usize,

    /// Call trace within the function.
    pub trace: Vec<BlockNode>,

    /// Return status.
    pub ret: Option<InstructionResult>,

    // Function information.
    /// The address of the code. Note that this is the address of the *code*, not necessarily the
    /// address of the storage.
    pub addr: RuntimeAddress,

    /// The fine-grained depth of the call.
    pub depth: Depth,
}

#[derive(Default, Debug)]
pub struct BlockNode {
    pub start_ic: usize,
    pub inst_n: usize,
}

impl BlockNode {
    pub fn new(start_ic: usize, end_ic: usize) -> Self {
        Self { start_ic, inst_n: end_ic - start_ic + 1 }
    }

    pub fn next_block(&self) -> usize {
        self.start_ic + self.inst_n
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Edge {
    /// The edge is a message call.
    MessageCall(CallKind),
    /// The edge is an intra-contract call. True if it is a call, false if it is a return.
    IntraContract(bool),
}

impl Edge {
    pub fn is_message_call(&self) -> bool {
        matches!(self, Self::MessageCall(_))
    }

    pub fn is_intra_contract(&self) -> bool {
        matches!(self, Self::IntraContract(_))
    }
}

#[derive(Debug)]
pub struct CallTraceInspector<'a, DB> {
    // The information needed to construct the call graph.
    //
    // The information about the push-jump instructions.
    push_jump_hint: &'a BTreeMap<RuntimeAddress, PJHint>,
    // The analyzed bytecodes.
    bytecodes: &'a BTreeMap<RuntimeAddress, AnalyzedBytecode>,

    // The call graph to be constructed.
    call_trace: CallTrace,

    // Runtime state.
    //
    // The current function node.
    cur_node: Option<usize>,
    // The start `ic` (instruction count) of the current block.
    cur_block_start: usize,
    // The current `ic` (instruction count).
    cur_ic: usize,

    // Phantom data.
    phantom: std::marker::PhantomData<DB>,
}

impl<'a, DB> CallTraceInspector<'a, DB>
where
    DB: Database,
{
    pub fn extract(self) -> CallTrace {
        self.call_trace
    }

    pub fn new(
        push_jump_info: &'a BTreeMap<RuntimeAddress, PJHint>,
        bytecodes: &'a BTreeMap<RuntimeAddress, AnalyzedBytecode>,
    ) -> Self {
        Self {
            push_jump_hint: push_jump_info,
            bytecodes,

            call_trace: CallTrace::default(),

            cur_node: None,
            cur_block_start: 0,
            cur_ic: 0,

            phantom: Default::default(),
        }
    }

    /// Exit from the current function.
    /// - ret: The return status of the function. Only `Some` if it is a message call.
    /// - ic: The instruction count of the return. Only `Some` if it is an intra-contract return.
    fn exit(&mut self, ret: Option<InstructionResult>, ic: Option<usize>) {
        debug!(addr=?self.get_current_address(), ret=?ret, ic=?ic, "exit");

        debug_assert!(ic.is_some() ^ ret.is_some());

        let cur_node = self.cur_node.expect("exit without entering");
        let cur_node = &mut self.call_trace.nodes[cur_node];

        // Update the current node.
        debug_assert!(cur_node.ret.is_none());
        cur_node.ret = ret;
        cur_node.trace.push(BlockNode::new(self.cur_block_start, self.cur_ic));

        if cur_node.parent.is_none() {
            // We are at the root node.
            return;
        }

        if let Some(ic) = ic {
            // This is an intra-contract return.
            let (parent_id, edge) = cur_node.parent.expect("intra-contract return without parent");
            debug_assert!(edge.is_intra_contract());

            // TODO (ZZ): make an intra-contract return check.
            self.cur_node = Some(parent_id);
            self.cur_ic = ic;
            self.cur_block_start = ic;
        } else {
            // This is a message call return.
            let mut cur_id = cur_node.loc;
            while let Some((parent_id, edge)) = self.call_trace.nodes[cur_id].parent {
                if edge.is_message_call() {
                    self.cur_node = Some(parent_id);
                    self.cur_block_start = self.call_trace.nodes[parent_id]
                        .trace
                        .last()
                        .expect("message call return without trace")
                        .next_block();
                    self.cur_ic = self.cur_block_start;
                    break;
                }

                cur_id = parent_id;
            }
        }
    }

    fn enter(&mut self, addr: RuntimeAddress, ic: usize, edge: Edge) {
        debug!(addr=?addr, ic=ic, edge=?edge, "enter");

        if let Some(parent_id) = self.cur_node {
            // Get the new node.
            let parent = &self.call_trace.nodes[parent_id];
            let depth = Depth::new_from_parent(&parent.depth, edge);
            let loc = self.call_trace.nodes.len();
            let node = FuncNode {
                loc,
                parent: Some((parent_id, edge)),
                child_loc: parent.children.len(),
                addr,
                depth,
                ..Default::default()
            };

            // Insert the new node.
            self.call_trace.nodes.push(node);

            // Update the parent node.
            let parent = &mut self.call_trace.nodes[parent_id];
            parent.children.push((loc, edge));
            parent.trace.push(BlockNode::new(self.cur_block_start, self.cur_ic));

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
    fn get_jump_hint(&self, addr: RuntimeAddress, ic: usize) -> JumpHint {
        let push_jump_hint = self.push_jump_hint.get(&addr).expect("invalid address");
        let pc = self.get_current_bytecode().ic_pc_map.get(ic).expect("invalid ic");
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

        warn!(addr=?self.get_current_address(), jmp_pc=jmp_pc, "incorrent call hint");
        false
    }

    #[inline]
    fn validate_return(&self, dest_pc: usize) -> bool {
        // TODO (ZZ): improve a recursive validation.
        let mut cur_id = self.cur_node.expect("validate_return without entering");
        while let Some((parent_id, edge)) = self.call_trace.nodes[cur_id].parent {
            if edge.is_message_call() {
                break;
            }

            if self.call_trace.nodes[parent_id]
                .trace
                .last()
                .expect("validate_return without trace")
                .next_block() ==
                dest_pc
            {
                return true;
            }

            cur_id = parent_id;
        }

        false
    }

    #[inline]
    fn refine_call_trace_by_return(&mut self, dest_pc: usize) {
        let cur_id = self.cur_node.expect("refine_call_trace_by_return without entering");
        let (parent_id, _) = self.call_trace.nodes[cur_id].parent.unwrap();
        if self.call_trace.nodes[parent_id]
            .trace
            .last()
            .expect("validate_return without trace")
            .next_block() !=
            dest_pc
        {
            // TODO (ZZ): refine the call trace.
            warn!(addr=?self.get_current_address(), dest_pc=dest_pc, "refine call trace by return");
            unimplemented!("refine call trace by return");
        }
    }
}

impl<'a, DB> Inspector<DB> for CallTraceInspector<'a, DB>
where
    DB: Database,
    DB::Error: std::error::Error,
{
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
            let dest = cur_bytecode.pc_ic_map.get(dest_pc).expect("invalid pc");
            debug!(addr=?cur_addr, ic=ic, op=?OpCode::new(op), cur_block_start=self.cur_block_start, dest=dest, "JUMP");

            let cur_node = &mut self.call_trace.nodes
                [self.cur_node.expect("get_current_node_mut without entering")];

            cur_node.trace.push(BlockNode::new(self.cur_block_start, self.cur_ic));

            if self.get_jump_hint(cur_addr, ic) == JumpHint::Call && self.validate_call(interp, pc)
            {
                self.enter(cur_addr, dest, Edge::IntraContract(true));
            } else if self.validate_return(dest_pc) {
                self.refine_call_trace_by_return(dest);
                self.exit(None, Some(dest));
            } else {
                self.cur_block_start = dest;
            }
        } else if op == JUMPI {
            let dest = cur_bytecode
                .pc_ic_map
                .get(interp.stack().peek(0).expect("call without dest").to())
                .expect("invalid pc");
            let cond = interp.stack().peek(1).expect("call without cond");
            trace!(addr=?cur_addr, ic=ic, op=?OpCode::new(op), cur_block_start=self.cur_block_start, dest=dest, cond=?cond, "JUMPI");

            let cur_node = &mut self.call_trace.nodes
                [self.cur_node.expect("get_current_node_mut without entering")];
            cur_node.trace.push(BlockNode::new(self.cur_block_start, self.cur_ic));

            self.cur_block_start = if cond.is_zero() {
                // The jump is not taken.
                self.cur_ic + 1
            } else {
                // The jump is taken.
                dest
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
        // XXX (ZZ): implement this after EOF is merged.
        unimplemented!("EOF create has not been merged into the mainnet");
    }

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
