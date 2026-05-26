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

//! Snapshot analysis for navigation and call stack tracking.
//!
//! This module provides sophisticated analysis capabilities for snapshots to enable
//! efficient debugging navigation. It analyzes execution flows, call hierarchies,
//! and snapshot relationships to support step-over, step-into, and step-out operations.
//!
//! # Core Analysis Features
//!
//! ## Next/Previous Step Analysis
//! - **Step Navigation**: Determines next/previous snapshots for debugging navigation
//! - **Call Stack Tracking**: Maintains function call hierarchy across snapshots
//! - **Return Analysis**: Identifies function returns and modifier exits
//! - **Hole Detection**: Handles gaps in snapshot coverage for complex call patterns
//!
//! ## Source-Level Analysis
//! The analysis engine provides specialized handling for source-level (hook-based) snapshots:
//! - **Function Entry/Exit Tracking**: Identifies function and modifier boundaries
//! - **Internal Call Detection**: Recognizes internal function calls and library calls
//! - **Call Stack Management**: Maintains accurate call stack state across snapshots
//! - **Return Chain Analysis**: Handles complex return patterns through nested calls
//!
//! ## Opcode-Level Analysis
//! For opcode snapshots, the analysis provides:
//! - **Sequential Linking**: Links consecutive opcode snapshots within execution frames
//! - **Frame Boundary Detection**: Identifies transitions between execution frames
//! - **Fallback Coverage**: Ensures complete navigation even without source-level analysis
//!
//! # Navigation Support
//!
//! The analysis results enable sophisticated debugging operations:
//! - **Step Over**: Navigate to the next snapshot in the same scope
//! - **Step Into**: Enter function calls when available
//! - **Step Out**: Exit current function to parent scope
//! - **Reverse Navigation**: Support for backwards debugging through previous snapshots

use std::collections::{HashMap, HashSet};

use alloy_primitives::{Address, B256};
use edb_common::types::{ExecutionFrameId, Trace};
use eyre::Result;
use itertools::Itertools;
use revm::{Database, DatabaseCommit, DatabaseRef, database::CacheDB};
use tracing::{debug, error, warn};

use crate::{
    Snapshot, Snapshots,
    analysis::{AnalysisResult, StepRef, UFID},
};

/// Trait for analyzing snapshots to enable debugging navigation.
///
/// This trait provides the interface for analyzing snapshot collections to determine
/// navigation relationships, call stack hierarchies, and execution flow patterns.
pub trait SnapshotAnalysis {
    /// Analyze snapshots using execution trace and source code analysis results.
    ///
    /// This method processes the snapshot collection to establish navigation relationships,
    /// call stack hierarchies, and execution flow patterns based on the execution trace
    /// and source code analysis results.
    ///
    /// `codehash_to_canonical` is the index built during engine prepare that maps the
    /// keccak hash of deployed bytecode back to the canonical address known to
    /// `analysis`. It is consulted as a fallback when the trace entry's
    /// `code_address` is a `vm.etch`-aliased address with no direct entry in
    /// `analysis`.
    fn analyze(
        &mut self,
        trace: &Trace,
        analysis: &HashMap<Address, AnalysisResult>,
        codehash_to_canonical: &HashMap<B256, Address>,
    ) -> Result<()>;
}

impl<DB> SnapshotAnalysis for Snapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn analyze(
        &mut self,
        trace: &Trace,
        analysis: &HashMap<Address, AnalysisResult>,
        codehash_to_canonical: &HashMap<B256, Address>,
    ) -> Result<()> {
        self.analyze_next_steps(trace, analysis, codehash_to_canonical)
    }
}

