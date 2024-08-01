use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Debug, Display},
    iter,
};

use alloy_primitives::{Address, U256};
use eyre::{bail, Result};
use revm::{
    interpreter::{
        opcode::{DUP1, DUP16, JUMP, JUMPDEST, JUMPI, POP, PUSH0, PUSH32, SWAP1, SWAP16},
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, EOFCreateInputs, Interpreter, OpCode,
    },
    Database, EvmContext, Inspector,
};

use crate::{utils::opcode::get_push_value, RuntimeAddress};

use super::AssertionUnwrap;

/// A jump instruction can have three labels (including JUMPI):
///  - `Block`: this instruction is a block jump.
///  - `Call`: this instruction is a call jump.
///  - `Ret`: this instruction is a return jump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpLabel {
    /// This instruction is a block jump.
    Block,

    /// This instruction is a call jump.
    Call,

    /// This instruction is a return jump.
    Return,

    /// This instruction type is unknown.
    Unknown,
}

impl PartialOrd for JumpLabel {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (JumpLabel::Unknown, JumpLabel::Unknown) => Some(std::cmp::Ordering::Equal),
            (JumpLabel::Unknown, _) => Some(std::cmp::Ordering::Less),
            (_, JumpLabel::Unknown) => Some(std::cmp::Ordering::Greater),
            _ if self == other => Some(std::cmp::Ordering::Equal),
            _ => None,
        }
    }
}

impl Display for JumpLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JumpLabel::Block => write!(f, "JUMP::Block"),
            JumpLabel::Call => write!(f, "JUMP::Call"),
            JumpLabel::Return => write!(f, "JUMP::Return"),
            JumpLabel::Unknown => write!(f, "JUMP::Unknown"),
        }
    }
}

/// A pushed item can have three labels:
///  - `CalleeAddr`: this item has been explictly used as a callee address during execution.
///  - `RetAddr`: this item has been explictly used a return address during execution.
///  - `BlockAddr`: this item has been explictly used by intra-procedural control flow.
///  - `NumericVal`: this item has been explictly used as a numeric value during data manipulation.
///  - `Unknown(bool)`: this item has not been used during execution. The boolean value indicates
///    whether this item is a jump destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushLabel {
    /// The item has been used as a callee address.
    CalleeAddr,

    /// The item has been used as a return address.
    ReturnAddr,

    /// The item has been used as a block address.
    BlockAddr,

    /// The item has been used as a numeric value.
    NumericVal,

    /// The item has not been used during execution (but its value is a pc of a JUMPDEST opcode).
    Unknown,
}

impl PartialOrd for PushLabel {
    /// The partial order of the push label. We construct it as a lattice:
    ///  - Bottom: Unknown
    ///  - The rest of the labels are incomparable.
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (PushLabel::Unknown, PushLabel::Unknown) => Some(std::cmp::Ordering::Equal),
            (PushLabel::Unknown, _) => Some(std::cmp::Ordering::Less),
            (_, PushLabel::Unknown) => Some(std::cmp::Ordering::Greater),
            _ if self == other => Some(std::cmp::Ordering::Equal),
            _ => None,
        }
    }
}

impl Display for PushLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PushLabel::CalleeAddr => write!(f, "PUSH::CalleeAddr"),
            PushLabel::ReturnAddr => write!(f, "PUSH::ReturnAddr"),
            PushLabel::BlockAddr => write!(f, "PUSH::BlockAddr"),
            PushLabel::NumericVal => write!(f, "PUSH::NumericVal"),
            PushLabel::Unknown => write!(f, "PUSH::Unknown"),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PushedItem {
    value: usize,
    push_pc: usize,

    // The first jump instruction since this item is pushed. JUMPI is not included.
    // Being `tagged` in this context means that the jump instruction is the first jump
    // instruction since this item is pushed.
    // To make it path sensitive, we need to consider (pc, step) pair.
    next_jump: Option<(usize, usize)>,
}

impl PushedItem {
    fn new(value: U256, push_pc: usize) -> Self {
        Self { value: value.to::<usize>(), push_pc, next_jump: None }
    }
}

