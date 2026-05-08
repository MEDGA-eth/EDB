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

//! Execution state management for TUI panels
//!
//! This module implements a two-tier architecture for execution state management:
//!
//! - `ExecutionManager`: Per-thread instance with cached execution state for immediate reads
//! - `ExecutionManagerCore`: Shared core that handles trace data fetching and complex operations
//!
//! This design ensures rendering threads can access execution state without blocking
//! on RPC calls or complex trace processing.

use alloy_primitives::{Address, U256};
use eyre::{Result, bail};
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::Arc,
};
use tokio::sync::RwLock;
use tracing::{debug, error};

use edb_common::types::{Breakpoint, BreakpointLocation, Code, SnapshotInfo, Trace};

use crate::{
    RpcClient,
    data::manager::core::{
        FetchCache, ManagerCore, ManagerInner, ManagerRequestTr, ManagerStateTr, ManagerTr,
    },
};

#[derive(Debug, Clone)]
pub struct ExecutionState {
    snapshot_count: usize,
    snapshot_info: FetchCache<usize, SnapshotInfo>,
    code: FetchCache<Address, Code>,
    next_call: FetchCache<usize, usize>,
    prev_call: FetchCache<usize, usize>,
    storage: FetchCache<(usize, U256), U256>,
    storage_diff: FetchCache<usize, HashMap<U256, (U256, U256)>>,
    breakpoint_hits: FetchCache<Breakpoint, Vec<usize>>,
    trace_data: Trace,
}

impl ManagerStateTr for ExecutionState {
    async fn with_rpc_client(rpc_client: Arc<RpcClient>) -> Result<Self> {
        let snapshot_count = rpc_client.get_snapshot_count().await?;
        let trace_data = rpc_client.get_trace().await?;
        Ok(Self {
            snapshot_count,
            snapshot_info: FetchCache::new(),
            code: FetchCache::new(),
            next_call: FetchCache::new(),
            prev_call: FetchCache::new(),
            storage: FetchCache::new(),
            storage_diff: FetchCache::new(),
            breakpoint_hits: FetchCache::new(),
            trace_data,
        })
    }

