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

//! Hook snapshot inspector for recording VM state at specific trigger points
//!
//! This inspector captures database snapshots only when execution calls
//! the magic trigger address 0x0000000000000000000000000000000000023333.
//! The call data contains an ABI-encoded number (usid) that identifies
//! the specific hook point.
//!
//! Unlike OpcodeSnapshotInspector which captures every instruction,
//! HookSnapshotInspector only captures at predetermined breakpoints,
//! making it more efficient for tracking specific execution states.

use alloy_dyn_abi::{DynSolType, DynSolValue};
use alloy_primitives::{Address, B256, Bytes, U256, keccak256};
use edb_common::{
    EdbContext, OpcodeTr,
    types::{CallResult, EdbSolValue, ExecutionFrameId, Trace},
};
use eyre::Result;
use foundry_compilers::{Artifact, artifacts::Contract};
use revm::{
    Database, DatabaseCommit, DatabaseRef, Inspector,
    bytecode::OpCode,
    context::{ContextTr, CreateScheme, JournalTr},
    database::CacheDB,
    interpreter::{
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter,
        interpreter_types::{InputsTr, Jumps},
    },
    state::TransientStorage,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};
use tracing::{debug, error};

use crate::{
    USID,
    analysis::{AnalysisResult, UVID, UserDefinedTypeRef, VariableRef, dyn_sol_type},
    inspector::utils::relax_gas_limit_at_callsite,
};

/// Magic number that indicates a snapshot to be taken
pub const MAGIC_SNAPSHOT_NUMBER: U256 = U256::from_be_bytes([
    0x20, 0x15, 0x05, 0x02, 0xff, 0xff, 0xff, 0xff, 0x20, 0x24, 0x01, 0x02, 0xff, 0xff, 0xff, 0xff,
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
]);

/// Magic number that indicates a variable update to be recorded
pub const MAGIC_VARIABLE_UPDATE_NUMBER: U256 = U256::from_be_bytes([
    0x20, 0x25, 0x02, 0x08, 0xff, 0x20, 0x25, 0x09, 0x16, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
]);

/// Single hook execution snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSnapshot<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Target address that triggered the hook
    pub target_address: Address,
    /// Bytecode address that the current snapshot is running
    pub bytecode_address: Address,
    /// Database state at the hook point
    pub database: Arc<CacheDB<DB>>,
    /// Transient storage
    #[serde(with = "edb_common::types::arc_transient_string_map")]
    pub transient_storage: Arc<TransientStorage>,
    /// Value of accessible local variables
    pub locals: HashMap<String, Option<Arc<EdbSolValue>>>,
    /// Value of state variables at this point (e.g., code address)
    pub state_variables: HashMap<String, Option<Arc<EdbSolValue>>>,
    /// User-defined snapshot ID from call data
    pub usid: USID,
    /// Block environment at the moment of capture. Mutates across the tx when
    /// cheatcodes like vm.warp/vm.roll fire.
    #[serde(default)]
    pub block_env: revm::context::BlockEnv,
    /// Cfg environment at the moment of capture. Mutates when vm.chainId fires.
    #[serde(default)]
    pub cfg_env: revm::context::CfgEnv,
}

impl<DB> Default for HookSnapshot<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Default,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn default() -> Self {
        Self {
            target_address: Address::default(),
            bytecode_address: Address::default(),
            database: Arc::new(CacheDB::default()),
            transient_storage: Arc::new(TransientStorage::default()),
            locals: HashMap::new(),
            state_variables: HashMap::new(),
            usid: USID::default(),
            block_env: revm::context::BlockEnv::default(),
            cfg_env: revm::context::CfgEnv::default(),
        }
    }
}

/// Collection of hook snapshots organized by execution order
///
/// Unlike OpcodeSnapshots which use a HashMap, HookSnapshots maintains
/// insertion order to track when each snapshot was taken during execution.
#[derive(Debug, Clone)]
pub struct HookSnapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Vector of (frame_id, optional_snapshot) pairs in execution order
    /// None indicates a frame where no hook was triggered
    snapshots: Vec<(ExecutionFrameId, Option<HookSnapshot<DB>>)>,
}

impl<DB> Default for HookSnapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn default() -> Self {
        Self { snapshots: Vec::new() }
    }
}

impl<DB> Deref for HookSnapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    type Target = Vec<(ExecutionFrameId, Option<HookSnapshot<DB>>)>;

    fn deref(&self) -> &Self::Target {
        &self.snapshots
    }
}

