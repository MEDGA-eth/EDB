use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt::{Debug, Display},
    iter,
    ops::{Deref, DerefMut},
};

use alloy_primitives::{Address, U256};
use eyre::{bail, OptionExt, Result};
use revm::{
    interpreter::{
        opcode::{DUP1, DUP16, JUMP, JUMPDEST, JUMPI, POP, PUSH0, PUSH32, SWAP1, SWAP16},
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, EOFCreateInputs, Interpreter, OpCode,
    },
    Database, EvmContext, Inspector,
};

#[cfg(feature = "paralize_analysis")]
use rayon::prelude::*;

use crate::{
    analysis::source_map::{
        debug_unit::DebugUnit, integrity::IntergrityLevel, source_label::SourceLabel,
        RefinedSourceMap,
    },
    artifact::onchain::AnalyzedBytecode,
    utils::opcode::get_push_value,
    RuntimeAddress,
};

use super::AssertionUnwrap;

/// A jump instruction can have three hints (including JUMPI):
///  - `Block`: this instruction is a block jump.
///  - `Call`: this instruction is a call jump.
///  - `Ret`: this instruction is a return jump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpHint {
    /// This instruction is a block jump.
    Block,

    /// This instruction is a call jump.
    Call,

    /// This instruction is a return jump.
    Return,

    /// This instruction type is unknown.
    Unknown,
}

impl PartialOrd for JumpHint {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Unknown, Self::Unknown) => Some(std::cmp::Ordering::Equal),
            (Self::Unknown, _) => Some(std::cmp::Ordering::Less),
            (_, Self::Unknown) => Some(std::cmp::Ordering::Greater),
            _ if self == other => Some(std::cmp::Ordering::Equal),
            _ => None,
        }
    }
}

impl Display for JumpHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Block => write!(f, "JUMP::Block"),
            Self::Call => write!(f, "JUMP::Call"),
            Self::Return => write!(f, "JUMP::Return"),
            Self::Unknown => write!(f, "JUMP::Unknown"),
        }
    }
}

/// A pushed item can have three hints:
///  - `CalleeAddr`: this item has been explictly used as a callee address during execution.
///  - `RetAddr`: this item has been explictly used a return address during execution.
///  - `BlockAddr`: this item has been explictly used by intra-procedural control flow.
///  - `NumericVal`: this item has been explictly used as a numeric value during data manipulation.
///  - `Unknown(bool)`: this item has not been used during execution. The boolean value indicates
///    whether this item is a jump destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushHint {
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

impl PartialOrd for PushHint {
    /// The partial order of the push label. We construct it as a lattice:
    ///  - Bottom: Unknown
    ///  - The rest of the hints are incomparable.
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Unknown, Self::Unknown) => Some(std::cmp::Ordering::Equal),
            (Self::Unknown, _) => Some(std::cmp::Ordering::Less),
            (_, Self::Unknown) => Some(std::cmp::Ordering::Greater),
            _ if self == other => Some(std::cmp::Ordering::Equal),
            _ => None,
        }
    }
}

impl Display for PushHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CalleeAddr => write!(f, "PUSH::CalleeAddr"),
            Self::ReturnAddr => write!(f, "PUSH::ReturnAddr"),
            Self::BlockAddr => write!(f, "PUSH::BlockAddr"),
            Self::NumericVal => write!(f, "PUSH::NumericVal"),
            Self::Unknown => write!(f, "PUSH::Unknown"),
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

#[derive(Debug)]
struct HintWrapper<'a> {
    bytecode: &'a AnalyzedBytecode,
    hint: PJHint,
}

impl Deref for HintWrapper<'_> {
    type Target = PJHint;

    fn deref(&self) -> &Self::Target {
        &self.hint
    }
}

impl DerefMut for HintWrapper<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.hint
    }
}

/// Hints for potential Push and Jump hints. Note that the hints are not 100% accurate.
#[derive(Debug, Default)]
pub struct PJHint {
    /// The address of the subject contract.
    pub addr: RuntimeAddress,

    /// The pushed values
    pub pushed_values: BTreeMap<usize, U256>,

    /// The jumpped targets: jump_pc -> [pc, ...]
    pub jump_targets: BTreeMap<usize, BTreeSet<U256>>,