    fn update(&mut self, other: &Self) {
        if self.snapshot_info.need_update(&other.snapshot_info) {
            self.snapshot_info.update(&other.snapshot_info);
        }

        if self.code.need_update(&other.code) {
            self.code.update(&other.code);
        }

        if self.next_call.need_update(&other.next_call) {
            self.next_call.update(&other.next_call);
        }

        if self.prev_call.need_update(&other.prev_call) {
            self.prev_call.update(&other.prev_call);
        }

        if self.storage.need_update(&other.storage) {
            self.storage.update(&other.storage);
        }

        if self.storage_diff.need_update(&other.storage_diff) {
            self.storage_diff.update(&other.storage_diff);
        }

        if self.breakpoint_hits.need_update(&other.breakpoint_hits) {
            self.breakpoint_hits.update(&other.breakpoint_hits);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExecutionRequest {
    SnapshotInfo(usize),
    Code(usize),
    CodeByAddress(Address),
    NextCall(usize),
    PrevCall(usize),
    Storage(usize, U256),
    StorageDiff(usize),
    BreakpointHits(Breakpoint),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExecutionStatus {
    Normal,
    WaitNextCall(usize),
    WaitPrevCall(usize),
    WaitBreakpointHits(Breakpoint),
}

impl ExecutionStatus {
    pub fn is_waiting(&self) -> bool {
        !matches!(self, Self::Normal)
    }
}

impl ManagerRequestTr<ExecutionState> for ExecutionRequest {
    async fn fetch_data(
        self,
        rpc_client: Arc<RpcClient>,
        state: &mut ExecutionState,
    ) -> Result<()> {
        match self {
            Self::SnapshotInfo(ref id) => {
                if state.snapshot_info.contains_key(id) {
                    return Ok(());
                }

                let info = rpc_client.get_snapshot_info(*id).await?;
                state.snapshot_info.insert(*id, Some(info));
            }
            Self::Code(ref id) => {
                if state.snapshot_info.get(id).is_none() {
                    let info = rpc_client.get_snapshot_info(*id).await?;
                    state.snapshot_info.insert(*id, Some(info));
                }

                let entry_id = state
                    .snapshot_info
                    .get(id)
                    .and_then(|info| info.as_ref().map(|i| i.frame_id()))
                    .ok_or_else(|| eyre::eyre!("Snapshot info for id {id} not found"))?
                    .trace_entry_id();
                let bytecode_address = state
                    .trace_data
                    .get(entry_id)
                    .map(|e| e.code_address)
                    .ok_or_else(|| eyre::eyre!("Trace entry with id {} not found", entry_id))?;

                if state.code.contains_key(&bytecode_address) {
                    return Ok(());
                }

                let code = rpc_client.get_code(*id).await?;
                state.code.insert(bytecode_address, Some(code));
            }
            Self::CodeByAddress(ref address) => {
                if state.code.contains_key(address) {
                    return Ok(());
                }

                let code = rpc_client.get_code_by_address(*address).await?;
                state.code.insert(*address, Some(code));
            }
            Self::NextCall(ref id) => {
                if state.next_call.contains_key(id) {
                    return Ok(());
                }

                let next_call = rpc_client.get_next_call(*id).await?;

                // We separate this insertion since id might be equal to next_call
                state.next_call.insert(*id, Some(next_call));

                // We will update all snapshots in the range
                for i in id + 1..next_call {
                    state.next_call.insert(i, Some(next_call));
                }
            }
            Self::PrevCall(ref id) => {
                if state.prev_call.contains_key(id) {
                    return Ok(());
                }

                let prev_call = rpc_client.get_prev_call(*id).await?;

                // We separate this insertion since id might be equal to prev_call
                state.prev_call.insert(*id, Some(prev_call));

                // We will update all snapshots in the range
                for i in prev_call + 1..*id {
                    state.prev_call.insert(i, Some(prev_call));
                }
            }
            Self::Storage(ref id, ref slot) => {
                if state.storage.contains_key(&(*id, *slot)) {
                    return Ok(());
                }

                let value = rpc_client.get_storage(*id, *slot).await?;
                state.storage.insert((*id, *slot), Some(value));
            }
            Self::StorageDiff(ref id) => {
                if state.storage_diff.contains_key(id) {
                    return Ok(());
                }

                let diff = rpc_client.get_storage_diff(*id).await?;
                state.storage_diff.insert(*id, Some(diff));
            }
            Self::BreakpointHits(ref breakpoint) => {
                if state.breakpoint_hits.contains_key(breakpoint) {
                    return Ok(());
                }

                let hits = rpc_client.get_breakpoint_hits(breakpoint).await?;
                state.breakpoint_hits.insert(breakpoint.clone(), Some(hits));
            }
        }

        Ok(())
    }
}

/// Per-thread execution manager with cached state for rendering
///
/// # Design Philosophy
///
/// `ExecutionManager` provides immediate access to execution state:
/// - All fields are directly accessible for non-blocking reads
/// - State updates happen through explicit `fetch_data()` calls
/// - Complex trace processing is handled by ExecutionManagerCore
///
/// # Cached Fields
///
/// All public fields are cached locally for immediate access:
/// - `current_snapshot`: Current execution position
/// - `total_snapshots`: Total execution steps available
/// - `current_line`: Active source line being executed
/// - `current_file`: Active source file
/// - `is_paused`: Execution pause state
/// - `trace_data`: Full trace data for rendering
///
/// # Usage Pattern
///
/// ```ignore
/// // In rendering loop - all reads are immediate
/// let line = execution_manager.current_line;
/// let paused = execution_manager.is_paused;
///
/// // When state changes
/// execution_manager.fetch_data().await?; // Sync with core
/// ```
#[derive(Debug, Clone)]
pub struct ExecutionManager {
    // Execution status
    execution_status: ExecutionStatus,

    // User-controlled data
    current_snapshot: usize,
    display_snapshot: usize,

    // Breakpoints
    breakpoint_set: HashSet<Breakpoint>,
    breakpoints: Vec<(Breakpoint, bool)>, // bool indicates enabled/disabled

    /// State
    state: ExecutionState,

    /// Pending request
    pending_requests: HashSet<ExecutionRequest>,
    core: Arc<RwLock<ManagerCore<ExecutionState, ExecutionRequest>>>,
}

// Data management
impl ManagerTr<ExecutionState, ExecutionRequest> for ExecutionManager {
    fn get_inner<'a>(&'a mut self) -> ManagerInner<'a, ExecutionState, ExecutionRequest> {
        ManagerInner {
            core: &mut self.core,
            state: &mut self.state,
            pending_requests: &mut self.pending_requests,
        }
    }

    fn get_core(&self) -> Arc<RwLock<ManagerCore<ExecutionState, ExecutionRequest>>> {
        self.core.clone()
    }
}

impl ExecutionManager {
    /// Create new execution manager with shared core
    pub async fn new(core: Arc<RwLock<ManagerCore<ExecutionState, ExecutionRequest>>>) -> Self {
        let mut mgr = Self {
            state: core.clone().read().await.state.clone(),
            pending_requests: HashSet::new(),
            execution_status: ExecutionStatus::Normal,
            core,
            current_snapshot: 0,
            display_snapshot: 0,
            breakpoints: Vec::new(),
            breakpoint_set: HashSet::new(),
        };

        let _ = mgr.goto_snapshot(0, false);
        let _ = mgr.display_snapshot(0);
        mgr
    }

    pub fn get_sanitized_id(&self, id: usize) -> usize {
        id.min(self.state.snapshot_count - 1)
    }

    pub fn get_storage(&mut self, id: usize, slot: U256) -> Option<&U256> {
        let _ = self.pull_from_core();

        let id = self.get_sanitized_id(id);
        if !self.state.storage.contains_key(&(id, slot)) {
            debug!("Storage not found in cache, fetching...");
            self.new_fetching_request(ExecutionRequest::Storage(id, slot));
            return None;
        }

        match self.state.storage.get(&(id, slot)) {
            Some(value) => value.as_ref(),
            _ => None,
        }
    }

    pub fn get_storage_diff(&mut self, id: usize) -> Option<&HashMap<U256, (U256, U256)>> {
        let _ = self.pull_from_core();

        let id = self.get_sanitized_id(id);
        if !self.state.storage_diff.contains_key(&id) {
            debug!("Storage diff not found in cache, fetching...");
            self.new_fetching_request(ExecutionRequest::StorageDiff(id));
            return None;
        }

        match self.state.storage_diff.get(&id) {
            Some(diff) => diff.as_ref(),
            _ => None,
        }
    }

    pub fn get_breakpoint_hits(&mut self, breakpoint: &Breakpoint) -> Option<&Vec<usize>> {
        let _ = self.pull_from_core();

        if !self.state.breakpoint_hits.contains_key(breakpoint) {
            debug!("Breakpoint hits not found in cache, fetching...");
            self.new_fetching_request(ExecutionRequest::BreakpointHits(breakpoint.clone()));
            return None;
        }

        match self.state.breakpoint_hits.get(breakpoint) {
            Some(hits) => hits.as_ref(),
            _ => None,
        }
    }

    pub fn get_snapshot_info(&mut self, id: usize) -> Option<&SnapshotInfo> {
        let _ = self.pull_from_core();

        let id = self.get_sanitized_id(id);
        if !self.state.snapshot_info.contains_key(&id) {
            debug!("Snapshot info not found in cache, fetching...");
            self.new_fetching_request(ExecutionRequest::SnapshotInfo(id));
            return None;
        }

        match self.state.snapshot_info.get(&id) {
            Some(info) => info.as_ref(),
            _ => None,
        }
    }

    pub fn get_current_address(&mut self) -> Option<Address> {
        let _ = self.pull_from_core();

        let current_id = self.get_current_snapshot();
        let snapshot_info = self.get_snapshot_info(current_id)?;
        let entry_id = snapshot_info.frame_id().trace_entry_id();
        let entry = self.get_trace().get(entry_id)?;

        Some(entry.target)
    }

    pub fn get_current_bytecode_address(&mut self) -> Option<Address> {
        let _ = self.pull_from_core();

        let current_id = self.get_current_snapshot();
        let snapshot_info = self.get_snapshot_info(current_id)?;
        let entry_id = snapshot_info.frame_id().trace_entry_id();
        let entry = self.get_trace().get(entry_id)?;

        Some(entry.code_address)
    }

    pub fn get_next_call(&mut self, id: usize) -> Option<usize> {
        let _ = self.pull_from_core();

        let id = self.get_sanitized_id(id);
        if !self.state.next_call.contains_key(&id) {
            debug!("Next call info not found in cache, fetching...");
            self.new_fetching_request(ExecutionRequest::NextCall(id));
            return None;
        }

        match self.state.next_call.get(&id) {
            Some(next_id) => *next_id,
            _ => None,
        }
    }

    pub fn get_prev_call(&mut self, id: usize) -> Option<usize> {
        let _ = self.pull_from_core();

        let id = self.get_sanitized_id(id);
        if !self.state.prev_call.contains_key(&id) {
            debug!("Prev call info not found in cache, fetching...");
            self.new_fetching_request(ExecutionRequest::PrevCall(id));
            return None;
        }

        match self.state.prev_call.get(&id) {
            Some(prev_id) => *prev_id,
            _ => None,
        }
    }

    pub fn get_execution_status(&self) -> ExecutionStatus {
        self.execution_status.clone()
    }

    pub fn get_snapshot_count(&self) -> usize {
        self.state.snapshot_count
    }

    pub fn get_trace(&self) -> &Trace {
        &self.state.trace_data
    }

    pub fn get_code(&mut self, id: usize) -> Option<&Code> {
        let _ = self.pull_from_core();

        let id = self.get_sanitized_id(id);
        let Some(entry_id) =
            self.get_snapshot_info(id).map(|info| info.frame_id().trace_entry_id())
        else {
            debug!("Code not found in cache, fetching...");
            self.new_fetching_request(ExecutionRequest::Code(id));
            return None;
        };

        let Some(bytecode_address) = self.get_trace().get(entry_id).map(|e| e.code_address) else {
            error!("Invalid trace entry id {entry_id}");
            return None;
        };

        if !self.state.code.contains_key(&bytecode_address) {
            debug!("Code not found in cache, fetching...");
            self.new_fetching_request(ExecutionRequest::Code(id));
            return None;
        }

        match self.state.code.get(&bytecode_address) {
            Some(code) => code.as_ref(),
            _ => None,
        }
    }

    pub fn get_code_by_bytecode_address(&mut self, address: Address) -> Option<&Code> {
        let _ = self.pull_from_core();

        if !self.state.code.contains_key(&address) {
            debug!("Code not found in cache, fetching...");
            self.new_fetching_request(ExecutionRequest::CodeByAddress(address));
            return None;
        }

        match self.state.code.get(&address) {
            Some(code) => code.as_ref(),
            _ => None,
        }
    }

    pub fn get_current_snapshot(&mut self) -> usize {
        let _ = self.check_pending_request();
        self.current_snapshot
    }

    pub fn get_display_snapshot(&mut self) -> usize {
        let _ = self.check_pending_request();
        self.display_snapshot
    }

    fn display_snapshot(&mut self, id: usize) -> Result<()> {
        if id >= self.state.snapshot_count {
            Err(eyre::eyre!(
                "Snapshot id {} out of bounds (total {})",
                id,
                self.state.snapshot_count
            ))
        } else {
            self.get_snapshot_info(id);
            self.get_code(id);

            self.display_snapshot = id;

            Ok(())
        }
    }

    fn goto_snapshot(&mut self, id: usize, stop_on_breakpoint: bool) -> Result<usize> {
        if id >= self.state.snapshot_count {
            Err(eyre::eyre!(
                "Snapshot id {} out of bounds (total {})",
                id,
                self.state.snapshot_count
            ))
        } else {
            debug!("Going to snapshot {id}, stop_on_breakpoint={stop_on_breakpoint}");

            let id = if stop_on_breakpoint {
                self.find_breakpoint_hit_in_range(self.current_snapshot, id)
            } else {
                id
            };

            self.get_snapshot_info(id);
            self.get_code(id);

            // Use try_write instead of blocking write
            self.current_snapshot = id;

            Ok(id)
        }
    }

    // Check whether the pending request can be fulfilled.
    // Return true if any following execution operation can be performed, false otherwise.
    fn check_pending_request(&mut self) -> bool {
        match self.execution_status {
            ExecutionStatus::WaitNextCall(src_id) => {
                // There is a pending execution request, for which we should wait
                // and should not update current_snapshot
                if let Some(next_id) = self.get_next_call(src_id) {
                    // The pending request is ready, we can proceed
                    self.execution_status = ExecutionStatus::Normal;

                    // We will override the current snapshot
                    let _ = self
                        .goto_snapshot(next_id, true)
                        .and_then(|to_id| self.display_snapshot(to_id));
                }

                // Any other execution request will be rejected
                false
            }
            ExecutionStatus::WaitPrevCall(src_id) => {
                // There is a pending execution request, for which we should wait
                // and should not update current_snapshot
                if let Some(prev_id) = self.get_prev_call(src_id) {
                    // The pending request is ready, we can proceed
                    self.execution_status = ExecutionStatus::Normal;

                    // We will override the current snapshot
                    let _ = self
                        .goto_snapshot(prev_id, false)
                        .and_then(|to_id| self.display_snapshot(to_id));
                }

                // Any other execution request will be rejected
                false
            }
            ExecutionStatus::WaitBreakpointHits(ref bp) => {
                // There is a pending execution request, for which we should wait
                // and should not update current_snapshot
                if self.get_breakpoint_hits(&bp.clone()).is_some() {
                    // The pending request is ready, we can proceed
                    self.execution_status = ExecutionStatus::Normal;
                }

                // Any other execution request will be rejected
                false
            }
            ExecutionStatus::Normal => true,
        }
    }

    pub fn display(&mut self, id: usize) -> Result<()> {
        self.display_snapshot(id)
    }

    /// The actual function that deals with execution
    pub fn goto(&mut self, id: usize, stop_on_breakpoint: bool) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update current_snapshot
            return Ok(());
        }

        let goto_id = self.get_sanitized_id(id);
        let _ = self
            .goto_snapshot(goto_id, stop_on_breakpoint)
            .and_then(|to_id| self.display_snapshot(to_id));

        Ok(())
    }

