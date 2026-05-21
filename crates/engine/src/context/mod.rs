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

//! Engine context management and EVM instantiation for debugging.
//!
//! This module provides the core [`EngineContext`] struct that consolidates all debugging
//! data and state needed for time travel debugging and expression evaluation. The context
//! serves as the primary data structure passed to the JSON-RPC server and contains all
//! analysis results, snapshots, artifacts, and execution state.
//!
//! # Core Components
//!
//! ## EngineContext
//! The [`EngineContext`] is the central data structure that encapsulates:
//! - **Fork Information**: Network and block context for the debugging session
//! - **EVM Environment**: Configuration, block, and transaction environments
//! - **Snapshots**: Merged opcode-level and hook-based execution snapshots
//! - **Artifacts**: Original and recompiled contract artifacts with source code
//! - **Analysis Results**: Instrumentation points and debugging metadata
//! - **Execution Trace**: Call hierarchy and frame structure
//!
//! ## EVM Instantiation
//! The context provides methods to create derived EVMs for expression evaluation:
//! - **Snapshot-specific EVMs**: Create EVM instances at any execution point
//! - **Transaction replay**: Send mock transactions in derived state
//! - **Function calls**: Invoke contract functions for expression evaluation
//!
//! # Workflow Integration
//!
//! 1. **Context Building**: Constructed after analysis and snapshot collection
//! 2. **Finalization**: Processes traces and pre-evaluates state variables
//! 3. **Server Integration**: Passed to JSON-RPC server for debugging API
//! 4. **Expression Evaluation**: Used to create derived EVMs for real-time evaluation
//!
//! # Thread Safety
//!
//! The [`EngineContext`] is designed to be thread-safe and can be shared across
//! multiple debugging clients through Arc wrapping. All database operations
//! use read-only snapshots to ensure debugging session integrity.

mod evm;
pub use evm::*;

mod query;
pub use query::*;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use alloy_primitives::{Address, B256, Bytes, TxHash, keccak256};
use edb_common::{
    ForkInfo,
    types::{Trace, parse_callable_abi_entries},
};
use eyre::{Result, eyre};
use indicatif::ProgressBar;
use once_cell::sync::OnceCell;
use revm::{
    Database, DatabaseCommit, DatabaseRef,
    context::{BlockEnv, CfgEnv, TxEnv},
    database::CacheDB,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

use crate::{Artifact, SnapshotDetail, Snapshots, analysis::AnalysisResult};

/// Complete debugging context containing all analysis results and state snapshots
///
/// This struct encapsulates all the data produced during the debugging workflow,
/// including the original transaction context, collected snapshots, analyzed source code,
/// and recompiled artifacts. It serves as the primary data structure passed to the
/// JSON-RPC server for time travel debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineContext<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    /// Forked information
    pub fork_info: ForkInfo,
    /// Configuration environment for the EVM
    pub cfg: CfgEnv,
    /// Block environment for the target block
    pub block: BlockEnv,
    /// Transaction environment for the target transaction
    pub tx: TxEnv,
    /// Transaction hash for the target transaction
    pub tx_hash: TxHash,
    /// Merged snapshots from both opcode-level and hook-based collection
    pub snapshots: Snapshots<DB>,
    /// Original contract artifacts with source code and metadata
    pub artifacts: HashMap<Address, Artifact>,
    /// Recompiled artifacts with instrumented source code
    pub recompiled_artifacts: HashMap<Address, Artifact>,
    /// Analysis results identifying instrumentation points
    pub analysis_results: HashMap<Address, AnalysisResult>,
    /// Codehash → canonical address index. Built once at engine-prepare time
    /// from **two** sources so the same map serves every reader:
    ///   * the keccak of each `recompiled_artifacts` entry's *instrumented*
    ///     (Pass 3) runtime bytecode — what the hook inspector sees live in
    ///     `interp.bytecode`, and
    ///   * the keccak of each `artifacts` entry's *original* (Pass 1) runtime
    ///     bytecode — what the call tracer captured into `trace.bytecode`
    ///     before instrumentation.
    ///
    /// Used by readers (hook inspector, RPC handlers, prepare-time snapshot
    /// analysis) to resolve an unknown address to a known artifact when the
    /// bytecode at the unknown address matches a known artifact's runtime
    /// (typically the result of `vm.etch(target, address(impl).code)`).
    ///
    /// First-walk wins on collision. If two addresses legitimately share the
    /// same runtime (instrumented or original), either canonical address
    /// gives a correct analysis lookup since analysis is source-driven
    /// (USID-keyed) and independent of the deploy address.
    pub codehash_to_canonical: HashMap<B256, Address>,
    /// Execution trace showing call hierarchy and frame structure
    pub trace: Trace,
    /// Relation between target addresses and their (delegated) code addresses
    #[serde(skip)]
    address_code_address_map: OnceCell<HashMap<Address, HashSet<Address>>>,
}

