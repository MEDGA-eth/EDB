mod calibration;
mod inspector;

use std::{collections::BTreeMap, fmt::Display};

pub use inspector::CallTraceInspector;
use revm::interpreter::InstructionResult;
use revm_inspectors::tracing::types::CallKind;

use crate::{
    analysis::source_map::{debug_unit::DebugUnit, source_label::SourceLabel},
    RuntimeAddress,
};

#[derive(Default, Debug)]
pub struct AnalyzedCallTrace {
    /// Whether the call trace is calibrated (with source code).
    /// - If not calibrated, the call trace may contain incorrect caller-callee relationships, but
    ///   the call trace strictly follows the rule of a child node must return to its parent node
    ///   (especially  for tail calls).
    /// - If calibrated, the call trace is refined by the source map, and the caller-callee
    ///   relationships are largely corrected. However, given the possibility of tail calls, the
    ///   call trace may not strictly follow the rule of a child node must return to its parent
    ///   node.
    calibrated: bool,

    /// The nodes in the call trace.
    nodes: Vec<FuncNode>,
}

impl AnalyzedCallTrace {
    pub fn apply_lazy_updates(&mut self) {
        if self.nodes.is_empty() {
            return;
        }

        debug_assert!(!self.nodes[0].is_discarded() && self.nodes[0].is_root());
        self.assign_depth(0, Depth::default(), false);
        self.assign_child_indices(0, 0);
    }

    pub fn assign_depth(&mut self, node_id: usize, depth: Depth, force: bool) {
        let node = &mut self.nodes[node_id];

        // If the depth is already assigned, then we do not need to assign it again.
        if node.depth.is_some() && !force {
            return;
        }

        node.depth = Some(depth);

        for (child_id, callsite) in node.children.clone().into_iter() {
            let new_depth = Depth::new_from_parent(&depth, callsite.edge);
            self.assign_depth(child_id, new_depth, force);
        }
    }

    pub fn assign_child_indices(&mut self, node_id: usize, child_index: usize) {
        // To avoid borrowing issues, we first collect the children ids.
        let children_ids =
            self.nodes[node_id].children.iter().map(|(id, _)| *id).collect::<Vec<_>>();

        // Update the child index of the node.
        let node = &mut self.nodes[node_id];
        node.child_index = child_index;

        // Update the child index of the children.
        for (child_index, child_id) in children_ids.into_iter().enumerate() {
            self.assign_child_indices(child_id, child_index);
        }

        #[cfg(debug_assertions)]
        if let Some((parent_id, _)) = self.nodes[node_id].parent {
            debug_assert!(
                self.nodes[parent_id].children[child_index].0 == node_id,
                "parent_id: {parent_id}, child_index: {child_index}, node_id: {node_id}",
            );
        }
    }

    pub fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(&FuncNode),
    {
        for node in self.nodes.iter() {
            if node.is_valid() {
                f(node);
            }
        }
    }
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
            Edge::IntraContract => {
                Self { message: parent.message, intra_contract: parent.intra_contract + 1 }
            }
        }
    }
}

#[derive(Default, Debug)]
pub struct FuncNode {
    /// Whether the node is discarded.
    pub discard: bool,

    /// Location in the whole graph.
    pub loc: usize,

    /// Parent node index in the graph (ic, callsite).
    pub parent: Option<(usize, Callsite)>,
    /// Location in the parent node (i.e., which child is this node).
    pub child_index: usize,

    /// Children node indexes in the graph (ic, callsite).
    pub children: Vec<(usize, Callsite)>,

    /// Call trace within the function.
    pub trace: Vec<BlockNode>,

    /// Return status of message call.
    pub ret: Option<InstructionResult>,

    /// The address of the code. Note that this is the address of the *code*, not necessarily the
    /// address of the storage.
    pub addr: RuntimeAddress,