    pub fn step(&mut self, count: usize) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update current_snapshot
            return Ok(());
        }

        let next_id = self.get_sanitized_id(self.current_snapshot.saturating_add(count));
        self.goto(next_id, true)
    }

    pub fn next(&mut self) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update current_snapshot
            return Ok(());
        }

        let current_id = self.get_current_snapshot();
        if let Some(snapshot_info) = self.get_snapshot_info(current_id) {
            let next_id = snapshot_info.next_id();
            self.goto(next_id, true)
        } else {
            // We do nothing if we do not have the snapshot info
            Ok(())
        }
    }

    pub fn prev(&mut self) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update current_snapshot
            return Ok(());
        }

        let current_id = self.get_current_snapshot();
        if let Some(snapshot_info) = self.get_snapshot_info(current_id) {
            let prev_id = snapshot_info.prev_id();
            self.goto(prev_id, false)
        } else {
            // We do nothing if we do not have the snapshot info
            Ok(())
        }
    }

    pub fn reverse_step(&mut self, count: usize) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update current_snapshot
            return Ok(());
        }

        let prev_id = self.current_snapshot.saturating_sub(count);
        self.goto(prev_id, false)
    }

    pub fn next_call(&mut self) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update current_snapshot
            return Ok(());
        }

        if let Some(next_call) = self.get_next_call(self.current_snapshot) {
            self.goto(next_call, true)?;
        } else {
            self.execution_status = ExecutionStatus::WaitNextCall(self.current_snapshot);
        }

        Ok(())
    }

    pub fn prev_call(&mut self) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update current_snapshot
            return Ok(());
        }

        if let Some(prev_call) = self.get_prev_call(self.current_snapshot) {
            self.goto(prev_call, false)?;
        } else {
            self.execution_status = ExecutionStatus::WaitPrevCall(self.current_snapshot);
        }

        Ok(())
    }

    /////////////////////////////////////////////
    // Breakpoint management
    /////////////////////////////////////////////

    fn validate_breakpoint(&mut self, mut bp: Breakpoint) -> Option<Breakpoint> {
        if bp.loc.is_none() && bp.condition.is_none() {
            // Invalid breakpoint with no location and no condition
            return None;
        }

        if bp.loc.is_none() {
            // We do not validate condition-only breakpoints
            return Some(bp);
        }

        let loc = bp.loc.as_ref()?;
        let bytecode_address = loc.bytecode_address();
        let code = self.get_code_by_bytecode_address(bytecode_address)?;

        match (loc, code) {
            (BreakpointLocation::Opcode { pc, .. }, Code::Opcode(info)) => {
                let _ = info.codes.get(pc)?;
                Some(bp)
            }
            (
                BreakpointLocation::Source { file_path: bp_path, line_number, .. },
                Code::Source(info),
            ) => {
                // line_number is 1-based
                if *line_number == 0 {
                    // Invalid line number
                    return None;
                }

                // We allow users to simple specify the file name instead of full path
                // We will match the first file that ends with the given path
                let mut codes = HashSet::new();

                for (path, source) in &info.sources {
                    if path.ends_with(bp_path) {
                        let lines: Vec<&str> = source.lines().collect();
                        if *line_number <= lines.len() {
                            // Valid line number
                            codes.insert(path.clone());
                        }
                    }
                }

                if codes.len() == 1 {
                    bp.loc = Some(BreakpointLocation::Source {
                        bytecode_address,
                        file_path: codes.into_iter().next().unwrap(),
                        line_number: *line_number,
                    });
                    Some(bp)
                } else {
                    // Ambiguous or not found
                    None
                }
            }
            _ => None,
        }
    }

    pub fn list_breakpoints(&self) -> impl Iterator<Item = (usize, &Breakpoint, bool)> {
        self.breakpoints.iter().enumerate().map(|(i, (bp, enabled))| (i + 1, bp, *enabled))
    }

    pub fn add_breakpoint(&mut self, bp: Breakpoint) -> Result<(Breakpoint, bool)> {
        // When we try to add a breakpoint, we check whether there is a pending request
        if !self.check_pending_request() {
            // There is a pending request, we should not update breakpoints
            bail!("Cannot add breakpoint while there is a pending request");
        }

        let bp = self.validate_breakpoint(bp).ok_or_else(|| eyre::eyre!("Invalid breakpoint"))?;

        if self.breakpoint_set.insert(bp.clone()) {
            self.breakpoints.push((bp.clone(), true));
        }

        if self.get_breakpoint_hits(&bp).is_some() {
            Ok((bp, true)) // Breakpoint added or already exists
        } else {
            self.execution_status = ExecutionStatus::WaitBreakpointHits(bp.clone());
            Ok((bp, false)) // Retrieving breakpoint hits, please wait
        }
    }

    pub fn update_breakpoint_condition(
        &mut self,
        id: usize,
        expr: String,
    ) -> Result<(Breakpoint, bool)> {
        // When we try to add a breakpoint, we check whether there is a pending request
        if !self.check_pending_request() {
            // There is a pending request, we should not update breakpoints
            bail!("Cannot update breakpoint condition while there is a pending request");
        }

        if id == 0 || id > self.breakpoints.len() {
            bail!("Breakpoint id {id} out of bounds");
        }

        let mut new_bp = self.breakpoints[id - 1].0.clone();
        new_bp.set_condition(&expr);

        // We can update the breakpoint
        self.breakpoints[id - 1].0.set_condition(&expr);

        if self.get_breakpoint_hits(&new_bp).is_some() {
            Ok((new_bp, true))
        } else {
            self.execution_status = ExecutionStatus::WaitBreakpointHits(new_bp.clone());
            Ok((new_bp, false)) // Retrieving breakpoint hits, please wait
        }
    }

    pub fn enable_breakpoint(&mut self, id: usize) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update breakpoints
            bail!("Cannot enable breakpoint while there is a pending request");
        }

        if id == 0 || id > self.breakpoints.len() {
            bail!("Breakpoint id {id} out of bounds");
        }

        self.breakpoints[id - 1].1 = true;
        Ok(())
    }

    pub fn disable_breakpoint(&mut self, id: usize) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update breakpoints
            bail!("Cannot disable breakpoint while there is a pending request");
        }

        if id == 0 || id > self.breakpoints.len() {
            bail!("Breakpoint id {id} out of bounds");
        }

        self.breakpoints[id - 1].1 = false;
        Ok(())
    }

    pub fn remove_breakpoint(&mut self, id: usize) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update breakpoints
            bail!("Cannot remove breakpoint while there is a pending request");
        }

        if id == 0 || id > self.breakpoints.len() {
            bail!("Breakpoint id {id} out of bounds");
        }

        let (bp, _) = self.breakpoints.remove(id - 1);
        self.breakpoint_set.remove(&bp);
        Ok(())
    }

    pub fn clear_breakpoints(&mut self) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update breakpoints
            bail!("Cannot clear breakpoints while there is a pending request");
        }

        self.breakpoints.clear();
        self.breakpoint_set.clear();
        Ok(())
    }

    pub fn disable_all_breakpoints(&mut self) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update breakpoints
            bail!("Cannot disable breakpoints while there is a pending request");
        }

        for (_, enabled) in &mut self.breakpoints {
            *enabled = false;
        }
        Ok(())
    }

    pub fn enable_all_breakpoints(&mut self) -> Result<()> {
        if !self.check_pending_request() {
            // There is a pending request, we should not update breakpoints
            bail!("Cannot enable breakpoints while there is a pending request");
        }

        for (_, enabled) in &mut self.breakpoints {
            *enabled = true;
        }
        Ok(())
    }

    /// Find breakpoints matching the given criteria
    /// Returns a vector of breakpoint IDs (1-indexed) that match
    pub fn find_breakpoints(&self, bp: &Breakpoint, location_only: bool) -> Vec<(usize, bool)> {
        let mut matching_ids = Vec::new();

        for (idx, (existing_bp, enabled)) in self.breakpoints.iter().enumerate() {
            if location_only {
                // Only check location match, ignore condition
                if bp.loc == existing_bp.loc && bp.loc.is_some() {
                    matching_ids.push((idx + 1, *enabled)); // 1-indexed
                }
            } else {
                // Check exact match (both location and condition)
                if bp == existing_bp {
                    matching_ids.push((idx + 1, *enabled)); // 1-indexed
                }
            }
        }

        matching_ids
    }

    /// Return the breakpoints that are hit at the current snapshot
    pub fn get_hit_breakpoints(&mut self, snaphost_id: usize) -> Vec<usize> {
        let _ = self.pull_from_core();

        let enabled_breakpoints: Vec<usize> = self
            .breakpoints
            .iter()
            .enumerate()
            .filter(|(_, (_, enabled))| *enabled)
            .map(|(idx, _)| idx) // 1-indexed
            .collect();

        enabled_breakpoints
            .into_iter()
            .filter(|&idx| {
                let (bp, _) = self.breakpoints[idx].clone();
                self.get_breakpoint_hits(&bp).is_some_and(|hits| hits.contains(&snaphost_id))
            })
            .map(|idx| idx + 1) // Convert to 1-indexed
            .collect()
    }

    /// Check if there is any breakpoint hit between from (exclusive) and to (inclusive)
    fn find_breakpoint_hit_in_range(&mut self, from: usize, to: usize) -> usize {
        let _ = self.pull_from_core();

        if to == from {
            return to;
        }

        let (lb, ub, forward) = if from < to {
            (from.saturating_add(1), to, true)
        } else {
            (to, from.saturating_sub(1), false)
        };
        let lb = self.get_sanitized_id(lb);
        let ub = self.get_sanitized_id(ub);

        let enabled_bps = self
            .breakpoints
            .iter()
            .enumerate()
            .filter(|(_, (_, enabled))| *enabled)
            .map(|(idx, _)| idx)
            .collect::<Vec<usize>>();

        let inscope_bps = enabled_bps.into_iter().filter_map(|idx| {
            let (bp, _) = self.breakpoints[idx].clone();
            self.get_breakpoint_hits(&bp).and_then(|hits| {
                let inscope_hits = hits.iter().filter(|&&hit_id| hit_id >= lb && hit_id <= ub);
                if forward { inscope_hits.min().cloned() } else { inscope_hits.max().cloned() }
            })
        });

        if forward { inscope_bps.min().unwrap_or(to) } else { inscope_bps.max().unwrap_or(to) }
    }
}

impl Deref for ExecutionManager {
    type Target = ExecutionState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}