impl<DB> EngineContext<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    /// Build a new EngineContext with all debugging data.
    ///
    /// This constructor consolidates all the debugging data collected during the analysis
    /// and snapshot collection phases into a unified context for debugging operations.
    ///
    /// # Arguments
    ///
    /// * `fork_info` - Network and fork information for the debugging session
    /// * `cfg` - EVM configuration environment
    /// * `block` - Block environment for the target block
    /// * `tx` - Transaction environment for the target transaction
    /// * `tx_hash` - Hash of the target transaction
    /// * `snapshots` - Merged snapshots from opcode and hook collection
    /// * `artifacts` - Original contract artifacts with source code
    /// * `recompiled_artifacts` - Recompiled artifacts with instrumentation
    /// * `analysis_results` - Analysis results identifying instrumentation points
    /// * `codehash_to_canonical` - Codehash → canonical address index built from recompiled artifacts
    /// * `trace` - Execution trace showing call hierarchy
    ///
    /// # Returns
    ///
    /// Returns a finalized [`EngineContext`] ready for debugging operations.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        fork_info: ForkInfo,
        cfg: CfgEnv,
        block: BlockEnv,
        tx: TxEnv,
        tx_hash: TxHash,
        snapshots: Snapshots<DB>,
        artifacts: HashMap<Address, Artifact>,
        recompiled_artifacts: HashMap<Address, Artifact>,
        analysis_results: HashMap<Address, AnalysisResult>,
        codehash_to_canonical: HashMap<B256, Address>,
        trace: Trace,
    ) -> Result<Self> {
        let mut context = Self {
            fork_info,
            cfg,
            block,
            tx,
            tx_hash,
            snapshots,
            artifacts,
            recompiled_artifacts,
            analysis_results,
            codehash_to_canonical,
            trace,
            address_code_address_map: OnceCell::new(),
        };

        // Finalize the context to populate derived fields
        context.finalize()?;
        Ok(context)
    }

    /// Finalize the EngineContext by processing traces and snapshots.
    ///
    /// This method performs post-processing on the collected debugging data:
    /// 1. Links trace entries with their corresponding snapshot IDs
    /// 2. Pre-evaluates state variables for all hook-based snapshots
    /// 3. Populates derived mappings for efficient lookups
    fn finalize(&mut self) -> Result<()> {
        self.finalize_trace()?;
        self.finalize_snapshots()?;

        Ok(())
    }

    /// Finalize the trace by linking trace entries with their corresponding snapshot IDs.
    ///
    /// This method processes the execution trace to establish the relationship between
    /// trace entries and snapshots, enabling efficient navigation during debugging.
    fn finalize_trace(&mut self) -> Result<()> {
        for entry in &mut self.trace {
            let trace_id = entry.id;

            // We first update the snapshot id
            for (snapshot_id, (frame_id, _)) in self.snapshots.iter().enumerate() {
                if frame_id.trace_entry_id() == trace_id {
                    entry.first_snapshot_id = Some(snapshot_id);
                    break;
                }
            }
        }

        for entry in &self.trace {
            if entry.first_snapshot_id.is_none() {
                debug!("Trace entry {} has no associated snapshot", entry.id);
            }
        }

        Ok(())
    }

    /// Finalize snapshots by pre-evaluating state variables.
    ///
    /// This method processes all hook-based snapshots to pre-evaluate their state
    /// variables, making them immediately available for expression evaluation.
    /// This optimization reduces latency during debugging sessions.
    fn finalize_snapshots(&mut self) -> Result<()> {
        let tx_hash = self.tx_hash;

        // Actually execute each transaction with revm
        let console_bar = Arc::new(ProgressBar::new(self.snapshots.len() as u64));
        let template = format!(
            "{{spinner:.green}} 🔮 Finalizing steps for {} [{{bar:40.cyan/blue}}] {{pos:>3}}/{{len:3}} ⛽ {{msg}}",
            &tx_hash.to_string()[2..10]
        );
        console_bar.set_style(
            indicatif::ProgressStyle::with_template(&template)?
                .progress_chars("🟩🟦⬜")
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
        );

        let mut results: HashMap<usize, _> = HashMap::new();
        for (snapshot_id, (_, snapshot)) in self.snapshots.iter().enumerate() {
            if snapshot.is_opcode() {
                console_bar.set_message(format!("Analyzing step {snapshot_id} w/o source code"));
                console_bar.inc(1);
                continue;
            } else {
                console_bar.set_message(format!("Analyzing step {snapshot_id} with source code"));
                console_bar.inc(1);
            }

            let code_address = snapshot.bytecode_address();
            // Direct hit, then codehash fallback for vm.etch-aliased contracts:
            // the snapshot's `bytecode_address` may be an etched address with no
            // direct entry in `recompiled_artifacts`. Reuse the standard
            // alias-aware resolver so we land on the canonical artifact.
            let Some(contract) = self
                .resolve_recompiled_artifact_via_codehash(code_address)
                .and_then(|a| a.contract())
            else {
                return Err(eyre!("No contract found for address {}", code_address));
            };

            let mut states = HashMap::new();
            for state_variable in parse_callable_abi_entries(contract)
                .into_iter()
                .filter(|v| v.is_state_variable() && v.inputs.is_empty())
            {
                match self.call_in_derived_evm(
                    snapshot_id,
                    snapshot.target_address(),
                    &state_variable.abi,
                    &[],
                    None,
                ) {
                    Ok(value) => {
                        states.insert(state_variable.name.clone(), Some(Arc::new(value.into())));
                    }
                    Err(e) => {
                        error!(id=?snapshot_id, "Failed to call state variable: {} ({})", state_variable, e);
                        states.insert(state_variable.name.clone(), None);
                    }
                }
            }

            results.insert(snapshot_id, states);
        }

        for (snapshot_id, states) in results.into_iter() {
            if let Some((_, snapshot)) = self.snapshots.get_mut(snapshot_id)
                && let SnapshotDetail::Hook(hook_detail) = snapshot.detail_mut()
            {
                hook_detail.state_variables = states;
            }
        }

        console_bar.finish_with_message(format!(
            "✨ Ready! All {} steps analyzed and finalized.",
            self.snapshots.len()
        ));

        Ok(())
    }

    /// Look up the runtime bytecode observed at `address` during Pass 1.
    ///
    /// Walks the recorded execution trace for the first entry whose
    /// `code_address` matches and returns its captured `bytecode`. Returns
    /// `None` when no trace entry recorded code for that address (e.g. the
    /// address was never executed, or its frame was elided).
    ///
    /// The trace is the canonical Pass-3 source for "what bytes were
    /// actually executing at this address" — the same bytes the engine
    /// keccak'd when building [`Self::codehash_to_canonical`] in `prepare`.
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
    /// [`Self::resolve_canonical_via_codehash`]. This is the standard
    /// reader-side fallback for handlers that consume
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
    /// Symmetric to [`Self::resolve_analysis_via_codehash`] but for the
    /// `artifacts` map. Used by code/abi RPC handlers so a `vm.etch`-aliased
    /// address surfaces the canonical artifact's sources and metadata
    /// instead of an `INVALID_ADDRESS` or `null`.
    pub(crate) fn resolve_artifact_via_codehash(&self, address: Address) -> Option<&Artifact> {
        self.artifacts.get(&address).or_else(|| {
            self.resolve_canonical_via_codehash(address)
                .and_then(|canonical| self.artifacts.get(&canonical))
        })
    }

    /// Look up the recompiled (instrumented) artifact for `address`,
    /// falling back to the codehash-aliased canonical address on miss.
    ///
    /// Symmetric to [`Self::resolve_artifact_via_codehash`] but for the
    /// `recompiled_artifacts` map. Backs the ABI handlers, where the
    /// instrumented artifact's contract ABI is the source of truth.
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

