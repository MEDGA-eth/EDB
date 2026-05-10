// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Opcode snapshot inspector for recording detailed VM state at each instruction
//!
//! This inspector captures instruction-level execution details including:
//! - Current instruction offset (PC)
//! - Contract address
//! - Current opcode
//! - Memory state (with Arc sharing for unchanged memory)
//! - Stack state (using persistent data for efficient clone)
//! - Call data (with Arc sharing across the same trace entry)
//!
//! Memory optimization: Uses Arc to share memory and calldata when unchanged,
//! reducing memory usage for large execution traces.

use alloy_primitives::{Address, Bytes, U256};
use edb_common::{
    EdbContext, OpcodeTr, edb_debug_assert, edb_debug_assert_eq,
    types::{ExecutionFrameId, Trace},
};
use revm::{
    Database, DatabaseCommit, DatabaseRef, Inspector,
    bytecode::opcode::OpCode,
    context::{ContextTr, LocalContextTr},
    database::CacheDB,
    interpreter::{
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter,
        interpreter_types::{InputsTr, Jumps},
    },
    state::TransientStorage,
};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Borrow,
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
    sync::Arc,
};
use tracing::{debug, error};

use crate::Stack;

/// Single opcode execution snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpcodeSnapshot<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Program counter (instruction offset)
    pub pc: usize,
    /// Target address that triggered the hook
    pub target_address: Address,
    /// Bytecode address that the current snapshot is running
    pub bytecode_address: Address,
    /// Current opcode
    pub opcode: u8,
    /// Memory state (shared via Arc when unchanged)
    pub memory: Arc<Vec<u8>>,
    /// Stack state (persistent stack)
    pub stack: Stack,
    /// Call data for this execution context (shared via Arc within same context)
    pub calldata: Arc<Bytes>,
    /// Database state (shared via Arc within same context)
    pub database: Arc<CacheDB<DB>>,
    /// Transient storage
    #[serde(with = "edb_common::types::arc_transient_string_map")]
    pub transient_storage: Arc<TransientStorage>,
    /// Block environment at the moment of capture. Mutates across the tx when
    /// cheatcodes like vm.warp/vm.roll fire.
    #[serde(default)]
    pub block_env: revm::context::BlockEnv,
    /// Cfg environment at the moment of capture. Mutates when vm.chainId fires.
    #[serde(default)]
    pub cfg_env: revm::context::CfgEnv,
}

impl<DB> Default for OpcodeSnapshot<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Default,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn default() -> Self {
        Self {
            pc: 0,
            target_address: Address::default(),
            bytecode_address: Address::default(),
            opcode: 0,
            memory: Arc::new(Vec::new()),
            stack: Stack::default(),
            calldata: Arc::new(Bytes::default()),
            database: Arc::new(CacheDB::default()),
            transient_storage: Arc::new(TransientStorage::default()),
            block_env: revm::context::BlockEnv::default(),
            cfg_env: revm::context::CfgEnv::default(),
        }
    }
}

/// Collection of opcode snapshots
#[derive(Debug, Clone)]
pub struct OpcodeSnapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    inner: HashMap<ExecutionFrameId, Vec<OpcodeSnapshot<DB>>>,
}

impl<DB> Default for OpcodeSnapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn default() -> Self {
        Self { inner: HashMap::new() }
    }
}

impl<DB> Deref for OpcodeSnapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    type Target = HashMap<ExecutionFrameId, Vec<OpcodeSnapshot<DB>>>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<DB> DerefMut for OpcodeSnapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Frame state tracking for memory optimization
#[derive(Debug, Clone)]
struct FrameState {
    /// Last captured memory state
    last_memory: Arc<Vec<u8>>,
}

impl FrameState {
    fn from_interp(interp: &Interpreter) -> Self {
        Self { last_memory: Arc::new(interp.memory.borrow().context_memory().to_vec()) }
    }

    fn update_with_interp(&mut self, interp: &Interpreter) {
        self.last_memory = Arc::new(interp.memory.borrow().context_memory().to_vec());
    }
}

/// Trace state tracking for stack
#[derive(Debug, Clone, Default)]
struct TraceState {
    /// Current persistent stack
    stack: Stack,
    /// Current calldata
    last_calldata: Arc<Bytes>,
}