impl<DB> DerefMut for HookSnapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.snapshots
    }
}

impl<DB> IntoIterator for HookSnapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    type Item = (ExecutionFrameId, Option<HookSnapshot<DB>>);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.snapshots.into_iter()
    }
}

impl<DB> HookSnapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Get snapshot for a specific frame ID
    pub fn get_snapshot(&self, frame_id: ExecutionFrameId) -> Option<&HookSnapshot<DB>> {
        self.snapshots
            .iter()
            .find(|(id, _)| *id == frame_id)
            .and_then(|(_, snapshot)| snapshot.as_ref())
    }

    /// Get all frames that have actual hook snapshots (non-None)
    pub fn get_frames_with_hooks(&self) -> Vec<ExecutionFrameId> {
        self.snapshots
            .iter()
            .filter_map(
                |(frame_id, snapshot)| {
                    if snapshot.is_some() { Some(*frame_id) } else { None }
                },
            )
            .collect()
    }

    /// Add a frame placeholder (will be None if no hook is triggered)
    fn add_frame_placeholder(&mut self, frame_id: ExecutionFrameId) {
        self.snapshots.push((frame_id, None));
    }

    /// Update the last frame with a hook snapshot
    fn update_last_frame_with_snapshot(
        &mut self,
        frame_id: ExecutionFrameId,
        snapshot: HookSnapshot<DB>,
    ) {
        if let Some((last_frame_id, slot)) = self.snapshots.last_mut() {
            if last_frame_id != &frame_id {
                error!("Mismatched frame IDs: expected {}, got {}", last_frame_id, frame_id);
            }
            if slot.is_none() {
                // If the last frame was empty, fill it with this snapshot
                *slot = Some(snapshot);
                return;
            }
        }

        self.snapshots.push((frame_id, Some(snapshot)));
    }
}

/// Inspector that records hook-triggered snapshots
#[derive(Debug)]
pub struct HookSnapshotInspector<'a, DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// The trace of the current tx
    trace: &'a Trace,

    /// Source code analysis results
    analysis: &'a HashMap<Address, AnalysisResult>,

    /// Codehash → canonical-address index, borrowed from `EngineContext`.
    /// Used to resolve addresses with no direct `analysis` entry by
    /// matching their live runtime bytecode against known artifacts.
    codehash_to_canonical: &'a HashMap<B256, Address>,

    /// Per-run cache mapping an alias address (one with no direct
    /// `analysis` entry) to its canonical address (the artifact's
    /// address whose bytecode it shares). The mapping is directional:
    /// **alias → canonical**, never the reverse. Populated lazily on
    /// first miss via codehash equality; subsequent lookups at the
    /// same alias are O(1). Cleared at the end of the run when the
    /// inspector is dropped.
    canonical_by_alias: HashMap<Address, Address>,

    /// Collection of hook snapshots
    pub snapshots: HookSnapshots<DB>,

    /// Stack to track current execution frames
    frame_stack: Vec<ExecutionFrameId>,

    /// Current trace entry counter
    current_trace_id: usize,

    /// Creation hooks (original contract bytecode, hooked bytecode, constructor args)
    creation_hooks: Vec<(Bytes, Bytes, Bytes)>,

    /// Predicted-address-keyed substitution map. When a CREATE is intercepted
    /// at a predicted address present in this map, the runtime init code is
    /// replaced with `hooked_creation_bytecode || extracted_args` — this is
    /// the path that makes nested CREATE hook firing work (where the parent
    /// contract embeds a copy of the child's creation code that may not
    /// byte-identically match the child's standalone artifact bytecode).
    ///
    /// Value tuple: `(hooked_creation_bytecode, args_size_bytes)`. The
    /// `args_size_bytes` is computed at registration time from the artifact's
    /// constructor ABI; the inspector reads that many trailing bytes from the
    /// runtime init_code as the constructor args to forward.
    creation_by_address: HashMap<Address, (Bytes, usize)>,

    /// The latest value of each UVID encountered (for variable tracking)
    uvid_values: HashMap<UVID, Arc<EdbSolValue>>,

    /// Last opcode
    last_opcode: Option<OpCode>,

    /// The current database
    database: Arc<CacheDB<DB>>,

    /// The current transient storage
    transient_storage: Arc<TransientStorage>,
}

