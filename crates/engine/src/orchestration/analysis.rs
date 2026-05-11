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

//! Orchestration module that coordinates the main steps of the EDB debugging process.
//! This includes transaction replay, source code analysis, bytecode tweaking,
//! and snapshot generation.

use std::collections::HashMap;

use alloy_primitives::{Address, TxHash};
use edb_common::EdbContext;
use eyre::Result;
use revm::{
    Database, DatabaseCommit, DatabaseRef, InspectEvm, MainBuilder,
    context::{
        TxEnv,
        result::{ExecutionResult, HaltReason},
    },
    database::CacheDB,
};
use tracing::{debug, error, info};

use crate::{
    Artifact, CallTracer, CodeTweaker, EngineConfig, TraceReplayResult,
    analysis::{AnalysisResult, analyze},
};

/// Replay the target transaction and collect call trace with all touched addresses
pub fn replay_and_collect_trace<DB, Cheats>(
    ctx: EdbContext<DB>,
    tx: TxEnv,
    cheats: Option<&mut Cheats>,
) -> Result<TraceReplayResult>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
    Cheats: revm::Inspector<EdbContext<DB>>,
{
    info!("Replaying transaction to collect call trace and touched addresses");

    let mut tracer = CallTracer::new();
    let mut stack = crate::inspector::CheatedStack::new(cheats, &mut tracer);
    let mut evm = ctx.build_mainnet_with_inspector(&mut stack);

    let result = evm
        .inspect_one_tx(tx)
        .map_err(|e| eyre::eyre!("Failed to inspect the target transaction: {:?}", e))?;

    if let ExecutionResult::Halt { reason, .. } = result
        && matches!(reason, HaltReason::OutOfGas { .. })
    {
        error!("EDB cannot debug out-of-gas errors. Proceed at your own risk.")
    }

    let result = tracer.into_replay_result();

    for (address, deployed) in &result.visited_addresses {
        if *deployed {
            debug!("Contract {} was deployed during transaction replay", address);
        } else {
            debug!("Address {} was touched during transaction replay", address);
        }
    }

    // Print the trace tree structure
    result.execution_trace.print_trace_tree();

    Ok(result)
}

/// Analyze the source code for instrumentation points and variable usage
pub fn analyze_source_code(
    artifacts: &HashMap<Address, Artifact>,
) -> Result<HashMap<Address, AnalysisResult>> {
    info!("Analyzing source code to identify instrumentation points");

    let mut analysis_result = HashMap::new();
    for (address, artifact) in artifacts {
        debug!("Analyzing contract at address: {address}");
        let analysis = analyze(artifact)?;
        debug!("Finished analyzing contract at address: {address}");
        analysis_result.insert(*address, analysis);
    }

    Ok(analysis_result)
}

/// Tweak the bytecode of the contracts.
///
/// `deployed_in_tx` is a snapshot of the tracer's `visited_addresses` flag:
/// `true` means the contract was deployed during the current transaction.
/// For such addresses we skip the Etherscan `get_creation_tx` lookup
/// entirely — that's necessary for `edb test` synthetic addresses (which
/// have no Etherscan presence at all) and is also a sound fast path in
/// replay mode (the result would be the same: tx_hash == current tx).
pub async fn tweak_bytecode<DB>(
    config: &EngineConfig,
    ctx: &mut EdbContext<DB>,
    artifacts: &HashMap<Address, Artifact>,
    recompiled_artifacts: &HashMap<Address, Artifact>,
    tx_hash: TxHash,
    deployed_in_tx: &HashMap<Address, bool>,
) -> Result<Vec<Address>>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    info!("Tweaking bytecode");
    debug!(
        target: "edb::hook::creation",
        recompiled = recompiled_artifacts.len(),
        deployed_in_tx = deployed_in_tx.iter().filter(|(_, v)| **v).count(),
        "tweak_bytecode: starting",
    );

    let mut tweaker =
        CodeTweaker::new(ctx, config.rpc_proxy_url.clone(), config.etherscan_api_key.clone());

    let mut contracts_in_tx = Vec::new();

    // When there is no upstream RPC (e.g. `edb test`'s synthetic in-memory
    // chain), Etherscan lookups can't succeed for any address; route every
    // non-deployed-in-tx contract through the creation-hook path instead.
    // In replay mode this is `true` and we keep the existing semantics
    // (Etherscan failures bubble up as errors).
    let has_upstream = config.has_upstream_rpc();

    for (address, recompiled_artifact) in recompiled_artifacts {
        // Fast path: the tracer recorded this contract as deployed during the
        // current tx (relevant for `edb test`-style synthetic addresses with
        // no Etherscan presence, and a safe shortcut in replay mode).
        if deployed_in_tx.get(address).copied().unwrap_or(false) {
            debug!(
                "Skip tweaking contract {}, since the tracer flagged it as deployed in this tx",
                address
            );
            contracts_in_tx.push(*address);
            continue;
        }

        // No upstream RPC → no Etherscan path. This is `edb test`-mode; any
        // non-deployed-in-tx address must be one we etched locally (e.g. the
        // synthetic test entrypoint), so route it through the creation-hook
        // path using the lifted artifact's constructor.
        if !has_upstream {
            debug!(
                "Routing {} through creation-hook path (no upstream RPC for Etherscan lookup)",
                address
            );
            contracts_in_tx.push(*address);
            continue;
        }

        let creation_tx_hash = tweaker.get_creation_tx(address).await?;
        if creation_tx_hash == tx_hash {
            debug!(
                "Skip tweaking contract {}, since it was created by the transaction under investigation",
                address
            );
            contracts_in_tx.push(*address);
            continue;
        }

        let artifact = artifacts
            .get(address)
            .ok_or_else(|| eyre::eyre!("No original artifact found for address {}", address))?;

        tweaker
            .tweak(address, artifact, recompiled_artifact, config.quick)
            .await
            .map_err(|e| eyre::eyre!("Failed to tweak bytecode for contract {}: {}", address, e))?;
    }

    Ok(contracts_in_tx)
}