#[cfg(test)]
mod codehash_resolver_tests {
    //! Unit tests for the codehash-aliased canonical-address resolver
    //! exposed on [`EngineContext`].
    //!
    //! These tests exercise [`EngineContext::resolve_canonical_via_codehash`]
    //! and its standard wrapper [`EngineContext::resolve_analysis_via_codehash`]
    //! against a hand-built [`EngineContext`] populated with just enough trace
    //! and index data to drive the fallback. They don't go through
    //! `EngineContext::build`, which would also run trace/snapshot
    //! finalization that isn't relevant here.
    use super::*;
    use alloy_primitives::{Address, B256, Bytes, address, keccak256};
    use edb_common::{
        ForkInfo,
        types::{CallType, TraceEntry},
    };
    use revm::{
        context::{BlockEnv, CfgEnv, TxEnv},
        database::CacheDB,
        database_interface::EmptyDB,
        interpreter::CallScheme,
        primitives::{TxKind, U256, hardfork::SpecId},
    };
    use std::collections::HashMap;

    use crate::analysis::AnalysisResult;
    use crate::snapshot::Snapshots;

    type TestDB = CacheDB<EmptyDB>;

    /// Build a single-entry [`Trace`] whose only `TraceEntry` records the
    /// supplied `(code_address, bytecode)` pair. The other fields are filled
    /// with deterministic placeholders sufficient for `bytecode_at` to find
    /// the entry by `code_address`.
    fn trace_with_entry(code_address: Address, bytecode: Option<Bytes>) -> Trace {
        let mut trace = Trace::default();
        trace.push(TraceEntry {
            id: 0,
            parent_id: None,
            depth: 0,
            call_type: CallType::Call(CallScheme::Call),
            caller: Address::ZERO,
            target: code_address,
            code_address,
            input: Bytes::new(),
            value: U256::ZERO,
            result: None,
            created_contract: false,
            create_scheme: None,
            bytecode,
            target_label: None,
            self_destruct: None,
            events: vec![],
            first_snapshot_id: None,
        });
        trace
    }

