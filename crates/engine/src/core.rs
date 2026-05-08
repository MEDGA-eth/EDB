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

//! Core engine functionality for transaction analysis and debugging.
//!
//! This module provides the main engine implementation that orchestrates the complete
//! debugging workflow for Ethereum transactions. It handles source code analysis,
//! contract instrumentation, transaction execution with debugging inspectors,
//! and RPC server management.
//!
//! # Workflow Overview
//!
//! 1. **Preparation**: Accept forked database and transaction configuration
//! 2. **Analysis**: Download and analyze contract source code
//! 3. **Instrumentation**: Inject debugging hooks into contract bytecode
//! 4. **Execution**: Replay transaction with comprehensive debugging inspectors
//! 5. **Collection**: Gather execution snapshots and trace data
//! 6. **API**: Start RPC server for debugging interface
//!
//! # Key Components
//!
//! - [`EngineConfig`] - Engine configuration and settings
//! - [`run_transaction_analysis`] - Main analysis workflow function
//! - Inspector coordination for comprehensive data collection
//! - Source code fetching and compilation management
//! - Snapshot generation and organization
//!
//! # Supported Features
//!
//! - **Multi-contract analysis**: Analyze all contracts involved in execution
//! - **Source fetching**: Automatic download from Etherscan and verification
//! - **Quick mode**: Fast analysis with reduced operations
//! - **Instrumentation**: Automatic debugging hook injection
//! - **Comprehensive inspection**: Opcode and source-level snapshot collection

use alloy_primitives::TxHash;
use dashmap::DashMap;
use edb_common::ForkResult;
use eyre::Result;
use revm::{Database, DatabaseCommit, DatabaseRef, context::Host, database::CacheDB};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::{Mutex, mpsc};
use tracing::info;

use crate::{
    EngineContext, SnapshotAnalysis, orchestration,
    rpc::{RpcServerHandle, start_debug_server},
    utils::next_etherscan_api_key,
};

/// Configuration for the EDB debugging engine.
///
/// Contains settings that control the engine's behavior during transaction analysis,
/// source code fetching, and debugging operations.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// RPC provider URL for blockchain interaction (typically a proxy or archive node)
    pub rpc_proxy_url: String,
    /// Optional Etherscan API key for automatic source code downloading and verification
    pub etherscan_api_key: Option<String>,
    /// Quick mode flag - when enabled, skips time-intensive operations for faster analysis
    pub quick: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            rpc_proxy_url: "http://localhost:8545".into(),
            etherscan_api_key: None,
            quick: false,
        }
    }
}

impl EngineConfig {
    /// Set the Etherscan API key for source code download
    pub fn with_etherscan_api_key(mut self, key: String) -> Self {
        self.etherscan_api_key = Some(key);
        self
    }

    /// Enable or disable quick mode for faster analysis
    pub fn with_quick_mode(mut self, quick: bool) -> Self {
        self.quick = quick;
        self
    }

    /// Set the RPC proxy URL for blockchain interactions
    pub fn with_rpc_proxy_url(mut self, url: String) -> Self {
        self.rpc_proxy_url = url;
        self
    }

    /// Get the Etherscan API key, either from config or rotate to the next available key
    pub fn get_etherscan_api_key(&self) -> String {
        self.etherscan_api_key.clone().unwrap_or(next_etherscan_api_key())
    }
}

/// The main Engine struct that performs transaction analysis
///
/// This struct is thread-safe and can be shared across multiple threads.
/// It uses per-transaction locking to ensure that only one thread can analyze
/// a given transaction at a time, while allowing concurrent analysis of different transactions.
#[derive(Debug)]
pub struct Engine {
    /// Concurrent map of transaction hashes to their RPC server handles
    server_handles: Arc<DashMap<TxHash, RpcServerHandle>>,

    /// Per-transaction locks to prevent duplicate analysis of the same transaction
    /// Each transaction hash gets its own lock, allowing parallel analysis of different
    /// transactions
    in_flight: Arc<DashMap<TxHash, Arc<Mutex<()>>>>,

    /// Configuration for the engine
    config: EngineConfig,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(EngineConfig::default())
    }
}

impl Engine {
    /// Create a new Engine instance from configuration
    pub fn new(config: EngineConfig) -> Self {
        Self {
            server_handles: Arc::new(DashMap::new()),
            in_flight: Arc::new(DashMap::new()),
            config,
        }
    }

    /// Get the RPC server address for a given transaction hash, if it exists
    pub fn get_rpc_server_addr(&self, tx_hash: &TxHash) -> Option<SocketAddr> {
        self.server_handles.get(tx_hash).map(|handle| handle.addr())
    }

    /// Shut down the RPC server for a given transaction hash, if it exists
    /// Returns true if the server was found and shut down, false otherwise
    pub fn shutdown_rpc_server(&self, tx_hash: &TxHash) -> Result<bool> {
        if let Some((_, handle)) = self.server_handles.remove(tx_hash) {
            handle.shutdown()?;
            info!("Shut down RPC server for transaction: {:?}", tx_hash);
            Ok(true)
        } else {
            info!("No RPC server found for transaction: {:?}", tx_hash);
            Ok(false)
        }
    }

