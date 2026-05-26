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

//! Snapshotting module that captures the EVM state at various points during
//! transaction execution to enable time travel debugging.
use std::collections::{HashMap, HashSet};

use alloy_primitives::{Address, B256, Bytes};
use edb_common::{EdbContext, relax_evm_constraints, types::Trace};
use eyre::Result;
use foundry_compilers::artifacts::Contract;
use revm::{
    Database, DatabaseCommit, DatabaseRef, InspectEvm, MainBuilder, context::TxEnv,
    database::CacheDB,
};
use tracing::{debug, info};

use crate::{
    Artifact, HookSnapshotInspector, HookSnapshots, OpcodeSnapshotInspector, OpcodeSnapshots,
    Snapshots, analysis::AnalysisResult,
};

/// Time travel (i.e., snapshotting) at the opcode level for contracts we do not
/// have source code.
pub fn capture_opcode_level_snapshots<DB, Cheats>(
    ctx: EdbContext<DB>,
    tx: TxEnv,
    excluded_addresses: HashSet<Address>,
    trace: &Trace,
    cheats: Option<&mut Cheats>,
) -> Result<OpcodeSnapshots<DB>>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
    Cheats: revm::Inspector<EdbContext<DB>>,
{
    info!("Collecting opcode-level step execution results");

    let mut inspector = OpcodeSnapshotInspector::new(&ctx, trace);
    inspector.with_excluded_addresses(excluded_addresses);
    {
        let mut stack = crate::inspector::CheatedStack::new(cheats, &mut inspector);
        let mut evm = ctx.build_mainnet_with_inspector(&mut stack);
        evm.inspect_one_tx(tx)
            .map_err(|e| eyre::eyre!("Failed to inspect the target transaction: {:?}", e))?;
    }

    let snapshots = inspector.into_snapshots();

    snapshots.print_summary();

    Ok(snapshots)
}

/// Collect creation hooks for contracts we have source code.
pub fn collect_creation_hooks<'a>(
    artifacts: &'a HashMap<Address, Artifact>,
    recompiled_artifacts: &'a HashMap<Address, Artifact>,
    contracts_in_tx: Vec<Address>,
) -> Result<Vec<(&'a Contract, &'a Contract, &'a Bytes)>> {
    info!("Collecting creation hooks for contracts in transaction");
    debug!(
        target: "edb::hook::creation",
        contracts_in_tx = contracts_in_tx.len(),
        "collect_creation_hooks: candidate addresses = {:?}",
        contracts_in_tx,
    );

    let mut hook_creation = Vec::new();
    for address in contracts_in_tx {
        let Some(artifact) = artifacts.get(&address) else {
            eyre::bail!("No original artifact found for address {}", address);
        };

        let Some(recompiled_artifact) = recompiled_artifacts.get(&address) else {
            eyre::bail!("No recompiled artifact found for address {}", address);
        };

        hook_creation.extend(artifact.find_creation_hooks(recompiled_artifact));
    }

    debug!(
        target: "edb::hook::creation",
        hooks = hook_creation.len(),
        "collect_creation_hooks: produced {} creation hooks",
        hook_creation.len(),
    );

    Ok(hook_creation)
}

/// Time travel (i.e., snapshotting) at hooks for contracts we have source code
///
/// `creation_by_address` is an optional predicted-address-keyed map of
/// hooked creation bytecode. Entries here let the inspector apply the
/// substitution by predicted address (the address that *will* be CREATEd)
/// rather than by bytecode prefix-match — required for nested CREATEs where
/// the parent's embedded child-creation-code does not byte-identically match
/// the child's standalone artifact bytecode.
#[allow(clippy::too_many_arguments)]
pub fn capture_hook_snapshots<'a, DB, Cheats>(
    mut ctx: EdbContext<DB>,
    mut tx: TxEnv,
    creation_hooks: Vec<(&'a Contract, &'a Contract, &'a Bytes)>,
    creation_by_address: HashMap<Address, (Bytes, usize)>,
    trace: &Trace,
    analysis_results: &HashMap<Address, AnalysisResult>,
    codehash_to_canonical: &HashMap<B256, Address>,
    cheats: Option<&mut Cheats>,
) -> Result<HookSnapshots<DB>>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
    Cheats: revm::Inspector<EdbContext<DB>>,
{
    info!("Re-executing transaction with snapshot collection");

    // We need to relax execution constraints for hook snapshots
    relax_evm_constraints(&mut ctx, &mut tx);

    info!("Collecting hook snapshots for source code contracts");

    let mut inspector =
        HookSnapshotInspector::new(&ctx, trace, analysis_results, codehash_to_canonical);
    inspector.with_creation_hooks(creation_hooks)?;
    inspector.with_creation_by_address(creation_by_address);
    {
        let mut stack = crate::inspector::CheatedStack::new(cheats, &mut inspector);
        let mut evm = ctx.build_mainnet_with_inspector(&mut stack);
        evm.inspect_one_tx(tx)
            .map_err(|e| eyre::eyre!("Failed to inspect the target transaction: {:?}", e))?;
    }

    let snapshots = inspector.into_snapshots();

    snapshots.print_summary();

    Ok(snapshots)
}

/// Merge opcode-level and hook-level snapshots into a unified snapshot structure
/// that supports time travel across the entire execution trace.
pub fn get_time_travel_snapshots<DB>(
    opcode_snapshots: OpcodeSnapshots<DB>,
    hook_snapshots: HookSnapshots<DB>,
) -> Result<Snapshots<DB>>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    info!("Merging opcode-level and hook-level snapshots");

    let snapshots = Snapshots::merge(opcode_snapshots, hook_snapshots);
    snapshots.print_summary();

    Ok(snapshots)
}
