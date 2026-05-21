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

//! Unified snapshot management for time travel debugging
//!
//! This module provides a unified interface for managing both opcode-level and hook-based
//! snapshots. It merges the two different snapshot types into a coherent structure that
//! maintains execution order and frame relationships for effective debugging.
//!
//! The main structure `Snapshots` combines:
//! - Opcode snapshots: Fine-grained instruction-level state captures
//! - Hook snapshots: Strategic breakpoint-based state captures
//!
//! The merging process prioritizes hook snapshots when available, falling back to
//! opcode snapshots for frames without hooks. This provides a comprehensive view
//! of execution state across the entire transaction.

mod analysis;
mod pretty_print;

use alloy_primitives::Address;
pub use analysis::SnapshotAnalysis;

use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use edb_common::types::ExecutionFrameId;
use revm::{Database, DatabaseCommit, DatabaseRef, database::CacheDB, state::TransientStorage};
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use crate::{HookSnapshot, HookSnapshots, OpcodeSnapshot, OpcodeSnapshots, USID};

/// Union type representing either an opcode or hook snapshot
///
/// This enum allows us to treat both types of snapshots uniformly while preserving
/// their specific characteristics. Hook snapshots are generally preferred as they
/// represent strategic breakpoints, while opcode snapshots provide fine-grained
/// instruction-level details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    id: usize,
    frame_id: ExecutionFrameId,
    next_id: Option<usize>,
    prev_id: Option<usize>,

    /// Detail of the snapshot
    detail: SnapshotDetail<DB>,
}

/// Union type representing details of either an opcode or hook snapshot
///
/// This enum allows us to treat both types of snapshots uniformly while preserving
/// their specific characteristics. Hook snapshots are generally preferred as they
/// represent strategic breakpoints, while opcode snapshots provide fine-grained
/// instruction-level details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SnapshotDetail<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Fine-grained opcode execution snapshot
    Opcode(OpcodeSnapshot<DB>),
    /// Strategic hook-based snapshot at instrumentation points
    Hook(HookSnapshot<DB>),
}

impl<DB> Snapshot<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Create an opcode snapshot
    pub fn new_opcode(id: usize, frame_id: ExecutionFrameId, detail: OpcodeSnapshot<DB>) -> Self {
        Self { id, frame_id, next_id: None, prev_id: None, detail: SnapshotDetail::Opcode(detail) }
    }

    /// Create a hook snapshot
    pub fn new_hook(id: usize, frame_id: ExecutionFrameId, detail: HookSnapshot<DB>) -> Self {
        Self { id, frame_id, next_id: None, prev_id: None, detail: SnapshotDetail::Hook(detail) }
    }

    /// Set the id of the next snapshot
    pub fn set_next_id(&mut self, id: usize) {
        self.next_id = Some(id);
    }

    /// Get the id of the next snapshot
    pub fn next_id(&self) -> Option<usize> {
        self.next_id
    }

    /// Set the id of the previous snapshot
    pub fn set_prev_id(&mut self, id: usize) {
        self.prev_id = Some(id);
    }

    /// Get the id of the previous snapshot
    pub fn prev_id(&self) -> Option<usize> {
        self.prev_id
    }

    /// Get the snapshot id
    pub fn id(&self) -> usize {
        self.id
    }

    /// Get the execution frame id
    pub fn frame_id(&self) -> ExecutionFrameId {
        self.frame_id
    }

    /// Get the detail of the snapshot
    pub fn detail(&self) -> &SnapshotDetail<DB> {
        &self.detail
    }

    /// Get the detail of the snapshot (mutable)
    pub fn detail_mut(&mut self) -> &mut SnapshotDetail<DB> {
        &mut self.detail
    }

    /// Get USID if the snapshot is hooked
    pub fn usid(&self) -> Option<USID> {
        match &self.detail {
            SnapshotDetail::Opcode(_) => None,
            SnapshotDetail::Hook(snapshot) => Some(snapshot.usid),
        }
    }

    /// Get DB
    pub fn db(&self) -> Arc<CacheDB<DB>> {
        match &self.detail {
            SnapshotDetail::Opcode(snapshot) => snapshot.database.clone(),
            SnapshotDetail::Hook(snapshot) => snapshot.database.clone(),
        }
    }

    /// Get transient storage
    pub fn transient_storage(&self) -> Arc<TransientStorage> {
        match &self.detail {
            SnapshotDetail::Opcode(snapshot) => snapshot.transient_storage.clone(),
            SnapshotDetail::Hook(snapshot) => snapshot.transient_storage.clone(),
        }
    }

    /// Get the contract address associated with this snapshot
    pub fn bytecode_address(&self) -> Address {
        match &self.detail {
            SnapshotDetail::Opcode(snapshot) => snapshot.bytecode_address,
            SnapshotDetail::Hook(snapshot) => snapshot.bytecode_address,
        }
    }

    /// Get the target address associated with this snapshot
    pub fn target_address(&self) -> Address {
        match &self.detail {
            SnapshotDetail::Opcode(snapshot) => snapshot.target_address,
            SnapshotDetail::Hook(snapshot) => snapshot.target_address,
        }
    }

    /// Check if this is a hook snapshot
    pub fn is_hook(&self) -> bool {
        matches!(self.detail, SnapshotDetail::Hook(_))
    }

    /// Check if this is an opcode snapshot  
    pub fn is_opcode(&self) -> bool {
        matches!(self.detail, SnapshotDetail::Opcode(_))
    }
}