    /// The fine-grained depth of the call. We post-calculate the depth of the call graph after the
    /// construction.
    pub depth: Option<Depth>,
}

impl FuncNode {
    pub fn is_discarded(&self) -> bool {
        self.discard
    }

    pub fn is_valid(&self) -> bool {
        !self.discard && self.depth.is_some()
    }

    pub fn is_root(&self) -> bool {
        self.parent.is_none()
    }

    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    pub fn find_step(&self, step: usize) -> Option<&BlockNode> {
        self.trace.iter().find(|block| block.contains_step(step))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CalibrationPoint {
    /// The calibration point is a statement or an inline assembly block.
    Singleton(SourceLabel),
    /// The calibration point is a range of statements or inline assembly blocks.
    Merged(Vec<SourceLabel>),
}

impl CalibrationPoint {
    pub fn is_singleton(&self) -> bool {
        matches!(self, Self::Singleton(_))
    }

    pub fn is_merged(&self) -> bool {
        matches!(self, Self::Merged(_))
    }

    pub fn as_singleton(&self) -> Option<&SourceLabel> {
        match self {
            Self::Singleton(label) => Some(label),
            _ => None,
        }
    }

    pub fn as_merged(&self) -> Option<&Vec<SourceLabel>> {
        match self {
            Self::Merged(labels) => Some(labels),
            _ => None,
        }
    }
}

#[derive(Default, Debug, PartialEq, Eq, Clone)]
pub struct BlockNode {
    /// The address of the code. Note that this is the address of the *code*, not necessarily the
    /// address of the storage.
    pub addr: RuntimeAddress,

    /// The first step (over the entire execution) of the block.
    pub start_step: usize,

    /// The first instruction count (over the contract) of the block.
    pub start_ic: usize,

    /// The number of instructions in the block.
    pub inst_n: usize,

    /// If the block ends with a call, then the node index of the callee.
    pub call_to: Option<usize>,

    /// Calibration points (towards the source map).
    pub calib: BTreeMap<usize, CalibrationPoint>,

    /// Calibrated Function
    pub calib_func: Option<DebugUnit>,
    /// Calibrated Modifiers
    pub calib_modifiers: Vec<DebugUnit>,
}

impl Display for BlockNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}..{}] ({}..{})",
            self.start_ic,
            self.start_ic + self.inst_n - 1,
            self.start_step,
            self.start_step + self.inst_n - 1
        )?;
        if let Some(call_to) = self.call_to {
            write!(f, " -call-> {call_to}")
        } else {
            Ok(())
        }
    }
}

impl BlockNode {
    pub fn new(addr: RuntimeAddress, start_ic: usize, end_ic: usize, end_step: usize) -> Self {
        Self {
            addr,
            start_step: end_step - (end_ic - start_ic),
            start_ic,
            inst_n: end_ic - start_ic + 1,
            ..Default::default()
        }
    }

    pub fn next_block_ic(&self) -> usize {
        self.start_ic + self.inst_n
    }

    pub fn next_to(&self, other: &Self) -> bool {
        self.addr == other.addr && self.next_block_ic() == other.start_ic
    }

    pub fn contains_step(&self, step: usize) -> bool {
        self.start_step <= step && step < self.start_step + self.inst_n
    }

    pub fn contains_ic(&self, ic: usize) -> bool {
        self.start_ic <= ic && ic < self.start_ic + self.inst_n
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Callsite {
    pub ic: usize,
    pub edge: Edge,
}

impl Callsite {
    pub fn new(ic: usize, edge: Edge) -> Self {
        Self { ic, edge }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Edge {
    /// The edge is a message call.
    MessageCall(CallKind),
    /// The edge is an intra-contract call.
    IntraContract,
}

impl Edge {
    pub fn is_message_call(&self) -> bool {
        matches!(self, Self::MessageCall(_))
    }

    pub fn is_intra_contract(&self) -> bool {
        matches!(self, Self::IntraContract)
    }
}