impl<'a, DB> HookSnapshotInspector<'a, DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Create a new hook snapshot inspector
    pub fn new(
        ctx: &EdbContext<DB>,
        trace: &'a Trace,
        analysis: &'a HashMap<Address, AnalysisResult>,
        codehash_to_canonical: &'a HashMap<B256, Address>,
    ) -> Self {
        Self {
            trace,
            analysis,
            codehash_to_canonical,
            canonical_by_alias: HashMap::new(),
            snapshots: HookSnapshots::default(),
            frame_stack: Vec::new(),
            current_trace_id: 0,
            creation_hooks: Vec::new(),
            creation_by_address: HashMap::new(),
            uvid_values: HashMap::new(),
            last_opcode: None,
            database: Arc::new(ctx.db().clone()),
            transient_storage: Arc::new(TransientStorage::default()),
        }
    }

    /// Add creation hooks
    pub fn with_creation_hooks(
        &mut self,
        hooks: Vec<(&Contract, &Contract, &Bytes)>,
    ) -> Result<()> {
        debug!(target: "edb::hook::creation", incoming = hooks.len(), "registering creation hooks");
        for (original, hooked, args) in hooks {
            self.creation_hooks.push((
                original
                    .get_bytecode_bytes()
                    .ok_or(eyre::eyre!("Failed to get bytecode for contract"))?
                    .as_ref()
                    .clone(),
                hooked
                    .get_bytecode_bytes()
                    .ok_or(eyre::eyre!("Failed to get bytecode for contract"))?
                    .as_ref()
                    .clone(),
                args.clone(),
            ));
        }

        Ok(())
    }

    /// Register a predicted-address-keyed creation substitution map. Entries
    /// are keyed by the address that *would* be CREATEd (computed from caller
    /// nonce or CREATE2 salt at substitution time) and store the hooked
    /// creation bytecode to swap in. See `check_and_apply_creation_hooks` for
    /// why this exists alongside `with_creation_hooks`.
    pub fn with_creation_by_address(&mut self, map: HashMap<Address, (Bytes, usize)>) {
        debug!(
            target: "edb::hook::creation",
            entries = map.len(),
            "registering address-keyed creation hooks",
        );
        self.creation_by_address = map;
    }

    /// Consume the inspector and return the collected snapshots
    pub fn into_snapshots(self) -> HookSnapshots<DB> {
        self.snapshots
    }

    /// Get the current execution frame ID
    fn current_frame_id(&self) -> Option<ExecutionFrameId> {
        self.frame_stack.last().copied()
    }

    /// Start tracking a new execution frame
    fn push_frame(&mut self, trace_id: usize) {
        let frame_id = ExecutionFrameId::new(trace_id, 0);
        self.frame_stack.push(frame_id);

        // Add placeholder for this frame
        self.snapshots.add_frame_placeholder(frame_id);
    }

    /// Stop tracking current execution frame and increment re-entry count
    fn pop_frame(&mut self) -> Option<ExecutionFrameId> {
        if let Some(frame_id) = self.frame_stack.pop() {
            // Increment re-entry count for parent frame if it exists
            if let Some(parent_frame_id) = self.frame_stack.last_mut() {
                parent_frame_id.increment_re_entry();
            }

            // Add placeholder for the new frame
            if let Some(current_frame_id) = self.current_frame_id() {
                self.snapshots.add_frame_placeholder(current_frame_id);
            }

            Some(frame_id)
        } else {
            None
        }
    }

    /// Update database when storage is changed
    fn update_database(&mut self, ctx: &EdbContext<DB>, force_update: bool) {
        if force_update
            || self
                .last_opcode
                .map(|c| c.modifies_evm_state() && !c.is_message_call())
                .unwrap_or(force_update)
        {
            // Clone current database state
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

    /// Check if this is a hook trigger call and record snapshot if so
    fn check_and_record_hook(
        &mut self,
        data: &[u8],
        interp: &Interpreter,
        ctx: &mut EdbContext<DB>,
    ) {
        let address = self
            .current_frame_id()
            .and_then(|frame_id| self.trace.get(frame_id.trace_entry_id()))
            .map(|entry| entry.code_address)
            .unwrap_or(interp.input.target_address());

        let usid_opt = if data.len() >= 32 {
            U256::from_be_slice(&data[..32]).try_into().ok()
        } else {
            error!("KECCAK256 input data too short for snapshot, skipping");
            return;
        };

        let Some(usid) = usid_opt else {
            error!("Hook call data does not contain valid USID, skipping snapshot");
            return;
        };

        // Resolve the analysis result for this address. Falls back through the
        // codehash alias index when the address itself isn't registered (the
        // `vm.etch(target, address(impl).code)` case).
        let Some(analysis) = self.resolve_analysis(address, interp) else {
            error!(
                address=?address,
                usid=?usid,
                "No analysis found for address (direct or via codehash alias), skipping hook snapshot",
            );
            return;
        };
        let Some(step) = analysis.usid_to_step.get(&usid) else {
            error!(
                address=?address,
                usid=?usid,
                "No analysis step found for USID, skipping hook snapshot",
            );
            return;
        };

        // Collect values of accessible variables
        let mut locals = HashMap::new();
        for variable in step.accessible_variables() {
            if variable.declaration().state_variable {
                continue;
            }
            let uvid = variable.id();
            let name = variable.declaration().name.clone();
            locals.insert(name, self.uvid_values.get(&uvid).cloned());
        }

        // Update the last frame with this snapshot
        if let Some(current_frame_id) = self.current_frame_id() {
            if let Some(entry) = self.trace.get(current_frame_id.trace_entry_id()) {
                // Create hook snapshot
                let hook_snapshot = HookSnapshot {
                    target_address: entry.target,
                    bytecode_address: entry.code_address,
                    database: self.database.clone(),
                    transient_storage: self.transient_storage.clone(),
                    locals,
                    usid,
                    state_variables: HashMap::new(), // State variables can be filled in later
                    block_env: ctx.block.clone(),
                    cfg_env: ctx.cfg.clone(),
                };

                self.snapshots.update_last_frame_with_snapshot(current_frame_id, hook_snapshot);
            } else {
                error!("No trace entry found for frame {}", current_frame_id);
            }
        } else {
            error!("No current frame to update with hook snapshot");
        }
    }

    fn check_and_record_variable_update(
        &mut self,
        data: &[u8],
        interp: &Interpreter,
        _ctx: &mut EdbContext<DB>,
    ) {
        let address = self
            .current_frame_id()
            .and_then(|frame_id| self.trace.get(frame_id.trace_entry_id()))
            .map(|entry| entry.code_address)
            .unwrap_or(interp.input.target_address());

        // The data is decoded as (uint256 magic, uint256 uvid, abi.encode(value))
        // So the data will be organized as
        //      -32 .. 0 : [uint256 magic] (parsed before this function)
        //        0 .. 32: [uint256 uvid]
        //       32 .. 64: [offset] (should be 0x60 considering the first two uint256)
        //       64 .. 96: [length of encoded value]
        //       96 .. _ : [encoded value]
        if data.len() < 96 {
            error!(
                address=?address,
                "KECCAK256 input data too short for variable update value, skipping"
            );
            return;
        }

        let Some(uvid) = U256::from_be_slice(&data[..32]).try_into().ok() else {
            error!("Hook call data does not contain valid UVID, skipping snapshot");
            return;
        };

        let offset = U256::from_be_slice(&data[32..64]);
        if offset != U256::from(0x60) {
            error!(
                address=?address,
                uvid=?uvid,
                offset=?offset,
                "Unexpected offset for variable update value, skipping"
            );
            return;
        }

        let length = U256::from_be_slice(&data[64..96]);
        let length_usize = match usize::try_from(length) {
            Ok(l) => l,
            Err(_) => {
                error!(
                    address=?address,
                    uvid=?uvid,
                    length=?length,
                    "Variable update value length too large, skipping"
                );
                return;
            }
        };

        let decoded_data = &data[96..96 + length_usize];

        let Some(analysis) = self.resolve_analysis(address, interp) else {
            error!(
                address=?address,
                uvid=?uvid,
                "No analysis found for address (direct or via codehash alias), \
                 skipping variable update recording",
            );
            return;
        };
        let Some(variable) = analysis.uvid_to_variable.get(&uvid) else {
            error!(
                address=?address,
                uvid=?uvid,
                "No variable found for address and UVID, skipping variable update recording",
            );
            return;
        };

        let value =
            match decode_variable_value(&analysis.user_defined_types, variable, decoded_data) {
                Ok(v) => v,
                Err(e) => {
                    error!(
                        address=?address,
                        uvid=?uvid,
                        variable=?variable.declaration().type_descriptions.type_string,
                        type_name = ?variable.declaration().type_name,
                        data=?hex::encode(decoded_data),
                        error=?e,
                    );
                    return;
                }
            };

        debug!(
            uvid=?uvid,
            address=?address,
            variable=?variable.declaration().name,
            value=?value,
            "Found variable update",
        );

        self.uvid_values.insert(uvid, Arc::new(value.into()));
    }

    /// Check and apply creation hooks if the bytecode matches
    fn check_and_apply_creation_hooks(
        &mut self,
        inputs: &mut CreateInputs,
        ctx: &mut EdbContext<DB>,
    ) {
        // Get the nonce from the caller account
        let Ok(account) = ctx.journaled_state.load_account(inputs.caller()) else {
            error!("Failed to load account for caller {:?}", inputs.caller());
            return;
        };

        // Calculate what address would be created using the built-in method
        let nonce = account.info.nonce;
        let predicted_address = inputs.created_address(nonce);

        debug!(
            target: "edb::hook::creation",
            caller = ?inputs.caller(),
            predicted = ?predicted_address,
            init_code_len = inputs.init_code().len(),
            num_hooks = self.creation_hooks.len(),
            "CREATE intercepted; scanning hooks",
        );

        // ---------- Step 1: try address-keyed substitution ----------
        //
        // We have a pre-computed map of `predicted_address → (hooked_creation,
        // args_size_bytes)` for every contract in `contracts_in_tx` (the
        // addresses whose source we have an artifact for). If the predicted
        // address of *this* CREATE is in that map, we substitute regardless
        // of whether the bytecode-prefix heuristic below would have matched.
        //
        // This handles **nested CREATE**s where the parent contract embeds a
        // copy of the child's creation bytecode that does *not*
        // byte-identically match the child's standalone artifact bytecode —
        // Solidity emits different deployed code (and thus different
        // creation wrappers) for an inlined-via-parent compilation vs. a
        // standalone compilation of the same source (different optimizer
        // context, library refs, metadata hash, etc.), which breaks the
        // exact-match path below.
        //
        // `args_size_bytes` was computed at registration time from the
        // contract's constructor ABI — exact for static-typed constructors
        // (`uint256`, `address`, `bool`, `bytesN`, fixed-size arrays of those)
        // and an underestimate for dynamic-typed ones (`bytes`, `string`,
        // dynamic arrays); for the dynamic case the substituted constructor
        // would receive truncated args. None of the current foundry-test
        // fixtures exercise dynamic-args constructors at the nested layer,
        // so we accept the limitation; the static path covers every test we
        // ship.
        if let Some((hooked_creation, args_size)) =
            self.creation_by_address.get(&predicted_address).cloned()
        {
            let init_code = inputs.init_code();
            let runtime_args: Bytes = if args_size == 0 {
                Bytes::default()
            } else if init_code.len() >= args_size {
                Bytes::from(init_code[init_code.len() - args_size..].to_vec())
            } else {
                Bytes::default()
            };

            let mut new_init_code = Vec::with_capacity(hooked_creation.len() + runtime_args.len());
            new_init_code.extend_from_slice(hooked_creation.as_ref());
            new_init_code.extend_from_slice(runtime_args.as_ref());
            inputs.set_init_code(Bytes::from(new_init_code));
            inputs.set_scheme(CreateScheme::Custom { address: predicted_address });

            debug!(
                target: "edb::hook::creation",
                caller = ?inputs.caller(),
                predicted = ?predicted_address,
                args_len = runtime_args.len(),
                strategy = "address-keyed",
                "creation bytecode replaced with hooked version",
            );
            return;
        }

        // ---------- Step 2: fall back to bytecode-prefix heuristic ----------
        //
        // For contracts NOT in `creation_by_address` (e.g. legacy Etherscan
        // path where addresses aren't pre-registered), keep the original
        // logic: peel the recorded `constructor_args` off the tail and do an
        // exact bytecode-prefix check.
        for (hook_idx, (original_bytecode, hooked_bytecode, constructor_args)) in
            self.creation_hooks.iter().enumerate()
        {
            if inputs.init_code().len() >= constructor_args.len() {
                let input_args_start = inputs.init_code().len() - constructor_args.len();
                let input_args = &inputs.init_code()[input_args_start..];

                if input_args == constructor_args.as_ref() {
                    let input_bytecode = &inputs.init_code()[..input_args_start];

                    if input_bytecode == original_bytecode.as_ref() {
                        let mut new_init_code = Vec::from(hooked_bytecode.as_ref());
                        new_init_code.extend_from_slice(constructor_args.as_ref());
                        inputs.set_init_code(Bytes::from(new_init_code));

                        inputs.set_scheme(CreateScheme::Custom { address: predicted_address });

                        debug!(
                            target: "edb::hook::creation",
                            hook = hook_idx,
                            caller = ?inputs.caller(),
                            predicted = ?predicted_address,
                            strategy = "bytecode-prefix",
                            "creation bytecode replaced with hooked version",
                        );

                        break;
                    }
                }
            }
        }
    }

    /// Clear all recorded data
    pub fn clear(&mut self) {
        self.snapshots.snapshots.clear();
        self.frame_stack.clear();
        self.current_trace_id = 0;
    }

    /// Resolve an `AnalysisResult` for the given address.
    ///
    /// Tries three sources in order:
    ///   1. Cached redirect from a previous codehash-fallback hit.
    ///   2. Direct `analysis[address]` lookup (the common case).
    ///   3. Codehash fallback: hash the live runtime bytecode currently
    ///      executing in `interp` and look up `codehash_to_canonical[hash]`.
    ///      On hit, cache the redirect so future calls are O(1).
    ///
    /// Returns `None` when no resolution is possible (genuinely-unknown
    /// bytecode — the current "no analysis found" path).
    fn resolve_analysis(
        &mut self,
        address: Address,
        interp: &Interpreter,
    ) -> Option<&'a AnalysisResult> {
        // Step 1: cached redirect.
        if let Some(canonical) = self.canonical_by_alias.get(&address) {
            return self.analysis.get(canonical);
        }
        // Step 2: direct hit.
        if let Some(a) = self.analysis.get(&address) {
            return Some(a);
        }
        // Step 3: codehash fallback. The bytes currently executing in
        // `interp` at `address` are the etched / instrumented runtime
        // bytecode in Pass 3 — exactly what we keccak'd when building
        // the `codehash_to_canonical` index in `prepare`.
        let raw_code = interp.bytecode.original_byte_slice();
        if raw_code.is_empty() {
            return None;
        }
        let code_hash = keccak256(raw_code);
        let canonical = *self.codehash_to_canonical.get(&code_hash)?;
        self.canonical_by_alias.insert(address, canonical);
        debug!(
            ?address,
            ?canonical,
            ?code_hash,
            "hook inspector resolved address via codehash alias"
        );
        self.analysis.get(&canonical)
    }
}