// Implementation of snapshot analysis for next/previous step navigation
impl<DB> Snapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Analyze next step relationships for debugging navigation.
    ///
    /// This method processes all snapshots to determine their next/previous relationships,
    /// handling both opcode-level and source-level analysis with call stack tracking.
    fn analyze_next_steps(
        &mut self,
        trace: &Trace,
        analysis: &HashMap<Address, AnalysisResult>,
        codehash_to_canonical: &HashMap<B256, Address>,
    ) -> Result<()> {
        let mut holed_snapshots: HashSet<usize> = HashSet::new();

        for (entry_id, snapshots) in &self
            .iter_mut()
            .sorted_by_key(|(f_id, _)| f_id.trace_entry_id())
            .chunk_by(|(f_id, _)| f_id.trace_entry_id())
        {
            let snapshots_vec: Vec<_> = snapshots.collect();

            // The last snapshot of a entry will become holed
            let Some((_, last_snapshot)) = snapshots_vec.last() else {
                continue;
            };
            holed_snapshots.insert(last_snapshot.id());

            if last_snapshot.is_opcode() {
                Self::analyze_next_steps_for_opcode(snapshots_vec)?;
            } else {
                let trace_entry = trace.get(entry_id).ok_or_else(|| {
                    eyre::eyre!("Trace entry {} not found for snapshot analysis", entry_id)
                })?;
                let bytecode_address = trace_entry.code_address;

                // Direct hit, then codehash fallback for vm.etch-aliased contracts:
                // the snapshot's `code_address` may be an etched address with no
                // direct entry in `analysis`. Resolve the trace entry's bytecode
                // to its canonical address through `codehash_to_canonical` (which
                // the engine populates with the instrumented Pass-3 runtime hash,
                // the original Pass-1 runtime hash, and a trailing-zero-stripped
                // hash for every canonical artifact — see `core.rs`).
                let analysis_result = analysis
                    .get(&bytecode_address)
                    .or_else(|| {
                        let bytecode = trace_entry.bytecode.as_ref()?;
                        if bytecode.is_empty() {
                            return None;
                        }
                        crate::utils::resolve_canonical(codehash_to_canonical, bytecode.as_ref())
                            .and_then(|c| analysis.get(&c))
                    })
                    .ok_or_else(|| {
                        eyre::eyre!(
                            "Analysis result not found for bytecode address {} (direct or via codehash alias)",
                            bytecode_address
                        )
                    })?;
                Self::analyze_next_steps_for_source(
                    snapshots_vec,
                    &mut holed_snapshots,
                    analysis_result,
                )?;
            }
        }

        // Handle holed snapshots
        self.find_next_step_for_holed_snapshots(trace, holed_snapshots)?;
        self.analyze_prev_steps()?;

        Ok(())
    }

    /// Analyze previous step relationships for reverse navigation.
    ///
    /// This method establishes the previous step links for all snapshots,
    /// enabling backwards debugging navigation through the execution history.
    fn analyze_prev_steps(&mut self) -> Result<()> {
        for i in 0..self.len() {
            let current_snapshot = &self[i].1;
            let current_id = current_snapshot.id();
            let next_id = current_snapshot
                .next_id()
                .ok_or_else(|| eyre::eyre!("Snapshot {} does not have next_id set", current_id))?;

            let next_snapshot = &mut self[next_id].1;
            if next_snapshot.prev_id().is_none() {
                // The first snapshot whose next_id is the subject one, will be the prev_id
                // of the current snapshot
                next_snapshot.set_prev_id(current_id);
            }
        }

        self.iter_mut().filter(|(_, snapshot)| snapshot.prev_id().is_none()).for_each(
            |(_, snapshot)| {
                snapshot.set_prev_id(snapshot.id().saturating_sub(1));
            },
        );

        Ok(())
    }

    /// Find next steps for holed snapshots that don't have direct successors.
    ///
    /// This method handles snapshots that represent the end of execution frames
    /// by finding their next steps in ancestor frames, ensuring complete navigation
    /// coverage even for complex call patterns.
    fn find_next_step_for_holed_snapshots(
        &mut self,
        trace: &Trace,
        holed_snapshots: HashSet<usize>,
    ) -> Result<()> {
        let last_snapshot_id = self.len().saturating_sub(1);

        // Handle holed snapshots
        for current_id in holed_snapshots {
            let entry_id = self[current_id].0.trace_entry_id();

            // Try to find the first snapshot in the ancestor frames that is after the current
            // snapshot
            let mut next_id = None;
            let mut entry = trace
                .get(entry_id)
                .ok_or_else(|| eyre::eyre!("Trace entry {} not found", entry_id))?;
            while let Some(parent_id) = entry.parent_id {
                // Find the first snapshot in the parent frame that is after the current snapshot
                if let Some((_, parent_snapshot)) = self
                    .iter()
                    .skip(current_id.saturating_add(1))
                    .find(|(f_id, _)| f_id.trace_entry_id() == parent_id)
                {
                    next_id = Some(parent_snapshot.id());
                    break;
                }

                // Move up to the next parent
                entry = trace
                    .get(parent_id)
                    .ok_or_else(|| eyre::eyre!("Trace entry {} not found", parent_id))?;
            }

            self[current_id].1.set_next_id(next_id.unwrap_or(last_snapshot_id));
        }

        Ok(())
    }

    /// Analyze next steps for opcode-level snapshots.
    ///
    /// For opcode snapshots, the analysis is straightforward: link each snapshot
    /// to the next one in the same execution frame, providing sequential navigation.
    fn analyze_next_steps_for_opcode(
        mut snapshots: Vec<&mut (ExecutionFrameId, Snapshot<DB>)>,
    ) -> Result<()> {
        for i in 0..snapshots.len().saturating_sub(1) {
            let (_, next_snapshot) = &snapshots[i + 1];
            let next_id = next_snapshot.id();

            // Link to the next snapshot in the same frame
            let (_, current_snapshot) = &mut snapshots[i];
            current_snapshot.set_next_id(next_id);
        }

        Ok(())
    }

    /// Analyze next steps for source-level (hook-based) snapshots.
    ///
    /// This is the most complex part of snapshot analysis, handling function calls,
    /// returns, modifier executions, and call stack management to provide accurate
    /// navigation for source-level debugging.
    fn analyze_next_steps_for_source(
        mut snapshots: Vec<&mut (ExecutionFrameId, Snapshot<DB>)>,
        holed_snapshots: &mut HashSet<usize>,
        analysis: &AnalysisResult,
    ) -> Result<()> {
        if snapshots.len() <= 1 {
            return Ok(());
        }

        let mut stack: Vec<CallStackEntry> = Vec::new();
        stack.push(CallStackEntry {
            func_info: FunctionInfo::Unknown,
            callsite: None,
            return_after_callsite: false,
        });

        let is_entry =
            |step: &StepRef| step.function_entry().is_some() || step.modifier_entry().is_some();

        for i in 0..snapshots.len().saturating_sub(1) {
            let usid = snapshots[i]
                .1
                .usid()
                .ok_or_else(|| eyre::eyre!("Snapshot {} does not have usid set", i))?;
            let step = analysis
                .usid_to_step
                .get(&usid)
                .ok_or_else(|| eyre::eyre!("No step found for USID {}", u64::from(usid)))?;
            let ufid = step.ufid();
            let contract = analysis.ufid_to_function.get(&ufid).and_then(|f| f.contract());

            let next_id = snapshots[i + 1].1.id();
            let next_usid = snapshots[i + 1]
                .1
                .usid()
                .ok_or_else(|| eyre::eyre!("Snapshot {} does not have usid set", i + 1))?;
            let next_step = analysis
                .usid_to_step
                .get(&next_usid)
                .ok_or_else(|| eyre::eyre!("No step found for USID {}", u64::from(next_usid)))?;
            let next_ufid = next_step.ufid();
            let next_contract =
                analysis.ufid_to_function.get(&next_ufid).and_then(|f| f.contract());

            // Step 0: To avoid annoying crash, we always add a placeholder stack entry
            if stack.is_empty() {
                warn!("Call stack is empty at Snapshot {} (step 0)", snapshots[i].1.id());
                stack.push(CallStackEntry {
                    func_info: FunctionInfo::Unknown,
                    callsite: None,
                    return_after_callsite: false,
                });
            }

            // Step 1: try to update the current function entry
            let stack_entry = stack.last_mut().ok_or_else(|| {
                eyre::eyre!("Call stack is empty at step 1 (which is impossible)")
            })?;
            if let Some(ufid) = step.function_entry() {
                stack_entry.func_info.with_function(ufid);
            }
            if let Some(ufid) = step.modifier_entry() {
                stack_entry.func_info.with_modifier(ufid);
            }
            if !stack_entry.func_info.is_valid() {
                // We output the error but do not stop here.
                error!("Invalid function info in call stack");
            }

            // Step 2: check whether this step contains any valid internal call
            //  a) The step contains internal calls
            if step.function_calls()
                > snapshots[i + 1].0.re_entry_count() - snapshots[i].0.re_entry_count()
                && is_entry(next_step)
            {
                // This step contains at least one valid internal call
                stack.push(CallStackEntry {
                    func_info: FunctionInfo::Unknown,
                    callsite: Some(Callsite {
                        id: i,
                        callees: step.function_calls()
                            - (snapshots[i + 1].0.re_entry_count()
                                - snapshots[i].0.re_entry_count()),
                    }),
                    return_after_callsite: step.contains_return(),
                });
                continue;
            }
            // b) There is overrided operation (e.g., `+`)
            if is_entry(next_step) && contract.is_some() && next_contract.is_none() {
                // This is likely an internal call to a library function
                debug!(
                    "Assuming an internal call to a library function at Snapshot {}",
                    snapshots[i].1.id()
                );
                stack.push(CallStackEntry {
                    func_info: FunctionInfo::Unknown,
                    callsite: Some(Callsite { id: i, callees: usize::MAX }),
                    return_after_callsite: step.contains_return(),
                });
                continue;
            }

            // Step 3: update next id since we are certain for steps without internal calls
            snapshots[i].1.set_next_id(next_id);

            // Step 4: check return
            let Some(stack_entry) = stack.last() else {
                warn!("Call stack is empty at Snapshot {} (step 4)", snapshots[i].1.id());
                continue;
            };

            let will_return = match &stack_entry.func_info {
                FunctionInfo::FunctionOnly(..) | FunctionInfo::ModifiedFunction { .. } => {
                    !stack_entry.func_info.contains_ufid(next_ufid) || is_entry(next_step)
                }
                FunctionInfo::ModifierOnly(..) => {
                    !(stack_entry.func_info.contains_ufid(next_ufid) || is_entry(next_step))
                }
                _ => !is_entry(next_step),
            } || step.contains_return();
            if !will_return {
                // There is nothing we need to do if this step will not return
                continue;
            }

            // Step 5: handle returning chain
            loop {
                let Some(mut stack_entry) = stack.pop() else {
                    warn!("Call stack is empty at Snapshot {} (step 5)", snapshots[i].1.id());
                    break;
                };

                if stack_entry.callsite.is_none() {
                    // We have returned from the top level, nothing more to do
                    break;
                }

                // We have finished one call
                let callsite = stack_entry.callsite.as_mut().unwrap();
                callsite.callees = callsite.callees.saturating_sub(1);

                let Some(parent_entry) = stack.last() else {
                    // We have returned from the top level, nothing more to do
                    break;
                };

                // Check whether we are done with this callsite (callsite_certainly_done has higher
                // priority)
                let callsite_certainly_done =
                    parent_entry.func_info.contains_ufid(next_ufid) && !is_entry(next_step);
                let callsite_certainly_not_done = next_contract.is_none(); // We are still in free functions
                if (callsite.callees > 0 || callsite_certainly_not_done) && !callsite_certainly_done
                {
                    stack_entry.func_info = FunctionInfo::Unknown;
                    stack.push(stack_entry);
                    break;
                }

                let continue_to_return = match &parent_entry.func_info {
                    FunctionInfo::FunctionOnly(..) | FunctionInfo::ModifiedFunction { .. } => {
                        !parent_entry.func_info.contains_ufid(next_ufid) || is_entry(next_step)
                    }
                    FunctionInfo::ModifierOnly(..) => {
                        !(parent_entry.func_info.contains_ufid(next_ufid) || is_entry(next_step))
                    }
                    _ => !is_entry(next_step),
                } || stack_entry.return_after_callsite;

                // We can confidently update the snapshot
                snapshots[callsite.id].1.set_next_id(next_id);

                if !continue_to_return {
                    break;
                }
            }
        }

        while let Some(CallStackEntry { callsite: Some(Callsite { id, .. }), .. }) = stack.pop() {
            debug!("Add snapshot as a hole: {}", snapshots[id].1.id());
            holed_snapshots.insert(snapshots[id].1.id());
        }

        Ok(())
    }
}