impl TraceState {
    fn from_evm<DB>(interp: &Interpreter, ctx: &EdbContext<DB>) -> Self
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let calldata = match interp.input.input() {
            revm::interpreter::CallInput::SharedBuffer(range) => Arc::new(
                ctx.local()
                    .shared_memory_buffer_slice(range.clone())
                    .map(|slice| Bytes::from(slice.to_vec()))
                    .unwrap_or_else(Bytes::new),
            ),
            revm::interpreter::CallInput::Bytes(bytes) => Arc::new(bytes.clone()),
        };
        Self { last_calldata: calldata, stack: Stack::default() }
    }
}

/// Inspector that records detailed opcode execution snapshots
#[derive(Debug)]
pub struct OpcodeSnapshotInspector<'a, DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// The trace of the current tx
    trace: &'a Trace,

    /// Map from execution frame ID to list of snapshots
    pub snapshots: OpcodeSnapshots<DB>,

    /// Set of addresses to exclude from recording (verified source code)
    pub excluded_addresses: HashSet<Address>,

    /// Stack to track current execution frames
    frame_stack: Vec<ExecutionFrameId>,

    /// Current trace entry counter (to match with call tracer)
    current_trace_id: usize,

    /// Frame state for each active frame (for memory optimization)
    frame_states: HashMap<ExecutionFrameId, FrameState>,

    /// Trace state
    trace_state: HashMap<usize, TraceState>,

    /// Database context
    database: Arc<CacheDB<DB>>,

    /// Transient storage
    transient_storage: Arc<TransientStorage>,

    /// Last opcode
    last_opcode: Option<OpCode>,
}