impl<'a, DB> Inspector<EdbContext<DB>> for HookSnapshotInspector<'a, DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn step(&mut self, interp: &mut Interpreter, ctx: &mut EdbContext<DB>) {
        // Relax gas limit at call site
        relax_gas_limit_at_callsite(interp);

        // Get current opcode safely
        let opcode = unsafe { OpCode::new_unchecked(interp.bytecode.opcode()) };
        self.last_opcode = Some(opcode);

        if opcode != OpCode::KECCAK256 {
            // KECCAK256 is the only hooked opcode.
            return;
        }

        let Some(data) = interp.stack.pop().ok().and_then(|offset_u256| {
            let data = interp.stack.pop().ok().and_then(|len_u256| {
                let offset = usize::try_from(offset_u256).ok()?;
                let len = usize::try_from(len_u256).ok()?;
                let data = interp.memory.slice_len(offset, len);

                let _ = interp.stack.push(len_u256);
                Some(data)
            });

            let _ = interp.stack.push(offset_u256);
            data
        }) else {
            error!("Failed to read KECCAK256 input data from stack");
            return;
        };

        if data.len() < 32 {
            // Not enough data for at least two U256 values
            return;
        }

        let magic_number = U256::from_be_slice(&data[..32]);

        if magic_number == MAGIC_SNAPSHOT_NUMBER {
            self.check_and_record_hook(&data[32..], interp, ctx);
        } else if magic_number == MAGIC_VARIABLE_UPDATE_NUMBER {
            self.check_and_record_variable_update(&data[32..], interp, ctx);
        }
    }

    fn step_end(&mut self, _interp: &mut Interpreter, context: &mut EdbContext<DB>) {
        self.update_database(context, false);
    }

    fn call(
        &mut self,
        context: &mut EdbContext<DB>,
        _inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        // Update database
        self.update_database(context, true);

        // Start tracking new execution frame for regular calls only
        self.push_frame(self.current_trace_id);
        self.current_trace_id += 1;

        None
    }

    fn call_end(
        &mut self,
        context: &mut EdbContext<DB>,
        inputs: &CallInputs,
        outcome: &mut CallOutcome,
    ) {
        // Update database
        self.update_database(context, true);

        let Some(frame_id) = self.pop_frame() else { return };

        let Some(entry) = self.trace.get(frame_id.trace_entry_id()) else { return };

        if entry.result != Some(outcome.into()) {
            // Mismatched call outcome
            error!(
                target_address = inputs.target_address.to_string(),
                bytecode_address = inputs.bytecode_address.to_string(),
                "Call outcome mismatch at frame {}: expected {:?}, got {:?} ({:?})",
                frame_id,
                entry.result,
                Into::<CallResult>::into(&outcome),
                outcome,
            );
        }
    }

    fn create(
        &mut self,
        context: &mut EdbContext<DB>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        // Update database
        self.update_database(context, true);

        // Check and apply creation hooks if applicable
        self.check_and_apply_creation_hooks(inputs, context);

        // Start tracking new execution frame for contract creation
        self.push_frame(self.current_trace_id);
        self.current_trace_id += 1;

        None
    }

    fn create_end(
        &mut self,
        context: &mut EdbContext<DB>,
        _inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        // Update database
        self.update_database(context, true);

        // Stop tracking current execution frame
        let Some(frame_id) = self.pop_frame() else { return };

        let Some(entry) = self.trace.get(frame_id.trace_entry_id()) else { return };

        // For creation, we only check the return status, not the actually bytecode, since we
        // will instrument the code
        if entry.result.as_ref().map(|r| r.result()) != Some(outcome.result.result) {
            // Mismatch create outcome
            error!(
                "Create outcome mismatch at frame {}: expected {:?}, got {:?}",
                frame_id, entry.result, outcome
            );
        }
    }
}