    /// Build an [`EngineContext`] with the supplied codehash index and trace,
    /// and otherwise-empty/default fields. This intentionally bypasses
    /// [`EngineContext::build`] (and therefore `finalize`) — those paths do
    /// EVM-side work that is irrelevant to the codehash-resolver logic under
    /// test here.
    fn make_context(
        codehash_to_canonical: HashMap<B256, Address>,
        trace: Trace,
        analysis_results: HashMap<Address, AnalysisResult>,
    ) -> EngineContext<TestDB> {
        EngineContext::<TestDB> {
            fork_info: ForkInfo {
                block_number: 0,
                block_hash: B256::ZERO,
                timestamp: 0,
                chain_id: 1,
                spec_id: SpecId::default(),
            },
            cfg: CfgEnv::default(),
            block: BlockEnv::default(),
            tx: TxEnv {
                tx_type: 0,
                caller: Address::ZERO,
                gas_limit: 0,
                gas_price: 0,
                kind: TxKind::Call(Address::ZERO),
                value: U256::ZERO,
                data: Bytes::new(),
                nonce: 0,
                chain_id: Some(1),
                access_list: Default::default(),
                gas_priority_fee: None,
                blob_hashes: Vec::new(),
                max_fee_per_blob_gas: 0,
                authorization_list: Vec::new(),
            },
            tx_hash: B256::ZERO,
            snapshots: Snapshots::<TestDB>::default(),
            artifacts: HashMap::new(),
            recompiled_artifacts: HashMap::new(),
            analysis_results,
            codehash_to_canonical,
            trace,
            address_code_address_map: OnceCell::new(),
        }
    }