type InnerStack = Vec<Option<PushedItem>>;

#[derive(Debug, Clone, Default)]
struct CallFrame {
    // The stack of the call.
    stack: InnerStack,

    // The current opcode step number.
    step: usize,

    // The code address of the call.
    address: Address,

    // Whether this call is a constructor.
    is_constructor: bool,

    // The push instructions that are not tagged with `next_jump``.
    untagged_pushes: BTreeSet<usize>,
}

impl CallFrame {
    fn new(address: Address, is_constructor: bool) -> Self {
        Self {
            stack: Vec::new(),
            step: 0,
            address,
            is_constructor,
            untagged_pushes: BTreeSet::new(),
        }
    }

    fn runtime_address(&self) -> RuntimeAddress {
        RuntimeAddress::new(self.address, self.is_constructor)
    }

    fn push(&mut self, item: Option<PushedItem>) {
        if let Some(PushedItem { next_jump: None, .. }) = item {
            self.untagged_pushes.insert(self.stack.len());
        }
        self.stack.push(item);
    }

    fn pop(&mut self) -> Option<PushedItem> {
        let item = self.stack.pop().assert_unwrap("stack is empty (pop)");
        self.untagged_pushes.remove(&self.stack.len());
        item
    }
}

#[derive(Debug, Default)]
pub struct PushJumpInspector {
    /// The message call stack:
    stack: Vec<CallFrame>,

    /// The pushed values
    pub pushed_values: BTreeMap<RuntimeAddress, BTreeMap<usize, U256>>,

    /// The jumpped targets: runtime_addr -> jump_pc -> [pc, ...]
    pub jump_targets: BTreeMap<RuntimeAddress, BTreeMap<usize, BTreeSet<U256>>>,

    /// The jump-tagged mapping: runtime_addr -> jump_pc -> [push_pc, ...].
    /// This mapping is the jump instruction and those push instructions that are tagged with this
    /// jump.
    pub jump_tags: BTreeMap<RuntimeAddress, BTreeMap<usize, BTreeSet<usize>>>,

    /// The jump-push mapping: runtime_addr -> jump_pc -> [push_pc, ...].
    /// This mapping is the jump instruction and those push instructions that are used by this
    /// jump.
    pub jump_pushes: BTreeMap<RuntimeAddress, BTreeMap<usize, BTreeSet<usize>>>,

    /// The push labels: runtime_addr -> push_pc -> label
    pub push_labels: BTreeMap<RuntimeAddress, BTreeMap<usize, PushLabel>>,

    /// The jump labels: runtime_addr -> jmp_pc -> label
    pub jump_labels: BTreeMap<RuntimeAddress, BTreeMap<usize, JumpLabel>>,
}

impl PushJumpInspector {
    pub fn posterior_analysis(&mut self) {
        // Rule 0.a: a jump instruction labelled as call will always jump to a callee address.
        // Rule 0.b: any push instruction used by a call jump is a callee-related push, whose values
        //           are callee addresses.

        // Rule 1.a: jump to callee address is a call jump.
        // Rule 1.b: pushed values used by a call jump are all callee addresses.
        // Rule 1.c: push instructions used by a call jump are all callee addresses.

        // Rule 2.a: jump to an address next to a call jump is a return jump.
        // Rule 2.b: pushed values used by a return jump are all return addresses.
        // Rule 2.c: push instructions used by a return jump are all return addresses.
    }
}