/// Pretty printing utilities for debugging
impl<DB> HookSnapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Print comprehensive summary of hook snapshots
    pub fn print_summary(&self) {
        println!(
            "\n\x1b[36m╔══════════════════════════════════════════════════════════════════╗\x1b[0m"
        );
        println!(
            "\x1b[36m║              HOOK SNAPSHOT INSPECTOR SUMMARY                     ║\x1b[0m"
        );
        println!(
            "\x1b[36m╚══════════════════════════════════════════════════════════════════╝\x1b[0m\n"
        );

        // Overall statistics
        let total_frames = self.len();
        let hook_frames = self.get_frames_with_hooks().len();

        println!("\x1b[33m📊 Overall Statistics:\x1b[0m");
        println!("  Total frames tracked: \x1b[32m{total_frames}\x1b[0m");
        println!("  Frames with hooks: \x1b[32m{hook_frames}\x1b[0m");
        println!(
            "  Hook trigger rate: \x1b[32m{:.1}%\x1b[0m",
            if total_frames > 0 { hook_frames as f64 / total_frames as f64 * 100.0 } else { 0.0 }
        );

        if self.is_empty() {
            println!("\n\x1b[90m  No execution frames were tracked.\x1b[0m");
            return;
        }

        println!("\n\x1b[33m🎯 Hook Trigger Details:\x1b[0m");
        println!(
            "\x1b[90m─────────────────────────────────────────────────────────────────\x1b[0m"
        );

        // Group snapshots by frame ID
        use std::collections::HashMap;
        let mut frame_groups: HashMap<ExecutionFrameId, Vec<&HookSnapshot<DB>>> = HashMap::new();
        let mut frame_order = Vec::new();

        for (frame_id, snapshot) in &self.snapshots {
            if !frame_groups.contains_key(frame_id) {
                frame_order.push(*frame_id);
            }

            match snapshot {
                Some(hook_snapshot) => {
                    frame_groups.entry(*frame_id).or_default().push(hook_snapshot);
                }
                None => {
                    // Ensure frame exists in map even if empty
                    frame_groups.entry(*frame_id).or_default();
                }
            }
        }

        // Print grouped results in original order
        for (display_idx, frame_id) in frame_order.iter().enumerate() {
            let hooks = frame_groups.get(frame_id).unwrap();

            if hooks.is_empty() {
                // Frame with no hooks
                println!(
                    "  \x1b[90m[{:3}] Frame {}\x1b[0m (trace.{}, re-entry {}) - No hooks",
                    display_idx,
                    frame_id,
                    frame_id.trace_entry_id(),
                    frame_id.re_entry_count()
                );
            } else {
                // Frame with hooks - collect all USIDs in execution order (no sorting)
                let usids: Vec<_> = hooks.iter().map(|h| h.usid).collect();
                let hook_count = hooks.len();
                #[allow(deprecated)]
                let addresses: std::collections::HashSet<_> =
                    hooks.iter().map(|h| h.bytecode_address).collect();

                println!(
                    "\n  \x1b[32m[{:3}] Frame {}\x1b[0m (trace.{}, re-entry {})",
                    display_idx,
                    frame_id,
                    frame_id.trace_entry_id(),
                    frame_id.re_entry_count()
                );
                println!(
                    "       └─ \x1b[33m{} Hook{} Triggered\x1b[0m",
                    hook_count,
                    if hook_count == 1 { "" } else { "s" }
                );

                // Show addresses (usually just one per frame)
                for address in &addresses {
                    println!("          ├─ Address: \x1b[36m{address:?}\x1b[0m");
                }

                // Show USIDs in execution order with smart formatting
                if usids.len() == 1 {
                    println!("          └─ USID: \x1b[36m{}\x1b[0m", usids[0]);
                } else if usids.len() <= 10 {
                    // Show all USIDs for small lists
                    let usid_list: Vec<String> = usids.iter().map(|u| u.to_string()).collect();
                    println!("          └─ USIDs: \x1b[36m[{}]\x1b[0m", usid_list.join(", "));
                } else {
                    // For large lists, show first few, count, and last few
                    let first_few: Vec<String> =
                        usids.iter().take(3).map(|u| u.to_string()).collect();
                    let last_few: Vec<String> =
                        usids.iter().rev().take(3).rev().map(|u| u.to_string()).collect();

                    if first_few.last() == last_few.first() {
                        // Handle overlap case (shouldn't happen with take(3) for >10 items, but
                        // defensive)
                        println!(
                            "          └─ USIDs: \x1b[36m[{} ... {} total]\x1b[0m",
                            first_few.join(", "),
                            usids.len()
                        );
                    } else {
                        println!(
                            "          └─ USIDs: \x1b[36m[{}, ... {}, {} total]\x1b[0m",
                            first_few.join(", "),
                            last_few.join(", "),
                            usids.len()
                        );
                    }
                }
            }
        }

        println!(
            "\n\x1b[90m─────────────────────────────────────────────────────────────────\x1b[0m"
        );
        println!("\x1b[33m💡 Magic Snapshot Number:\x1b[0m {MAGIC_SNAPSHOT_NUMBER:?}");
    }
}