impl<'a, DB> OpcodeSnapshotInspector<'a, DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Create a new opcode snapshot inspector
    pub fn new(ctx: &EdbContext<DB>, trace: &'a Trace) -> Self {
        Self {
            trace,
            snapshots: OpcodeSnapshots::<DB>::default(),
            excluded_addresses: HashSet::new(),
            frame_stack: Vec::new(),
            current_trace_id: 0,
            frame_states: HashMap::new(),
            trace_state: HashMap::new(),
            database: Arc::new(ctx.db().clone()),
            transient_storage: Arc::new(TransientStorage::default()),
            last_opcode: None,
        }
    }

    /// Create inspector with excluded addresses
    pub fn with_excluded_addresses(&mut self, excluded_addresses: HashSet<Address>) {
        self.excluded_addresses = excluded_addresses;
    }

    /// Consume the inspector and return the collected snapshots
    pub fn into_snapshots(self) -> OpcodeSnapshots<DB> {
        self.snapshots
    }

    /// Add an address to exclude from recording
    pub fn exclude_address(&mut self, address: Address) {
        self.excluded_addresses.insert(address);
    }

    /// Get the current execution frame ID
    fn current_frame_id(&self) -> Option<ExecutionFrameId> {
        self.frame_stack.last().copied()
    }

    /// Check if we should record steps for the given address
    fn should_record(&self, address: Address) -> bool {
        !self.excluded_addresses.contains(&address)
    }

    /// Update the persistent stack based on the interpreter's current stack
    fn update_stack(&mut self, interp: &Interpreter, ctx: &mut EdbContext<DB>) {
        let Some(opcode) = self.last_opcode else { return };
        let opcode_info = opcode.info();

        let Some(frame_id) = self.frame_stack.last() else { return };
        let trace_state = self
            .trace_state
            .entry(frame_id.trace_entry_id())
            .or_insert(TraceState::from_evm(interp, ctx));

        if opcode_info.inputs() == 0 && opcode_info.outputs() == 0 {
            // No stack change
            return;
        }

        let mut new_stack = trace_state.stack.clone();
        for _ in 0..opcode_info.inputs() {
            let (_popped, stack_after_pop) =
                new_stack.pop().unwrap_or_else(|| panic!("Stack underflow ({opcode})"));
            new_stack = stack_after_pop;
        }
        if !opcode.is_message_call() {
            // For call opcodes, the stack will only be updated after the call returns
            for i in (0..opcode_info.outputs()).rev() {
                let value = interp
                    .stack
                    .peek(i as usize)
                    .unwrap_or_else(|e| panic!("Stack underflow ({opcode}): {e:?}"));
                new_stack = new_stack.push(value);
            }
        }

        edb_debug_assert_eq!(
            new_stack.len(),
            interp.stack.len(),
            "Stack length mismatch after executing {opcode} (in {}, out {}): expected {}, got {}",
            opcode_info.inputs(),
            opcode_info.outputs(),
            interp.stack.len(),
            new_stack.len()
        );

        trace_state.stack = new_stack;
    }

    /// Update memory
    fn update_memory(&mut self, interp: &Interpreter) {
        let Some(frame_id) = self.frame_stack.last() else { return };

        self.frame_states
            .entry(*frame_id)
            .and_modify(|state| {
                if self.last_opcode.map(|c| c.modifies_memory()).unwrap_or_default() {
                    state.update_with_interp(interp);
                }
            })
            .or_insert(FrameState::from_interp(interp));
    }

    /// Update storage
    fn update_storage(&mut self, ctx: &mut EdbContext<DB>, force_update: bool) {
        // When the last opcode is_call, we do not update storage, since the
        // storage will be updated in call()/create()/call_end()/create_end()
        if force_update
            || self
                .last_opcode
                .map(|c| c.modifies_evm_state() && !c.is_message_call())
                .unwrap_or(force_update)
        {
            let mut inner = ctx.journal().to_inner();
            let changes = inner.finalize();
            let mut snap = ctx.db().clone();
            snap.commit(changes);
            self.database = Arc::new(snap);
        }

        if force_update
            || self
                .last_opcode
                .map(|c| c.modifies_transient_storage() && !c.is_message_call())
                .unwrap_or(force_update)
        {
            let transient_storage = ctx.journal().transient_storage.clone();
            self.transient_storage = Arc::new(transient_storage);
        }
    }

    /// Record a snapshot at the current step
    fn record_snapshot(&mut self, interp: &Interpreter, ctx: &mut EdbContext<DB>) {
        debug!("Recording snapshot at PC {}", interp.bytecode.pc());

        // Get current opcode safely
        let opcode = unsafe { OpCode::new_unchecked(interp.bytecode.opcode()) };

        // Update last opcode
        self.last_opcode = Some(opcode);

        // Get current frame
        let Some(frame_id) = self.current_frame_id() else {
            return;
        };

        // Check if we should record for this address
        let contract_address =
            interp.input.bytecode_address().cloned().unwrap_or(interp.input.target_address());
        if !self.should_record(contract_address) {
            return;
        }

        let address = interp.input.target_address();

        // Get or create frame state
        let frame_state =
            self.frame_states.entry(frame_id).or_insert(FrameState::from_interp(interp));
        let memory = frame_state.last_memory.clone();
        edb_debug_assert!(
            check_memory_consistency(memory.deref(), &interp.memory.borrow().context_memory()),
            "inconsistent memory content"
        );

        // Get or create trace state
        let trace_state = self
            .trace_state
            .entry(frame_id.trace_entry_id())
            .or_insert(TraceState::from_evm(interp, ctx));
        let calldata = trace_state.last_calldata.clone();

        // Create snapshot (stack is always cloned as it changes frequently)
        let entry = self.trace.get(frame_id.trace_entry_id());
        let snapshot = OpcodeSnapshot {
            pc: interp.bytecode.pc(),
            bytecode_address: entry.map(|t| t.code_address).unwrap_or(address),
            target_address: entry.map(|t| t.target).unwrap_or(address),
            opcode: opcode.get(),
            memory,
            stack: self
                .trace_state
                .get(&frame_id.trace_entry_id())
                .map(|s| s.stack.clone())
                .unwrap_or_default(),
            calldata,
            database: self.database.clone(),
            transient_storage: self.transient_storage.clone(),
            block_env: ctx.block.clone(),
            cfg_env: ctx.cfg.clone(),
        };

        // Add to snapshots for this frame
        self.snapshots.entry(frame_id).or_default().push(snapshot);
    }

    /// Start tracking a new execution frame
    fn push_frame(&mut self, trace_id: usize) {
        let frame_id = ExecutionFrameId::new(trace_id, 0);
        self.frame_stack.push(frame_id);

        // Initialize empty snapshot list for this frame if not exists
        self.snapshots.entry(frame_id).or_default();
    }

    /// Stop tracking current execution frame and increment re-entry count
    fn pop_frame(&mut self) -> Option<ExecutionFrameId> {
        if let Some(frame_id) = self.frame_stack.pop() {
            // Clean up frame state
            self.frame_states.remove(&frame_id);

            // Increment re-entry count for parent frame if it exists
            if let Some(parent_frame_id) = self.frame_stack.last_mut() {
                parent_frame_id.increment_re_entry();
            }

            Some(frame_id)
        } else {
            None
        }
    }

    /// Get all recorded snapshots for a specific frame
    pub fn get_frame_snapshots(
        &self,
        frame_id: ExecutionFrameId,
    ) -> Option<&Vec<OpcodeSnapshot<DB>>> {
        self.snapshots.get(&frame_id)
    }

    /// Get all execution frame IDs that have recorded snapshots
    pub fn get_recorded_frames(&self) -> Vec<ExecutionFrameId> {
        self.snapshots.keys().copied().collect()
    }

    /// Clear all recorded data
    pub fn clear(&mut self) {
        self.snapshots.clear();
        self.frame_stack.clear();
        self.frame_states.clear();
        self.current_trace_id = 0;
    }
}

