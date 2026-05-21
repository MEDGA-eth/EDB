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

use std::collections::{HashMap, HashSet};

use alloy_primitives::{Address, Bytes, keccak256};
use revm::{Database, DatabaseCommit, DatabaseRef, database::CacheDB};
use tracing::debug;

use crate::{Artifact, EngineContext, analysis::AnalysisResult};

/// Trait providing query capabilities on the EngineContext.
/// This trait allows querying various aspects of the execution context,
/// such as retrieving addresses associated with snapshots and
/// determining parent-child relationships in the execution trace.  
pub trait ContextQueryTr {
    /// Get the bytecode address for a snapshot.
    ///
    /// Returns the address where the executing bytecode is stored, which may differ
    /// from the target address in cases of delegatecall or proxy contracts.
    fn get_bytecode_address(&self, snapshot_id: usize) -> Option<Address>;

    /// Get the target address for a snapshot.
    ///
    /// Returns the address that was the target of the call, which is the address
    /// receiving the call in the current execution frame.
    fn get_target_address(&self, snapshot_id: usize) -> Option<Address>;

    /// Check if one trace entry is the parent of another.
    ///
    /// This method determines the parent-child relationship between trace entries,
    /// useful for understanding call hierarchy during debugging.
    fn is_parent_trace(&self, parent_id: usize, child_id: usize) -> bool;

    /// Get the address to code address mapping.
    ///
    /// Returns a cached mapping from target addresses to all code addresses that
    /// have been executed for each target. This is useful for understanding
    /// proxy patterns and delegatecall relationships.
    fn address_code_address_map(&self) -> &HashMap<Address, HashSet<Address>>;
}

impl<DB> ContextQueryTr for EngineContext<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    fn get_bytecode_address(&self, snapshot_id: usize) -> Option<Address> {
        let (frame_id, _) = self.snapshots.get(snapshot_id)?;
        self.trace.get(frame_id.trace_entry_id()).map(|entry| entry.code_address)
    }

    fn get_target_address(&self, snapshot_id: usize) -> Option<Address> {
        let (frame_id, _) = self.snapshots.get(snapshot_id)?;
        self.trace.get(frame_id.trace_entry_id()).map(|entry| entry.target)
    }

    fn is_parent_trace(&self, parent_id: usize, child_id: usize) -> bool {
        match self.trace.get(child_id) {
            Some(child_entry) => child_entry.parent_id == Some(parent_id),
            None => false,
        }
    }

    fn address_code_address_map(&self) -> &HashMap<Address, HashSet<Address>> {
        self.address_code_address_map.get_or_init(|| {
            let mut map: HashMap<Address, HashSet<Address>> = HashMap::new();
            for entry in &self.trace {
                map.entry(entry.target).or_default().insert(entry.code_address);
            }
            map
        })
    }
}

impl<DB> EngineContext<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    /// Look up the runtime bytecode observed at `address` during Pass 1.
    ///
    /// Walks the recorded execution trace for the first entry whose
    /// `code_address` matches and returns its captured `bytecode`. Returns
    /// `None` when no trace entry recorded code for that address (e.g. the
    /// address was never executed, or its frame was elided).
    ///
    /// The trace is the canonical Pass-3 source for "what bytes were
    /// actually executing at this address" — the same bytes the engine
    /// keccak'd when building [`EngineContext::codehash_to_canonical`] in
    /// `prepare`.
    pub(crate) fn bytecode_at(&self, address: Address) -> Option<Bytes> {
        self.trace
            .iter()
            .find(|entry| entry.code_address == address)
            .and_then(|entry| entry.bytecode.clone())
    }

    /// Try to resolve a canonical artifact address for `address` via the
    /// codehash alias index.
    ///
    /// Returns the canonical address whose recompiled instrumented runtime
    /// hashes to the same digest as the bytecode at `address`. Returns
    /// `None` when there is no recorded bytecode for `address`, the
    /// bytecode is empty, or no alias entry exists for that hash.
    ///
    /// This mirrors the fallback wired into `HookSnapshotInspector` (see
    /// `resolve_analysis`) so RPC readers stay consistent with the
    /// snapshot data the inspector produced for `vm.etch`-aliased
    /// addresses.
    pub(crate) fn resolve_canonical_via_codehash(&self, address: Address) -> Option<Address> {
        let raw_code = self.bytecode_at(address)?;
        if raw_code.is_empty() {
            return None;
        }
        let hash = keccak256(raw_code.as_ref());
        let canonical = self.codehash_to_canonical.get(&hash).copied()?;
        debug!(?address, ?canonical, ?hash, "engine context resolved address via codehash alias");
        Some(canonical)
    }

    /// Look up the analysis result for `address`, falling back to the
    /// codehash-aliased canonical address on miss.
    ///
    /// Behaves like `self.analysis_results.get(&address)` for direct hits,
    /// then for misses retries at the canonical address returned by
    /// [`EngineContext::resolve_canonical_via_codehash`]. This is the
    /// standard reader-side fallback for handlers that consume
    /// `analysis_results` keyed by a user-supplied or trace-derived
    /// address (notably the etched address recorded on a hook snapshot's
    /// `bytecode_address` after `vm.etch`).
    pub(crate) fn resolve_analysis_via_codehash(
        &self,
        address: Address,
    ) -> Option<&AnalysisResult> {
        self.analysis_results.get(&address).or_else(|| {
            self.resolve_canonical_via_codehash(address)
                .and_then(|canonical| self.analysis_results.get(&canonical))
        })
    }

    /// Look up the original artifact for `address`, falling back to the
    /// codehash-aliased canonical address on miss.
    ///
    /// Symmetric to [`EngineContext::resolve_analysis_via_codehash`] but
    /// for the `artifacts` map. Used by code/abi RPC handlers so a
    /// `vm.etch`-aliased address surfaces the canonical artifact's
    /// sources and metadata instead of an `INVALID_ADDRESS` or `null`.
    pub(crate) fn resolve_artifact_via_codehash(&self, address: Address) -> Option<&Artifact> {
        self.artifacts.get(&address).or_else(|| {
            self.resolve_canonical_via_codehash(address)
                .and_then(|canonical| self.artifacts.get(&canonical))
        })
    }

    /// Look up the recompiled (instrumented) artifact for `address`,
    /// falling back to the codehash-aliased canonical address on miss.
    ///
    /// Symmetric to [`EngineContext::resolve_artifact_via_codehash`] but
    /// for the `recompiled_artifacts` map. Backs the ABI handlers, where
    /// the instrumented artifact's contract ABI is the source of truth.
    pub(crate) fn resolve_recompiled_artifact_via_codehash(
        &self,
        address: Address,
    ) -> Option<&Artifact> {
        self.recompiled_artifacts.get(&address).or_else(|| {
            self.resolve_canonical_via_codehash(address)
                .and_then(|canonical| self.recompiled_artifacts.get(&canonical))
        })
    }
}