    /// The jump-tagged mapping: jump_pc -> [push_pc, ...].
    /// This mapping is the jump instruction and those push instructions that are tagged with this
    /// jump.
    pub jump_tags: BTreeMap<usize, BTreeSet<usize>>,

    /// The jump-push mapping: jump_pc -> [push_pc, ...].
    /// This mapping is the jump instruction and those push instructions that are used by this
    /// jump.
    pub jump_pushes: BTreeMap<usize, BTreeSet<usize>>,

    /// The push hints: push_pc -> label
    pub push_hints: BTreeMap<usize, PushHint>,

    /// The jump hints: jmp_pc -> hint
    pub jump_hints: BTreeMap<usize, JumpHint>,
}

impl<'a> HintWrapper<'a> {
    fn unwrap(self) -> PJHint {
        self.hint
    }

    fn new(bytecode: &'a AnalyzedBytecode) -> Self {
        Self { bytecode, hint: PJHint::default() }
    }

    fn heuristic_analysis(&mut self) -> Result<()> {
        let mut callee_addrs = BTreeSet::new();
        let mut return_addrs = BTreeSet::new();

        let bytecode = self.bytecode;

        // Heuristic: if a jump instruction can jump to multiple targets, we will treat it as a
        // return jump.
        let jump_targets = &self.hint.jump_targets;
        let jump_hints = &mut self.hint.jump_hints;
        for (pc, targets) in jump_targets {
            if targets.len() > 1 {
                jump_hints.ordered_insert(*pc, JumpHint::Return);
            }
        }

        // The following propogation rules are based on the following observations:
        //  - A jump instruction labelled as call will always jump to a callee address.
        //  - A jump instruction labelled as return will always jump to a return address.
        //
        // However, they are not always true. We will refine them using `strict_check_call` and
        // `strict_check_return` functions.
        let mut worklist =
            self.jump_hints.iter().map(|(&pc, &label)| (pc, label)).collect::<VecDeque<_>>();

        while let Some((pc, label)) = worklist.pop_front() {
            // update the jump hints
            self.jump_hints.ordered_insert(pc, label);

            match label {
                JumpHint::Call => {
                    // Rule 1: a jump instruction labelled as call will always jump to a callee
                    // address.
                    for callee_addr in
                        self.jump_targets.get(&pc).ok_or_eyre("jump target not found (call)")?
                    {
                        trace!(pc=pc, callee_addr=?callee_addr, callee_addrs=?self.jump_targets[&pc], "callee address during worklist iteration");
                        let callee_addr = callee_addr.to::<usize>();
                        if callee_addrs.insert(callee_addr) {
                            worklist.extend(self.find_new_callee_addr(callee_addr)?.iter());
                        }
                    }

                    // Rule 2: the address right after a call jump is a return jump. Note that we
                    // can directly cacluate the next pc since JUMP is a single byte instruction.
                    if let Some(next_pc) = bytecode.next_insn_pc(pc) {
                        debug_assert!(next_pc == pc + 1, "invalid jump opcode");
                        if bytecode.get_opcode_at_pc(next_pc).map_or(false, |op| op.is_jumpdest()) &&
                            return_addrs.insert(next_pc)
                        {
                            worklist.extend(self.find_new_return_addr(next_pc)?.iter());
                        }
                    }

                    // Rule 3: any push instruction used by a call jump is to push a callee
                    // address.
                    let jump_pushes = &self.hint.jump_pushes;
                    let push_hints = &mut self.hint.push_hints;
                    if let Some(pushes) = jump_pushes.get(&pc) {
                        // We do not further propogate the callee address information. It is because
                        // there is not indirect call in Solidity, so in
                        // most cases, we already handle the callee address.
                        for push_pc in pushes {
                            push_hints.ordered_insert(*push_pc, PushHint::CalleeAddr);
                        }
                    }
                }
                JumpHint::Return => {
                    // Rule 4: a jump instruction labelled as return will always jump to a
                    // return address.
                    for return_addr in
                        self.jump_targets.get(&pc).ok_or_eyre("jump target not found (return)")?
                    {
                        let return_addr = return_addr.to::<usize>();
                        if return_addrs.insert(return_addr) {
                            worklist.extend(self.find_new_return_addr(return_addr)?.iter());
                        }
                    }

                    // Rule 5: any push instruction used by a return jump is to push a return
                    // address.
                    let jump_pushes = &self.hint.jump_pushes;
                    let push_hints = &mut self.hint.push_hints;
                    if let Some(pushes) = jump_pushes.get(&pc) {
                        for push_pc in pushes {
                            push_hints.ordered_insert(*push_pc, PushHint::ReturnAddr);
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Strictly check whether a jump instruction is a call jump. Specifically, we will check
    /// whether its predecessor is a push instruction and the pushed value is the jump target.
    fn strict_check_call(&self, pc: usize) -> bool {
        let bytecode = self.bytecode;

        let Some(op) = bytecode.get_opcode_at_pc(pc) else {
            return false;
        };

        if !op.is_jump() {
            return false;
        }

        if let Some(pushes) = self.jump_pushes.get(&pc) {
            pushes.len() == 1 &&
                pushes.iter().all(|&push_pc| {
                    bytecode.pc_ic_map.get(push_pc).map(|ic| ic + 1) == bytecode.pc_ic_map.get(pc)
                })
        } else {
            false
        }
    }

    /// Strictly check whether a jump instruction is a return jump. Specifically, we will check
    /// whether its predecessor is a non-push stack manipulation instruction.
    fn strict_check_return(&self, pc: usize, bytecode: &AnalyzedBytecode) -> bool {
        let Some(op) = bytecode.get_opcode_at_pc(pc) else {
            return false;
        };

        let Some(ic) = bytecode.pc_ic_map.get(pc) else {
            return false;
        };

        if ic == 0 {
            return false;
        }

        if !op.is_jump() {
            return false;
        }

        let Some(prev_op) = bytecode.get_opcode_at_ic(ic - 1) else {
            return false;
        };

        matches!(prev_op.get(), DUP1..=DUP16 | SWAP1..=SWAP16 | POP..=POP)
    }

    fn find_new_callee_addr(&self, callee_addr: usize) -> Result<Vec<(usize, JumpHint)>> {
        let r_addr = self.addr;
        trace!(addr=?r_addr, callee_addr, "find new callee addr");

        let mut new_hints = Vec::new();

        // Rule C1: jump to callee address is a call jump.
        for (pc, targets) in &self.jump_targets {
            if targets.contains(&U256::from(callee_addr)) {
                match self.jump_hints.get(pc) {
                    Some(&l) => {
                        // We will have strict constraint for propagation
                        if JumpHint::Call > l && self.strict_check_call(*pc) {
                            trace!(r_addr=?r_addr, pc=pc, targets=?targets, callee_addr, original_label=?self.jump_hints.get(pc), "new finding by Rule C1");
                            new_hints.push((*pc, JumpHint::Call));
                        }
                    }
                    None => {
                        debug_assert!(false, "invalid jump label");
                    }
                }
            }
        }

        Ok(new_hints)
    }

    fn find_new_return_addr(&self, return_addr: usize) -> Result<Vec<(usize, JumpHint)>> {
        let r_addr = self.addr;
        let bytecode = self.bytecode;

        trace!(r_addr=?r_addr, return_addr, "find new return addr");

        let mut new_hints = Vec::new();

        // Rule R1: jump to return address is a return jump.
        for (pc, targets) in &self.jump_targets {
            if targets.contains(&U256::from(return_addr)) {
                match self.jump_hints.get(pc) {
                    Some(&l) => {
                        // We will have strict constraint for propagation.
                        if JumpHint::Return > l && self.strict_check_return(*pc, bytecode) {
                            trace!(r_addr=?r_addr, pc=pc, targets=?targets, return_addr, original_label=?self.jump_hints.get(pc), "new finding by Rule R1");
                            new_hints.push((*pc, JumpHint::Return));
                        }
                    }
                    None => {
                        debug_assert!(false, "invalid jump label");
                    }
                }
            }
        }

        // Rule R2: the address right before a return address is a call jump.
        let call_pc = bytecode.prev_insn_pc(return_addr).ok_or_eyre("invalid pc")?;
        if call_pc + 1 == return_addr &&
            self.strict_check_call(call_pc) &&
            self.jump_hints.get(&call_pc) == Some(&JumpHint::Unknown)
        {
            trace!(r_addr=?r_addr, pc=call_pc, return_addr=return_addr, original_label=?self.jump_hints.get(&call_pc), "new finding by Rule R2");
            new_hints.push((call_pc, JumpHint::Call));
        }

        Ok(new_hints)
    }

    fn refine_by_source_map(&mut self, source_map: &RefinedSourceMap) -> Result<()> {
        if source_map.intergrity_level == IntergrityLevel::Corrupted {
            // If the source map is corrupted, we will skip the refinement.
            return Ok(());
        }

        let r_addr = self.addr;

        let jump_hints = &mut self.hint.jump_hints;
        let jump_targets = &mut self.hint.jump_targets;
        let bytecode = &self.bytecode;

        for (pc, label) in jump_hints.iter_mut() {
            let Some(opcode) = bytecode.get_opcode_at_pc(*pc) else {
                bail!("invalid pc");
            };

            if opcode.get() == JUMPI {
                debug_assert!(*label == JumpHint::Block, "invalid jump label");
                continue;
            }

            let targets = jump_targets
                .get(pc)
                .expect("jump targets not found")
                .iter()
                .map(|t| t.to::<usize>())
                .collect::<Vec<_>>();

            let ic = self.bytecode.pc_ic_map.get(*pc).expect("invalid pc");
            trace!(r_addr=?r_addr, pc=pc, ic=ic, opcode=opcode.as_str(), label=?label, "try to refine jump label");

            let pre_src_label = source_map.labels.get(ic - 1).ok_or_eyre(format!(
                "invalid ic: {}@{}",
                ic - 1,
                r_addr
            ))?;
            let cur_src_label =
                source_map.labels.get(ic).ok_or_eyre(format!("invalid ic: {ic}@{r_addr}"))?;
            let next_src_label = source_map.labels.get(ic + 1); // next pc may not exist

            if pre_src_label == cur_src_label && Some(cur_src_label) == next_src_label {
                if let SourceLabel::PrimitiveStmt { func: func_1, .. } = cur_src_label {
                    if targets.iter().all(|t_pc| {
                        let Some(t_ic) = self.bytecode.pc_ic_map.get(*t_pc) else {
                            return false;
                        };

                        let Some(t_label) = source_map.labels.get(t_ic) else {
                            return false;
                        };

                        match t_label {
                            SourceLabel::PrimitiveStmt { func: func_2, .. } => func_1 != func_2,
                            SourceLabel::Tag { tag: func_2 }
                                if matches!(func_2, DebugUnit::Function(..)) =>
                            {
                                func_1 != func_2
                            }
                            _ => false,
                        }
                    }) {
                        // Three contiguous instructions (including the jump instruction) are in
                        // the same statement, and the jump targets are all in different
                        // functions. We will refine the jump label to `Call`.
                        debug_assert!(
                            JumpHint::Call >= *label,
                            "failed to refine jump label: r_addr={r_addr:?}, pc={pc}, label={label:?}, targets={targets:?}"
                        );
                        *label = JumpHint::Call;
                    }
                }
            }
        }

        Ok(())
    }
}

/// The inspector that performs the push-jump analysis.
/// The push-jump analysis is a dynmaic analysis that aims to determine the label of each push
/// instruction and jump instruction.
///
/// Note that, while the analysis is not 100% accurate, we shall try to make the label of call as
/// accurate as possible.
#[derive(Debug)]
pub struct PushJumpInspector<'a> {
    /// The message call stack:
    stack: Vec<CallFrame>,

    /// The Push-Jump information.
    hint: BTreeMap<RuntimeAddress, HintWrapper<'a>>,
}

impl<'a> PushJumpInspector<'a> {
    /// Extract the push-jump information.
    pub fn extract(self) -> BTreeMap<RuntimeAddress, PJHint> {
        self.hint.into_iter().map(|(addr, info)| (addr, info.unwrap())).collect()
    }

    /// Create a new push-jump inspector.
    pub fn new(bytecodes: &'a BTreeMap<RuntimeAddress, AnalyzedBytecode>) -> Self {
        Self {
            stack: Vec::new(),
            hint: bytecodes
                .iter()
                .map(|(addr, bytecode)| (*addr, HintWrapper::new(bytecode)))
                .collect(),
        }
    }

    /// perform the post-analysis after the execution.
    /// We mainly leverage heuristics to determine the label of each push and jump instruction.
    pub fn posterior_analysis(&mut self) -> Result<()> {
        #[cfg(feature = "paralize_analysis")]
        let iter = self.hint.par_iter_mut();
        #[cfg(not(feature = "paralize_analysis"))]
        let mut iter = self.hint.iter_mut();

        iter.try_for_each(|(_, r_hint)| {
            r_hint.heuristic_analysis()?;
            Ok(())
        })
    }

    /// Refine the analysis result using the source map. In this function, we mainly focus on
    /// inferring more call jumps.
    pub fn refine_analysis_by_source_map(
        &mut self,
        source_map: &BTreeMap<RuntimeAddress, RefinedSourceMap>,
    ) -> Result<()> {
        #[cfg(feature = "paralize_analysis")]
        let iter = self.hint.par_iter_mut();
        #[cfg(not(feature = "paralize_analysis"))]
        let mut iter = self.hint.iter_mut();

        iter.try_for_each(|(r_addr, r_hint)| {
            let Some(source_map) = source_map.get(r_addr) else {
                // This contract does not have a source map (i.e., not verified). We will skip the
                // refinement.
                return Ok(());
            };

            r_hint.refine_by_source_map(source_map)
        })
    }

    #[cfg(debug_assertions)]
    pub fn log_unknown_hints(&self) {
        for (addr, info) in &self.hint {
            for (pc, label) in &info.push_hints {
                if *label == PushHint::Unknown {
                    trace!(addr=?addr, pc=pc, label=?label, "unknown push label");
                }
            }

            for (pc, label) in &info.jump_hints {
                if *label == JumpHint::Unknown {
                    trace!(addr=?addr, pc=pc, label=?label, targets=?info.jump_targets[pc], "unknown jump label");
                }
            }
        }
    }
}

impl<'a, DB> Inspector<DB> for PushJumpInspector<'a>
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
        let r_hint = self.hint.get_mut(&r_addr).expect("info not found");
        trace!(r_addr=?r_addr, pc=pc, op=op, "step (PushJumpInspector)");

        match op {
            PUSH0..=PUSH32 => {
                let code = interp.bytecode.as_ref();
                let value = get_push_value(code, pc).assert_unwrap("invalid bytecode");

                r_hint.pushed_values.equal_insert(pc, value);

                // Check whether the pushed value is larger than the code size
                if value >= U256::from(code.len()) || code[value.to::<usize>()] != JUMPDEST {
                    // The pushed value is a not jump destination. As a result, it must be a
                    // numerical value.
                    r_hint.push_hints.ordered_insert(pc, PushHint::NumericVal);

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
                    r_hint.push_hints.or_insert(pt.push_pc, PushHint::Unknown);
                }
            }
            JUMP..=JUMP => {
                let jump_target =
                    interp.stack().data().last().cloned().assert_unwrap("empty evm stack");
                r_hint.jump_targets.entry(pc).or_default().insert(jump_target);

                // The collection of all push instructions that are going to be tagged with
                // this jump instruction.
                let jump_tags = r_hint.jump_tags.entry(pc).or_default();

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

                    r_hint.jump_pushes.entry(pc).or_default().insert(pt.push_pc);

                    let Some((pjmp_pc, pjmp_step)) = pt.next_jump else {
                        debug_assert!(false, "next_jump is not set");
                        return;
                    };

                    if pjmp_step != frame.step && pjmp_pc + 1 == pt.value {
                        // The push item is not tagged with the same jump instruction, but its
                        // pushed value is the next instruction of its corresponding jump
                        // instruction.
                        //
                        // THIS IS A STRONG INDICATION THAT THE PUSHED VALUE IS A RETURN ADDRESS.
                        r_hint.jump_hints.ordered_insert(pc, JumpHint::Return);
                        r_hint.push_hints.ordered_insert(pt.push_pc, PushHint::ReturnAddr);

                        // We will also tag the corresponding jump instruction.
                        r_hint.jump_hints.ordered_insert(pjmp_pc, JumpHint::Call);
                        let jump_pushes = &mut r_hint.hint.jump_pushes;
                        let push_hints = &mut r_hint.hint.push_hints;
                        for push_pc in jump_pushes.entry(pjmp_pc).or_default().iter() {
                            push_hints.ordered_insert(*push_pc, PushHint::CalleeAddr);
                        }
                    } else if untagged_n == 0 {
                        // Heuristic: if the jump instruction is not tagged with any push
                        // instruction, we will treat it as a return jump.
                        r_hint.jump_hints.ordered_insert(pc, JumpHint::Return);
                        r_hint.push_hints.ordered_insert(pt.push_pc, PushHint::ReturnAddr);
                    } else {
                        trace!(addr=?r_addr, pc=pc, untagged_n=untagged_n, pt=?pt, "we cannot determine the label of the jump instruction");
                        r_hint.jump_hints.or_insert(pc, JumpHint::Unknown);
                    }

                    // note: the following heurisitc may be usefule, but many test cases have shown
                    // that they are buggy.
                    //
                    // ```
                    // if untagged_n == 1 && pjmp_step == frame.step {
                    //     // The pushed item is used by its corresponding jump instruction. Meanwhile,
                    //     // it is the only untagged push item.
                    //     self.jump_hints.ordered_insert(r_addr, pc, JumpLabel::Block);
                    //     self.push_hints.ordered_insert(r_addr, pt.push_pc, PushLabel::BlockAddr);
                    // }
                    // ```
                } else {
                    trace!(addr=?r_addr, pc=pc, "we cannot determine the jump target value");
                    r_hint.jump_hints.or_insert(pc, JumpHint::Unknown);
                }
            }
            JUMPI..=JUMPI => {
                // the jumpi instruction is a block jump.
                r_hint.jump_hints.ordered_insert(pc, JumpHint::Block);

                if let Some(pt) = frame.pop() {
                    stack_top_check(interp, pt.value);
                    r_hint.push_hints.ordered_insert(pt.push_pc, PushHint::BlockAddr);
                }

                if let Some(pt) = frame.pop() {
                    r_hint.push_hints.ordered_insert(pt.push_pc, PushHint::NumericVal);
                }
            }
            _ => {
                let opcode = OpCode::new(op).assert_unwrap("invalid opcode");

                for _ in 0..opcode.inputs() {
                    // All poped items will be treated as numeric values.
                    if let Some(pt) = frame.pop() {
                        r_hint.push_hints.ordered_insert(pt.push_pc, PushHint::NumericVal);
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
        trace!(addr=?addr, depth=?self.stack.len(), "call to contract address");

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
        trace!("call end");

        let frame = self.stack.pop().assert_unwrap("stack is empty (call)");
        debug_assert!(!frame.is_constructor, "constructor should not return here");

        // We will mark all pushed items as unknown if they are not analyzed so far.
        let r_addr = frame.runtime_address();
        for pt in frame.stack.iter().flatten() {
            self.hint
                .get_mut(&r_addr)
                .expect("this should not happen")
                .push_hints
                .or_insert(pt.push_pc, PushHint::Unknown);
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
            debug_assert!(false, "load caller account error during contract creation: {err}");
        }

        let nonce = context.journaled_state.account(inputs.caller).info.nonce;
        let addr = inputs.created_address(nonce);

        trace!(depth=?self.stack.len(), addr=?addr, "create contract");

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
        trace!("create end");

        let frame = self.stack.pop().assert_unwrap("stack is empty (create)");
        debug_assert!(frame.is_constructor, "non-constructor call should not return here");

        // We will mark all pushed items as unknown if they are not analyzed so far.
        let r_addr = frame.runtime_address();
        for pt in frame.stack.iter().flatten() {
            self.hint
                .get_mut(&r_addr)
                .expect("this should not happen")
                .push_hints
                .or_insert(pt.push_pc, PushHint::Unknown);
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
    fn ordered_insert(&mut self, pc: usize, value: T);
    fn equal_insert(&mut self, pc: usize, value: T);
    fn or_insert(&mut self, pc: usize, value: T);
}

impl<T> GuardedLabelMap<T> for BTreeMap<usize, T>
where
    T: PartialOrd + Copy + Display + Debug,
{
    fn ordered_insert(&mut self, pc: usize, value: T) {
        trace!(pc=pc, value=?value, "ordered insert");
        if let Some(old_value) = self.insert(pc, value) {
            let ord = old_value.partial_cmp(&value);
            debug_assert!(
                ord == Some(std::cmp::Ordering::Less) || ord == Some(std::cmp::Ordering::Equal),
                "decending order is not allowed ({old_value} -> {value})"
            );
        }
    }

    fn equal_insert(&mut self, pc: usize, value: T) {
        trace!(pc=pc, value=?value, "equal insert");
        if let Some(old_value) = self.insert(pc, value) {
            debug_assert!(old_value == value, "different value is not allowed");
        }
    }

    fn or_insert(&mut self, pc: usize, value: T) {
        trace!(pc=pc, value=?value, "or insert");
        self.entry(pc).or_insert(value);
    }
}