impl<'a, DB> Inspector<EdbContext<DB>> for OpcodeSnapshotInspector<'a, DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn step(&mut self, interp: &mut Interpreter, context: &mut EdbContext<DB>) {
        // Record snapshot BEFORE executing the opcode
        self.record_snapshot(interp, context);
    }

    fn step_end(&mut self, interp: &mut Interpreter, context: &mut EdbContext<DB>) {
        // Record snapshot AFTER executing the opcode
        self.update_storage(context, false);
        self.update_stack(interp, context);
        self.update_memory(interp);
    }

    fn call(
        &mut self,
        context: &mut EdbContext<DB>,
        _inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        // Start tracking new execution frame
        self.push_frame(self.current_trace_id);
        self.current_trace_id += 1;
        self.update_storage(context, true);
        None
    }

    fn call_end(
        &mut self,
        context: &mut EdbContext<DB>,
        _inputs: &CallInputs,
        outcome: &mut CallOutcome,
    ) {
        // Stop tracking current execution frame
        let Some(frame_id) = self.pop_frame() else { return };

        let Some(entry) = self.trace.get(frame_id.trace_entry_id()) else { return };

        edb_debug_assert_eq!(
            entry.result,
            Some(outcome.into()),
            "Call outcome mismatch in frame {frame_id:?}: expected {:?}, got {outcome:?}",
            entry.result
        );
        if entry.result != Some(outcome.into()) {
            // Mismatch in expected outcome, log error
            error!(
                "Call outcome mismatch in frame {frame_id:?}: expected {:?}, got {outcome:?}",
                entry.result
            );
        }

        // Update storage
        self.update_storage(context, true);

        // Update the stack after the call returns
        let Some(trace_state) = self
            .frame_stack
            .last()
            .and_then(|frame_id| self.trace_state.get_mut(&frame_id.trace_entry_id()))
        else {
            return;
        };

        let success = if outcome.result.is_ok() { U256::ONE } else { U256::ZERO };
        trace_state.stack = trace_state.stack.push(success);
    }

    fn create(
        &mut self,
        context: &mut EdbContext<DB>,
        _inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        // Start tracking new execution frame for contract creation
        self.push_frame(self.current_trace_id);
        self.current_trace_id += 1;
        self.update_storage(context, true);
        None
    }

    fn create_end(
        &mut self,
        context: &mut EdbContext<DB>,
        _inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        // Stop tracking current execution frame
        let Some(frame_id) = self.pop_frame() else { return };

        let Some(entry) = self.trace.get(frame_id.trace_entry_id()) else { return };

        edb_debug_assert_eq!(
            entry.result,
            Some(outcome.into()),
            "Create outcome mismatch in frame {frame_id:?}: expected {:?}, got {outcome:?}",
            entry.result
        );
        if entry.result != Some(outcome.into()) {
            // Mismatch in expected outcome, log error
            error!(
                "Create outcome mismatch in frame {frame_id:?}: expected {:?}, got {outcome:?}",
                entry.result
            );
        }

        // Update storage
        self.update_storage(context, true);

        // Update the stack after the create returns
        let Some(trace_state) = self
            .frame_stack
            .last()
            .and_then(|frame_id| self.trace_state.get_mut(&frame_id.trace_entry_id()))
        else {
            return;
        };

        let address_bytes = outcome.address.unwrap_or_default().into_word();
        trace_state.stack = trace_state.stack.push(U256::from_be_slice(address_bytes.as_slice()));
    }
}