/// Unified collection of execution snapshots organized by execution frame
///
/// This structure maintains a chronological view of all captured snapshots,
/// whether they are fine-grained opcode snapshots or strategic hook snapshots.
/// Multiple snapshots can exist within a single execution frame, representing
/// different points of interest during that frame's execution.
///
/// The collection prioritizes hook snapshots when available, as they represent
/// intentional debugging breakpoints. Opcode snapshots are included for frames
/// that lack hook coverage, ensuring comprehensive state tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Vector of (frame_id, snapshot) pairs in execution order
    inner: Vec<(ExecutionFrameId, Snapshot<DB>)>,
}

impl<DB> Deref for Snapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    type Target = [(ExecutionFrameId, Snapshot<DB>)];

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<DB> DerefMut for Snapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

// IntoIterator for owned Trace (moves out its contents)
impl<DB> IntoIterator for Snapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    type Item = (ExecutionFrameId, Snapshot<DB>);
    type IntoIter = std::vec::IntoIter<(ExecutionFrameId, Snapshot<DB>)>;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

// IntoIterator for &Trace (shared iteration)
impl<'a, DB> IntoIterator for &'a Snapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    type Item = &'a (ExecutionFrameId, Snapshot<DB>);
    type IntoIter = std::slice::Iter<'a, (ExecutionFrameId, Snapshot<DB>)>;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

// IntoIterator for &mut Trace (mutable iteration)
impl<'a, DB> IntoIterator for &'a mut Snapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    type Item = &'a mut (ExecutionFrameId, Snapshot<DB>);
    type IntoIter = std::slice::IterMut<'a, (ExecutionFrameId, Snapshot<DB>)>;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter_mut()
    }
}

impl<DB> Default for Snapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<DB> Snapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Create a new empty snapshots collection
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Merge hook snapshots and opcode snapshots into a unified collection
    ///
    /// This method combines the two snapshot types using the following strategy:
    /// 1. For frames with hook snapshots, use the hook snapshot (strategic breakpoints)
    /// 2. For frames without hook snapshots, include all opcode snapshots (fine-grained detail)
    /// 3. Maintain execution order for proper debugging flow
    ///
    /// Hook snapshots are preferred because they represent intentional instrumentation
    /// points, while opcode snapshots provide comprehensive coverage for uninstrumented code.
    pub fn merge(
        mut opcode_snapshots: OpcodeSnapshots<DB>,
        hook_snapshots: HookSnapshots<DB>,
    ) -> Self {
        let mut inner = Vec::new();

        // Process hook snapshots first (they take priority)
        for (frame_id, snapshot_opt) in hook_snapshots {
            match snapshot_opt {
                Some(snapshot) => {
                    // We have a valid hook snapshot - this takes priority over
                    // any opcode snapshots recorded for the same frame. Drop
                    // them so the post-loop sanity check only fires for frames
                    // the hook inspector never saw.
                    inner.push((frame_id, Snapshot::new_hook(inner.len(), frame_id, snapshot)));
                    opcode_snapshots.remove(&frame_id);
                }
                None => {
                    // No hook snapshot for this frame - try to use opcode snapshots
                    // It is possible to be None when a hooked step contains multiple external calls
                    if let Some(opcode_frame_snapshots) = opcode_snapshots.remove(&frame_id) {
                        // Add all opcode snapshots for this frame
                        if opcode_frame_snapshots.is_empty() && frame_id.re_entry_count() == 0 {
                            // We are going to a state variable view function generated by compiler
                            debug!(
                                "No opcode snapshots found for the initial entry frame {frame_id:?}"
                            )
                        }

                        for opcode_snapshot in opcode_frame_snapshots {
                            inner.push((
                                frame_id,
                                Snapshot::new_opcode(inner.len(), frame_id, opcode_snapshot),
                            ));
                        }
                    } else if frame_id.re_entry_count() == 0 {
                        // We are going to a state variable view function generated by compiler
                        debug!("No snapshots found for the initial entry frame {frame_id:?}");
                    }
                }
            }
        }

        // Include any remaining opcode snapshots for frames not covered by hooks
        if opcode_snapshots.values().any(|snapshots| !snapshots.is_empty()) {
            error!(
                "There are still opcode snapshots left after merging: {:?}",
                opcode_snapshots.keys().collect::<Vec<_>>()
            );
        }

        Self { inner }
    }

    /// Get all snapshots for a specific execution frame
    pub fn get_frame_snapshots(&self, frame_id: ExecutionFrameId) -> Vec<&Snapshot<DB>> {
        self.inner
            .iter()
            .filter_map(|(id, snapshot)| if *id == frame_id { Some(snapshot) } else { None })
            .collect()
    }

    /// Get all unique execution frame IDs that have snapshots
    pub fn get_frame_ids(&self) -> Vec<ExecutionFrameId> {
        let mut frame_ids: Vec<_> = self.inner.iter().map(|(id, _)| *id).collect();
        frame_ids.dedup();
        frame_ids
    }

    /// Get the total number of snapshots across all frames
    pub fn total_snapshot_count(&self) -> usize {
        self.inner.len()
    }

    /// Get the number of unique frames that have snapshots
    pub fn frame_count(&self) -> usize {
        self.get_frame_ids().len()
    }
}