    #[test]
    fn resolve_canonical_via_codehash_resolves_etched_address() {
        let canonical_addr = address!("0000000000000000000000000000000000000A1A");
        let etched_addr = address!("6342000000000000000000000000000000000006");
        let runtime: &[u8] = &[0xfe, 0xed, 0xbe, 0xef, 0x60, 0x80];
        let runtime_hash = keccak256(runtime);

        let mut codehash_to_canonical = HashMap::new();
        codehash_to_canonical.insert(runtime_hash, canonical_addr);
        let trace = trace_with_entry(etched_addr, Some(Bytes::copy_from_slice(runtime)));
        let context = make_context(codehash_to_canonical, trace, HashMap::new());

        assert_eq!(
            context.resolve_canonical_via_codehash(etched_addr),
            Some(canonical_addr),
            "fallback should resolve etched_addr via codehash"
        );
    }

    #[test]
    fn resolve_canonical_via_codehash_returns_none_on_unknown_codehash() {
        let etched_addr = address!("6342000000000000000000000000000000000006");
        let unknown_runtime: &[u8] = &[0x00, 0x01, 0x02];

        // codehash_to_canonical is intentionally empty — the etched address's
        // bytecode hash has no entry, so the lookup must miss.
        let trace = trace_with_entry(etched_addr, Some(Bytes::copy_from_slice(unknown_runtime)));
        let context = make_context(HashMap::new(), trace, HashMap::new());

        assert_eq!(context.resolve_canonical_via_codehash(etched_addr), None);
    }

    #[test]
    fn resolve_canonical_via_codehash_returns_none_on_empty_bytecode() {
        let eoa_addr = address!("0000000000000000000000000000000000001111");

        // Seed the index with a real entry so a mismatch on _bytecode_ (not on
        // index emptiness) is what causes the None result.
        let other_hash = keccak256(b"some other bytecode");
        let mut codehash_to_canonical = HashMap::new();
        codehash_to_canonical
            .insert(other_hash, address!("0000000000000000000000000000000000002222"));

        let trace = trace_with_entry(eoa_addr, Some(Bytes::new()));
        let context = make_context(codehash_to_canonical, trace, HashMap::new());

        assert_eq!(context.resolve_canonical_via_codehash(eoa_addr), None);
    }

    #[test]
    fn resolve_canonical_via_codehash_returns_none_when_no_trace_entry() {
        // Address never appeared in the trace, so `bytecode_at` returns None
        // and the wrapper must propagate None without panicking.
        let absent_addr = address!("0000000000000000000000000000000000003333");
        let context = make_context(HashMap::new(), Trace::default(), HashMap::new());

        assert_eq!(context.resolve_canonical_via_codehash(absent_addr), None);
    }

    #[test]
    fn resolve_analysis_via_codehash_falls_back_to_canonical() {
        let canonical_addr = address!("0000000000000000000000000000000000000A1A");
        let etched_addr = address!("6342000000000000000000000000000000000006");
        let runtime: &[u8] = &[0xfe, 0xed, 0xbe, 0xef, 0x60, 0x80];
        let runtime_hash = keccak256(runtime);

        let mut codehash_to_canonical = HashMap::new();
        codehash_to_canonical.insert(runtime_hash, canonical_addr);

        // Only the canonical address has an analysis entry — the etched
        // address must transparently resolve to it through the codehash
        // wrapper.
        let mut analysis_results: HashMap<Address, AnalysisResult> = HashMap::new();
        analysis_results.insert(canonical_addr, AnalysisResult::default());

        let trace = trace_with_entry(etched_addr, Some(Bytes::copy_from_slice(runtime)));
        let context = make_context(codehash_to_canonical, trace, analysis_results);

        // Direct hit at the canonical address still works.
        assert!(
            context.resolve_analysis_via_codehash(canonical_addr).is_some(),
            "direct lookup at canonical address must succeed"
        );

        // Etched address resolves through the codehash fallback to the
        // canonical address's analysis entry.
        assert!(
            context.resolve_analysis_via_codehash(etched_addr).is_some(),
            "etched address must resolve to canonical analysis via codehash"
        );

        // Unrelated address with no codehash match yields None.
        let unrelated = address!("0000000000000000000000000000000000004444");
        assert!(context.resolve_analysis_via_codehash(unrelated).is_none());
    }
}