/// Pretty printing utilities for debugging
impl<DB> OpcodeSnapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Print comprehensive summary with frame details
    pub fn print_summary(&self) {
        println!(
            "\n\x1b[36m╔══════════════════════════════════════════════════════════════════╗\x1b[0m"
        );
        println!(
            "\x1b[36m║              OPCODE SNAPSHOT INSPECTOR SUMMARY                   ║\x1b[0m"
        );
        println!(
            "\x1b[36m╚══════════════════════════════════════════════════════════════════╝\x1b[0m\n"
        );

        // Overall statistics
        let total_frames = self.len();
        let total_snapshots: usize = self.values().map(|v| v.len()).sum();

        println!("\x1b[33m📊 Overall Statistics:\x1b[0m");
        println!("  Total frames recorded: \x1b[32m{total_frames}\x1b[0m");
        println!("  Total snapshots recorded:  \x1b[32m{total_snapshots}\x1b[0m");

        if self.is_empty() {
            println!("\n\x1b[90m  No opcode snapshots were recorded.\x1b[0m");
            return;
        }

        // Calculate memory sharing statistics
        let mut total_memory_instances = 0;
        let mut unique_memory_instances = HashSet::new();
        let mut total_calldata_instances = 0;
        let mut unique_calldata_instances = HashSet::new();

        for snapshots in self.values() {
            for snapshot in snapshots {
                total_memory_instances += 1;
                unique_memory_instances.insert(Arc::as_ptr(&snapshot.memory) as usize);
                total_calldata_instances += 1;
                unique_calldata_instances.insert(Arc::as_ptr(&snapshot.calldata) as usize);
            }
        }

        let memory_sharing_ratio = if total_memory_instances > 0 {
            (total_memory_instances - unique_memory_instances.len()) as f64
                / total_memory_instances as f64
                * 100.0
        } else {
            0.0
        };

        let calldata_sharing_ratio = if total_calldata_instances > 0 {
            (total_calldata_instances - unique_calldata_instances.len()) as f64
                / total_calldata_instances as f64
                * 100.0
        } else {
            0.0
        };

        println!("\n\x1b[33m💾 Memory Optimization:\x1b[0m");
        println!(
            "  Memory - Unique instances: \x1b[32m{}\x1b[0m / Total refs: \x1b[32m{}\x1b[0m (Sharing: \x1b[32m{:.1}%\x1b[0m)",
            unique_memory_instances.len(),
            total_memory_instances,
            memory_sharing_ratio
        );
        println!(
            "  Calldata - Unique instances: \x1b[32m{}\x1b[0m / Total refs: \x1b[32m{}\x1b[0m (Sharing: \x1b[32m{:.1}%\x1b[0m)",
            unique_calldata_instances.len(),
            total_calldata_instances,
            calldata_sharing_ratio
        );

        println!("\n\x1b[33m📋 Frame Details:\x1b[0m");
        println!(
            "\x1b[90m─────────────────────────────────────────────────────────────────\x1b[0m"
        );

        // Sort frames for consistent output
        let mut sorted_frames: Vec<_> = self.iter().collect();
        sorted_frames
            .sort_by_key(|(frame_id, _)| (frame_id.trace_entry_id(), frame_id.re_entry_count()));

        for (frame_id, snapshots) in sorted_frames {
            // Frame header with color coding based on snapshot count
            let color = if snapshots.is_empty() {
                "\x1b[90m" // Gray for empty
            } else if snapshots.len() < 10 {
                "\x1b[32m" // Green for small
            } else if snapshots.len() < 100 {
                "\x1b[33m" // Yellow for medium
            } else {
                "\x1b[31m" // Red for large
            };

            println!(
                "\n  {}Frame {}\x1b[0m (trace.{}, re-entry {})",
                color,
                frame_id,
                frame_id.trace_entry_id(),
                frame_id.re_entry_count()
            );
            println!("  └─ Snapshots: \x1b[36m{}\x1b[0m", snapshots.len());

            if !snapshots.is_empty() {
                // Show first few and last few snapshots for context
                let preview_count = 3.min(snapshots.len());

                // First few snapshots
                println!("     \x1b[90mFirst {preview_count} snapshots:\x1b[0m");
                for (i, snapshot) in snapshots.iter().take(preview_count).enumerate() {
                    self.print_snapshot_line(i, snapshot, "     ");
                }

                // Last few snapshots if there are more
                if snapshots.len() > preview_count * 2 {
                    println!(
                        "     \x1b[90m... {} more snapshots ...\x1b[0m",
                        snapshots.len() - preview_count * 2
                    );
                    println!("     \x1b[90mLast {preview_count} snapshots:\x1b[0m");
                    let start_idx = snapshots.len() - preview_count;
                    for (i, snapshot) in snapshots.iter().skip(start_idx).enumerate() {
                        self.print_snapshot_line(start_idx + i, snapshot, "     ");
                    }
                } else if snapshots.len() > preview_count {
                    // Show remaining snapshots
                    for (i, snapshot) in snapshots.iter().skip(preview_count).enumerate() {
                        self.print_snapshot_line(preview_count + i, snapshot, "     ");
                    }
                }

                // Summary stats for this frame
                let total_memory: usize = snapshots.iter().map(|s| s.memory.len()).sum();
                let avg_stack_depth: f64 = snapshots.iter().map(|s| s.stack.len()).sum::<usize>()
                    as f64
                    / snapshots.len() as f64;

                println!("     \x1b[90m├─ Avg stack depth: {avg_stack_depth:.1}\x1b[0m");
                println!("     \x1b[90m└─ Total memory used: {total_memory} bytes\x1b[0m");
            }
        }

        println!(
            "\n\x1b[90m─────────────────────────────────────────────────────────────────\x1b[0m"
        );
    }

    /// Helper to print a single snapshot line
    fn print_snapshot_line(&self, index: usize, snapshot: &OpcodeSnapshot<DB>, indent: &str) {
        let opcode = unsafe { OpCode::new_unchecked(snapshot.opcode) };
        let opcode_str = opcode.as_str().to_string();

        #[allow(deprecated)]
        let addr_short = format!("{:?}", snapshot.bytecode_address);
        let addr_display = if addr_short.len() > 10 {
            format!("{}...{}", &addr_short[0..6], &addr_short[addr_short.len() - 4..])
        } else {
            addr_short
        };

        println!(
            "{}  [{:4}] PC={:5} \x1b[94m{:18}\x1b[0m @ \x1b[37m{}\x1b[0m | Stack:{:2} Mem:{:6}B",
            indent,
            index,
            snapshot.pc,
            opcode_str,
            addr_display,
            snapshot.stack.len(),
            snapshot.memory.len()
        );
    }
}

#[cfg(debug_assertions)]
fn check_memory_consistency(origin: &[u8], reference: &[u8]) -> bool {
    let (shorter, longer) =
        if origin.len() <= reference.len() { (origin, reference) } else { (reference, origin) };

    // Check common portion
    if shorter != &longer[..shorter.len()] {
        return false;
    }

    // Check if extra bytes in longer slice are all zeros
    if let Some((i, &byte)) = longer[shorter.len()..].iter().enumerate().find(|&(_, b)| *b != 0) {
        let absolute_idx = shorter.len() + i;
        debug!(
            "Memory length mismatch at byte {}: expected a padding 0, got {:02x}",
            absolute_idx, byte
        );
        return false;
    }

    true
}

#[cfg(test)]
mod env_capture_tests {
    use super::*;
    use revm::{
        context::{BlockEnv, CfgEnv},
        database::CacheDB,
        database_interface::EmptyDB,
    };

    type TestDB = CacheDB<EmptyDB>;

    #[test]
    fn opcode_snapshot_carries_block_env() {
        let s = OpcodeSnapshot::<TestDB>::default();
        let _: &BlockEnv = &s.block_env;
        let _: &CfgEnv = &s.cfg_env;
    }
}