/// Decode the variable value from the given ABI-encoded data according to the variable declaration.
///
/// This function takes raw ABI-encoded data and decodes it according to the variable's
/// type information from its declaration. It handles both primitive Solidity types
/// (uint, address, bool, etc.) and user-defined types (structs, enums).
///
/// # Arguments
/// * `user_defined_types` - Mapping of type IDs to user-defined type references for resolving
///   custom types
/// * `variable` - The variable reference containing the declaration and type information
/// * `data` - The ABI-encoded variable value as raw bytes
///
/// # Returns
/// The decoded variable value as a [`DynSolValue`] that can be used in expression evaluation
///
/// # Errors
/// Returns an error if:
/// - The variable declaration lacks type information
/// - The type cannot be resolved from the declaration
/// - The data cannot be ABI-decoded according to the resolved type
///
/// # Example
/// ```rust,ignore
/// let decoded = decode_variable_value(&user_types, &variable_ref, &encoded_data)?;
/// match decoded {
///     DynSolValue::Uint(val, _) => println!("Uint value: {}", val),
///     DynSolValue::Address(addr) => println!("Address: {}", addr),
///     _ => println!("Other type: {:?}", decoded),
/// }
/// ```
pub fn decode_variable_value(
    user_defined_types: &HashMap<usize, UserDefinedTypeRef>,
    variable: &VariableRef,
    data: &[u8],
) -> Result<DynSolValue> {
    let type_name = variable
        .type_name()
        .as_ref()
        .ok_or(eyre::eyre!("Failed to get variable type: no type name in the declaration"))?;
    let Some(variable_type): Option<DynSolType> = dyn_sol_type(user_defined_types, type_name)
    else {
        return Err(eyre::eyre!("Failed to get variable type: no type string in the declaration"));
    };
    let value = variable_type
        .abi_decode(data)
        .map_err(|e| eyre::eyre!("Failed to decode variable value: {}", e))?;
    Ok(value)
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
    fn hook_snapshot_carries_block_env() {
        let s = HookSnapshot::<TestDB>::default();
        let _: &BlockEnv = &s.block_env;
        let _: &CfgEnv = &s.cfg_env;
    }
}