#[derive(Debug)]
struct CallStackEntry {
    func_info: FunctionInfo,
    callsite: Option<Callsite>,
    return_after_callsite: bool,
}

#[derive(Debug)]
struct Callsite {
    // The id in the snapshot
    // NOTE: it is not snapshot id
    id: usize,
    // Number of callees that we haven't visited
    callees: usize,
}

#[derive(Debug)]
enum FunctionInfo {
    Unknown,
    ModifierOnly(Vec<UFID>),
    FunctionOnly(UFID),
    ModifiedFunction { func: UFID, modifiers: Vec<UFID> },
    Invalid,
}

impl FunctionInfo {
    fn contains_ufid(&self, ufid: UFID) -> bool {
        match self {
            Self::Unknown => false,
            Self::ModifierOnly(ids) => ids.contains(&ufid),
            Self::FunctionOnly(id) => *id == ufid,
            Self::ModifiedFunction { func, modifiers } => {
                *func == ufid || modifiers.contains(&ufid)
            }
            Self::Invalid => false,
        }
    }

    fn is_valid(&self) -> bool {
        !matches!(self, Self::Invalid)
    }

    fn _certainly_in_body(&self) -> bool {
        matches!(self, Self::FunctionOnly(..) | Self::ModifiedFunction { .. })
    }

    fn with_modifier(&mut self, modifier: UFID) {
        match self {
            Self::Unknown => {
                *self = Self::ModifierOnly(vec![modifier]);
            }
            Self::ModifierOnly(ids) => {
                ids.push(modifier);
            }
            Self::FunctionOnly(func) => {
                *self = Self::ModifiedFunction { func: *func, modifiers: vec![modifier] };
            }
            Self::ModifiedFunction { modifiers, .. } => {
                modifiers.push(modifier);
            }
            Self::Invalid => {}
        }
    }

    fn with_function(&mut self, function: UFID) {
        match self {
            Self::Unknown => *self = Self::FunctionOnly(function),
            Self::ModifierOnly(ids) => {
                *self = Self::ModifiedFunction { func: function, modifiers: ids.clone() }
            }
            Self::FunctionOnly(..) => *self = Self::Invalid,
            Self::ModifiedFunction { .. } => *self = Self::Invalid,
            Self::Invalid => {}
        }
    }
}