impl<'a, DB> Inspector<DB> for PushJumpInspector
where
    DB: Database,
    DB::Error: std::error::Error,
{
    /// Called after `step` when the instruction has been executed.
    ///
    /// Setting `interp.instruction_result` to anything other than
    /// [crate::interpreter::InstructionResult::Continue] alters the execution
    /// of the interpreter.
    #[inline]
    fn step_end(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        // We perform a naive stack consistency check here.
        if let Some(frame) = self.stack.last_mut() {
            if let Some(Some(PushedItem { value, .. })) = frame.stack.last() {
                stack_top_check(interp, *value);
            }
            frame.step += 1;
        }
    }

    /// Called on each step of the interpreter.
    ///
    /// Information about the current execution, including the memory, stack and more is available
    /// on `interp` (see [Interpreter]).
    ///
    /// # Example
    ///
    /// To get the current opcode, use `interp.current_opcode()`.
    #[inline]
    fn step(&mut self, interp: &mut Interpreter, _context: &mut EvmContext<DB>) {
        let pc = interp.program_counter();
        let op = interp.current_opcode();
        let Some(frame) = self.stack.last_mut() else {
            debug_assert!(false, "stack is empty (step)");
            return;
        };

        let r_addr = frame.runtime_address();

        match op {
            PUSH0..=PUSH32 => {
                let code = interp.bytecode.as_ref();
                let value = get_push_value(code, pc).assert_unwrap("invalid bytecode");

                self.pushed_values.equal_insert(r_addr, pc, value);

                // Check whether the pushed value is larger than the code size
                if value >= U256::from(code.len()) || code[value.to::<usize>()] != JUMPDEST {
                    // The pushed value is a not jump destination. As a result, it must be a
                    // numerical value.
                    self.push_labels.ordered_insert(r_addr, pc, PushLabel::NumericVal);

                    frame.push(None);
                } else {
                    // In this case, we need to do a naive taint analysis to determine the label of
                    // the pushed value.
                    frame.push(Some(PushedItem::new(value, pc)));
                }
            }
            DUP1..=DUP16 => {
                let idx = frame.stack.len() - 1 - (op - DUP1) as usize;
                let pt =
                    frame.stack.get(idx).cloned().assert_unwrap("the dup operation is invalid");

                frame.push(pt);
            }
            SWAP1..=SWAP16 => {
                let a_idx = frame.stack.len() - 1;
                let b_idx = a_idx - 1 - (op - SWAP1) as usize;
                frame.stack.swap(a_idx, b_idx);

                let a_untagged = frame.untagged_pushes.contains(&a_idx);
                let b_untagged = frame.untagged_pushes.contains(&b_idx);

                if a_untagged && !b_untagged {
                    frame.untagged_pushes.remove(&a_idx);
                    frame.untagged_pushes.insert(b_idx);
                } else if !a_untagged && b_untagged {
                    frame.untagged_pushes.insert(a_idx);
                    frame.untagged_pushes.remove(&b_idx);
                }
            }
            POP..=POP => {
                if let Some(pt) = frame.pop() {
                    // Consistency check.
                    stack_top_check(interp, pt.value);
                    self.push_labels.or_insert(r_addr, pt.push_pc, PushLabel::Unknown);
                }
            }
            JUMP..=JUMP => {
                let jump_target =
                    interp.stack().data().last().cloned().assert_unwrap("empty evm stack");
                self.jump_targets
                    .entry(r_addr)
                    .or_default()
                    .entry(pc)
                    .or_default()
                    .insert(jump_target);

                // The collection of all push instructions that are going to be tagged with
                // this jump instruction.
                let jump_tags = self.jump_tags.entry(r_addr).or_default().entry(pc).or_default();

                // Update `next_jump` for all untagged push items.
                let untagged_n = frame.untagged_pushes.len();
                while let Some(idx) = frame.untagged_pushes.pop_last() {
                    if let Some(Some(pt)) = frame.stack.get_mut(idx) {
                        jump_tags.insert(pt.push_pc);
                        pt.next_jump = Some((pc, frame.step));
                    } else {
                        debug_assert!(false, "invalid index");
                    }
                }

                if let Some(pt) = frame.pop() {
                    stack_top_check(interp, pt.value);

                    self.jump_pushes
                        .entry(r_addr)
                        .or_default()
                        .entry(pc)
                        .or_default()
                        .insert(pt.push_pc);

                    let Some((pjmp_pc, pjmp_step)) = pt.next_jump else {
                        debug_assert!(false, "next_jump is not set");
                        return;
                    };

                    if pjmp_step != frame.step && pjmp_pc + 1 == pt.value {
                        // The push item is not tagged with the same jump instruction, but its
                        // pushed value is the next instruction of its
                        // corresponding jump instruction.
                        self.jump_labels.ordered_insert(r_addr, pc, JumpLabel::Return);
                        self.push_labels.ordered_insert(r_addr, pt.push_pc, PushLabel::ReturnAddr);

                        // We will also tag the corresponding jump instruction.
                        self.jump_labels.ordered_insert(r_addr, pjmp_pc, JumpLabel::Call);
                        for push_pc in self
                            .jump_pushes
                            .entry(r_addr)
                            .or_default()
                            .entry(pjmp_pc)
                            .or_default()
                            .iter()
                        {
                            self.push_labels.ordered_insert(
                                r_addr,
                                *push_pc,
                                PushLabel::CalleeAddr,
                            );
                        }
                    } else if untagged_n == 1 && pjmp_step == frame.step {
                        // The pushed item is used by its corresponding jump instruction. Meanwhile,
                        // it is the only untagged push item.
                        self.jump_labels.ordered_insert(r_addr, pc, JumpLabel::Block);
                        self.push_labels.ordered_insert(r_addr, pt.push_pc, PushLabel::BlockAddr);
                    } else if untagged_n == 0 {
                        // Heuristic: if the jump instruction is not tagged with any push
                        // instruction, we will treat it as a return jump.
                        self.jump_labels.ordered_insert(r_addr, pc, JumpLabel::Return);
                        self.push_labels.ordered_insert(r_addr, pt.push_pc, PushLabel::ReturnAddr);
                    } else {
                        trace!(addr=?r_addr, pc=pc, untagged_n=untagged_n, pt=?pt, "we cannot determine the label of the jump instruction");
                        self.jump_labels.or_insert(r_addr, pc, JumpLabel::Unknown);
                    }
                } else {
                    // Heuristic: if the jump instruction does not use any recorded stack-pushed
                    // values, i.e., the target address is not directly pushed onto stack but
                    // calculated from other operations, we will treat it as a
                    // call jump. FIXME (ZZ): fix it later
                    trace!(addr=?r_addr, pc=pc, "we cannot determine the jump target value");
                    self.jump_labels.or_insert(r_addr, pc, JumpLabel::Unknown);
                }
            }
            JUMPI..=JUMPI => {
                // The jumpi instruction is a block jump.
                self.jump_labels.ordered_insert(r_addr, pc, JumpLabel::Block);

                if let Some(pt) = frame.pop() {
                    stack_top_check(interp, pt.value);
                    self.push_labels.ordered_insert(r_addr, pt.push_pc, PushLabel::BlockAddr);
                }

                if let Some(pt) = frame.pop() {
                    self.push_labels.ordered_insert(r_addr, pt.push_pc, PushLabel::NumericVal);
                }
            }
            _ => {
                let opcode = OpCode::new(op).assert_unwrap("invalid opcode");

                for _ in 0..opcode.inputs() {
                    // All poped items will be treated as numeric values.
                    if let Some(pt) = frame.pop() {
                        self.push_labels.ordered_insert(r_addr, pt.push_pc, PushLabel::NumericVal);
                    }
                }

                frame.stack.extend(iter::repeat(None).take(opcode.outputs() as usize));
            }
        }
    }

    /// Called whenever a call to a contract is about to start.
    ///
    /// InstructionResulting anything other than [crate::interpreter::InstructionResult::Continue]
    /// overrides the result of the call.
    #[inline]
    fn call(
        &mut self,
        _context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        let addr = inputs.bytecode_address;
        debug!(addr=?addr, depth=?self.stack.len(), "call to contract address");

        self.stack.push(CallFrame::new(addr, false));

        None
    }

    /// Called when a call to a contract has concluded.
    ///
    /// The returned [CallOutcome] is used as the result of the call.
    ///
    /// This allows the inspector to modify the given `result` before returning it.
    #[inline]
    fn call_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        debug!("call end");

        let frame = self.stack.pop().assert_unwrap("stack is empty (call)");
        debug_assert!(!frame.is_constructor, "constructor should not return here");

        // We will mark all pushed items as unknown if they are not analyzed so far.
        let r_addr = frame.runtime_address();
        for pt in frame.stack.iter().flatten() {
            self.push_labels.or_insert(r_addr, pt.push_pc, PushLabel::Unknown);
        }

        outcome
    }

    /// Called when a contract is about to be created.
    ///
    /// If this returns `Some` then the [CreateOutcome] is used to override the result of the
    /// creation.
    ///
    /// If this returns `None` then the creation proceeds as normal.
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
            debug_assert!(false, "load caller account error during contract creation: {}", err);
        }

        let nonce = context.journaled_state.account(inputs.caller).info.nonce;
        let addr = inputs.created_address(nonce);

        debug!(depth=?self.stack.len(), addr=?addr, "create contract");

        self.stack.push(CallFrame::new(addr, true));

        None
    }

    /// Called when a contract has been created.
    ///
    /// InstructionResulting anything other than the values passed to this function (`(ret,
    /// remaining_gas, address, out)`) will alter the result of the create.
    #[inline]
    fn create_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &CreateInputs,
        outcome: CreateOutcome,
    ) -> CreateOutcome {
        debug!("create end");

        let frame = self.stack.pop().assert_unwrap("stack is empty (create)");
        debug_assert!(frame.is_constructor, "non-constructor call should not return here");

        // We will mark all pushed items as unknown if they are not analyzed so far.
        let r_addr = frame.runtime_address();
        for pt in frame.stack.iter().flatten() {
            self.push_labels.or_insert(r_addr, pt.push_pc, PushLabel::Unknown);
        }

        outcome
    }

    /// Called when EOF creating is called.
    ///
    /// This can happen from create TX or from EOFCREATE opcode.
    fn eofcreate(
        &mut self,
        _context: &mut EvmContext<DB>,
        _inputs: &mut EOFCreateInputs,
    ) -> Option<CreateOutcome> {
        // XXX (ZZ): implement this after EOF is merged.
        unimplemented!("EOF create has not been merged into the mainnet");
    }

    /// Called when eof creating has ended.
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