    /// Main preparation method for the engine
    ///
    /// This method accepts a forked database and EVM configuration prepared by the edb binary.
    /// It focuses on the core debugging workflow:
    /// 1. Replays the target transaction to collect touched contracts
    /// 2. Downloads verified source code for each contract
    /// 3. Analyzes the source code to identify instrumentation points
    /// 4. Instruments and recompiles the source code
    /// 5. Collect opcode-level step execution results
    /// 6. Re-executes the transaction with state snapshots
    /// 7. Starts a JSON-RPC server with the analysis results and snapshots
    ///
    /// # Thread Safety
    ///
    /// This method is thread-safe and can be called concurrently from multiple threads.
    /// Per-transaction locking ensures that only one thread can analyze a given transaction
    /// at a time. If a transaction is already being analyzed by another thread, subsequent
    /// calls will wait for the analysis to complete and then return the cached result.
    pub async fn prepare<DB>(
        &self,
        fork_result: ForkResult<DB>,
        progress_tx: Option<mpsc::UnboundedSender<edb_common::ProgressMessage>>,
    ) -> Result<SocketAddr>
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
        <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
        <DB as Database>::Error: Clone + Send + Sync,
    {
        // a utility macro to send progress message to the progress channel, if it exists
        macro_rules! send_progress {
            // With step tracking: send_progress!(current, total, "message")
            ($current:expr, $total:expr, $($message:tt)*) => {
                progress_tx.as_ref().map(|tx| {
                    tx.send(edb_common::ProgressMessage::with_steps(
                        format!($($message)*),
                        $current,
                        $total
                    )).ok()
                });
            };
            // Without step tracking: send_progress!("message")
            ($($message:tt)*) => {
                progress_tx.as_ref().map(|tx| {
                    tx.send(edb_common::ProgressMessage::new(format!($($message)*))).ok()
                });
            };
        }

        let tx_hash = fork_result.target_tx_hash;

        // Get or create per-transaction lock
        let lock =
            self.in_flight.entry(tx_hash).or_insert_with(|| Arc::new(Mutex::new(()))).clone();

        // Acquire lock - blocks if another thread is processing this transaction
        let _guard = lock.lock().await;

        // Check if this transaction has already been analyzed
        if let Some(existing_handle) = self.server_handles.get(&tx_hash) {
            info!(
                "Transaction {:?} already analyzed, returning existing RPC server at {}",
                tx_hash,
                existing_handle.addr()
            );
            send_progress!("Transaction {:?} already analyzed", tx_hash);
            return Ok(existing_handle.addr());
        }

        info!("Starting engine preparation for transaction: {:?}", tx_hash);

        // Step 0: Initialize context and database
        let ForkResult { context: mut ctx, target_tx_env: tx, target_tx_hash: tx_hash, fork_info } =
            fork_result;

        // Step 1: Replay the target transaction to collect call trace and touched contracts
        send_progress!(
            1,
            8,
            "Replaying the target transaction to collect call trace and touched contracts..."
        );
        let replay_result = orchestration::replay_and_collect_trace(ctx.clone(), tx.clone())?;

        // Step 2: Download verified source code for each contract
        send_progress!(2, 8, "Downloading verified source code for each contract...");
        let artifacts = orchestration::download_verified_source_code(
            &self.config,
            &replay_result,
            ctx.chain_id().to::<u64>(),
        )
        .await?;

        // Step 3: Analyze source code to identify instrumentation points
        send_progress!(3, 8, "Analyzing source code to identify instrumentation points...");
        let analysis_results = orchestration::analyze_source_code(&artifacts)?;

        // Step 4: Instrument source code
        send_progress!(4, 8, "Instrumenting source code...");
        let recompiled_artifacts =
            orchestration::instrument_and_recompile_source_code(&artifacts, &analysis_results)?;

        // Step 5: Collect opcode-level step execution results
        send_progress!(5, 8, "Collecting opcode-level step execution results...");
        let opcode_snapshots = orchestration::capture_opcode_level_snapshots(
            ctx.clone(),
            tx.clone(),
            artifacts.keys().cloned().collect(),
            &replay_result.execution_trace,
        )?;

        // Step 6: Replace original bytecode with instrumented versions
        send_progress!(6, 8, "Replacing original bytecode with instrumented versions...");
        let contracts_in_tx = orchestration::tweak_bytecode(
            &self.config,
            &mut ctx,
            &artifacts,
            &recompiled_artifacts,
            tx_hash,
        )
        .await?;

        // Step 7: Re-execute the transaction with snapshot collection
        send_progress!(7, 8, "Collecting creation hooks for contracts in transaction...");
        let hook_creation = orchestration::collect_creation_hooks(
            &artifacts,
            &recompiled_artifacts,
            contracts_in_tx,
        )?;
        let hook_snapshots = orchestration::capture_hook_snapshots(
            ctx.clone(),
            tx.clone(),
            hook_creation,
            &replay_result.execution_trace,
            &analysis_results,
        )?;

        // Step 8: Start RPC server with analysis results and snapshots
        send_progress!(8, 8, "Collecting opcode-level and hook-level snapshots...");
        let mut snapshots =
            orchestration::get_time_travel_snapshots(opcode_snapshots, hook_snapshots)?;
        snapshots.analyze(&replay_result.execution_trace, &analysis_results)?;

        // Let's pack the debug context
        let context = EngineContext::build(
            fork_info,
            ctx.cfg.clone(),
            ctx.block.clone(),
            tx,
            tx_hash,
            snapshots,
            artifacts,
            recompiled_artifacts,
            analysis_results,
            replay_result.execution_trace,
        )?;

        let rpc_handle = start_debug_server(context).await?;
        info!("Debug RPC server started on {}", rpc_handle.addr());

        // Store the server handle for future reference
        let addr = rpc_handle.addr();
        self.server_handles.insert(tx_hash, rpc_handle);

        Ok(addr)
    }
}
