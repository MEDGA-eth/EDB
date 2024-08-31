//! Inspector to construct the dynamic call graph.

// XXX (ZZ): Self-Check for returns!

use std::collections::BTreeMap;

use alloy_primitives::Address;
use revm::{Database, Inspector};
use revm_inspectors::tracing::types::CallKind;
use serde::de;

use crate::{AnalyzedBytecode, RuntimeAddress};

use super::push_jmp::{JumpLabel, PushJmpInfo};

#[derive(Default, Debug)]
pub struct CallTrace {
    nodes: Vec<FuncNode>,
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

    // Function information.
    /// The address of the code. Note that this is the address of the *code*, not necessarily the
    /// address of the storage.
    pub code_address: RuntimeAddress,
    /// The address of the storage.
    pub storage_address: Address,

    /// The depth of the message call. Note that this is not about the call made by JUMP/JUMPI.
    pub msg_depth: usize,
    /// The call depth of the function (within a single contract call).
    pub func_depth: usize,
}

#[derive(Default, Debug)]
pub struct BlockNode {
    pub start_ic: usize,
    pub inst_n: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum Edge {
    /// The edge is a message call.
    MessageCall(CallKind),
    /// The edge is an intra-contract call.
    IntraContract,
}

#[derive(Debug)]
pub struct CallTraceInspector<'a, DB> {
    push_jump_info: &'a BTreeMap<RuntimeAddress, PushJmpInfo>,
    bytecodes: &'a BTreeMap<RuntimeAddress, AnalyzedBytecode>,

    // The call graph to be constructed.
    call_graph: CallTrace,

    cur_node: Option<usize>,
    phantom: std::marker::PhantomData<DB>
}

impl<'a, DB> CallTraceInspector<'a, DB> 
where
    DB: Database {
    pub fn new(
        push_jump_info: &'a BTreeMap<RuntimeAddress, PushJmpInfo>,
        bytecodes: &'a BTreeMap<RuntimeAddress, AnalyzedBytecode>,
    ) -> Self {
        Self {
            push_jump_info,
            bytecodes,
            call_graph: CallTrace::default(),
            cur_node: None,
            phantom: Default::default(),
        }
    }

    fn enter(&mut self, edge: Edge, code_address: RuntimeAddress, storage_address: Address, msg_depth: usize, func_depth: usize) {
        let loc = self.call_graph.nodes.len();
        let parent = self.cur_node.map(|parent| (parent, edge));
        let child_loc = parent.map(|(parent, _)| self.call_graph.nodes[parent].children.len()).unwrap_or(0);

        self.cur_node = Some(loc);
        self.call_graph.nodes.push(FuncNode {
            loc,
            parent,
            children: Vec::new(),
            child_loc,
            trace: Vec::new(),
            code_address,
            storage_address,
            msg_depth,
            func_depth,
        });
    }
}

impl<'a, DB> Inspector<DB> for CallTraceInspector<'a, DB>
where
    DB: Database,
    DB::Error: std::error::Error,
    {}