#[inline]
fn stack_top_check(interp: &Interpreter, value: usize) {
    debug_assert!(
        &U256::from(value) == interp.stack().data().last().expect("empty evm stack"),
        "poped value is not consistent with the recored stack"
    );
}

trait GuardedLabelMap<T> {
    fn ordered_insert(&mut self, addr: RuntimeAddress, pc: usize, value: T);
    fn equal_insert(&mut self, addr: RuntimeAddress, pc: usize, value: T);
    fn or_insert(&mut self, addr: RuntimeAddress, pc: usize, value: T);
}

impl<T> GuardedLabelMap<T> for BTreeMap<RuntimeAddress, BTreeMap<usize, T>>
where
    T: PartialOrd + Copy + Display + Debug,
{
    fn ordered_insert(&mut self, addr: RuntimeAddress, pc: usize, value: T) {
        trace!(addr=?addr, pc=pc, value=?value, "ordered insert");
        if let Some(old_value) = self.entry(addr).or_default().insert(pc, value) {
            let ord = old_value.partial_cmp(&value);
            debug_assert!(
                ord == Some(std::cmp::Ordering::Less) || ord == Some(std::cmp::Ordering::Equal),
                "decending order is not allowed ({} -> {})",
                old_value,
                value
            );
        }
    }

    fn equal_insert(&mut self, addr: RuntimeAddress, pc: usize, value: T) {
        trace!(addr=?addr, pc=pc, value=?value, "equal insert");
        if let Some(old_value) = self.entry(addr).or_default().insert(pc, value) {
            debug_assert!(old_value == value, "different value is not allowed");
        }
    }

    fn or_insert(&mut self, addr: RuntimeAddress, pc: usize, value: T) {
        trace!(addr=?addr, pc=pc, value=?value, "or insert");
        self.entry(addr).or_default().entry(pc).or_insert(value);
    }
}
