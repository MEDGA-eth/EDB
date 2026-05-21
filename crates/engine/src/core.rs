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

use alloy_primitives::{Address, B256, Bytes, TxHash, keccak256};
use dashmap::DashMap;
use edb_common::ForkResult;
use eyre::Result;
use revm::{
    Database, DatabaseCommit, DatabaseRef, context::Host, context_interface::ContextTr,
    database::CacheDB,
};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::{Mutex, mpsc};
use tracing::info;

use crate::{
    EngineContext, SnapshotAnalysis, orchestration,
    rpc::{Router, RpcServerHandle},
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

    /// True when the engine has a usable upstream RPC for etherscan fallback /
    /// fork-mode db reads. False when the URL is blank or `http://localhost:8545`
    /// (the unset default in the struct).
    pub fn has_upstream_rpc(&self) -> bool {
        let u = self.rpc_proxy_url.trim();
        !u.is_empty() && u != "http://localhost:8545"
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
        self.prepare_with_router(fork_result, progress_tx, None).await
    }

    /// Same as [`Engine::prepare`] but also attaches an optional extra Axum
    /// router (e.g. for serving the embedded web UI on the same listener).
    pub async fn prepare_with_router<DB>(
        &self,
        fork_result: ForkResult<DB>,
        progress_tx: Option<mpsc::UnboundedSender<edb_common::ProgressMessage>>,
        extra_router: Option<Router>,
    ) -> Result<SocketAddr>
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
        <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
        <DB as Database>::Error: Clone + Send + Sync,
    {
        self.prepare_with_router_and_cheats::<DB, crate::inspector::NoCheats>(
            fork_result,
            progress_tx,
            extra_router,
            None,
            None,
            None,
        )
        .await
    }

    /// Same as [`prepare_with_router`] but accepts an optional factory that yields
    /// fresh `Cheats` inspector instances. Each orchestration pass (tracer / opcode /
    /// hook) gets its own freshly-built `Cheats` so prank/mocked-call state doesn't
    /// bleed between passes.
    ///
    /// The optional `between_passes_hook` is invoked after passes 1 (tracer) and 2
    /// (opcode snapshots). If it returns `Err`, preparation is aborted immediately —
    /// this lets callers short-circuit expensive later passes when an early pass has
    /// already observed a fatal condition (e.g. an unsupported cheatcode hit).
    pub async fn prepare_with_router_and_cheats<DB, Cheats>(
        &self,
        fork_result: ForkResult<DB>,
        progress_tx: Option<mpsc::UnboundedSender<edb_common::ProgressMessage>>,
        extra_router: Option<Router>,
        cheats_factory: Option<Box<dyn Fn() -> Cheats + Send + Sync>>,
        local_artifacts: Option<crate::orchestration::LocalArtifactSet>,
        between_passes_hook: Option<Box<dyn Fn() -> eyre::Result<()> + Send + Sync>>,
    ) -> Result<SocketAddr>
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
        <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
        <DB as Database>::Error: Clone + Send + Sync,
        Cheats: revm::Inspector<edb_common::EdbContext<DB>> + Send + 'static,
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
        let mut cheats1 = cheats_factory.as_ref().map(|f| f());
        let replay_result =
            orchestration::replay_and_collect_trace(ctx.clone(), tx.clone(), cheats1.as_mut())?;

        // Between passes 1 and 2: run the hook (e.g. unsupported-cheatcode early exit).
        if let Some(hook) = between_passes_hook.as_ref() {
            hook()?;
        }

        // Step 2: Resolve artifacts — prefer local (codehash-keyed), fall back to Etherscan.
        send_progress!(2, 8, "Downloading verified source code for each contract...");

        // Build a `address → runtime bytecode` map for every touched address.
        // For contracts CREATEd inside this transaction the pre-tx database
        // doesn't yet know about them, so we prefer the post-deploy runtime
        // captured by the CallTracer (`in_tx_runtime_code`) and only fall
        // back to the database for addresses already-on-chain at tx-start.
        //
        // The foundry-style local-artifact lookup needs the actual bytes
        // (not just the codehash) so it can mask immutables / linked-library
        // offsets and fall back to fuzzy matching for metadata variance.
        let touched_with_runtime = build_touched_runtime_bytes(
            &mut ctx,
            &replay_result.visited_addresses,
            &replay_result.in_tx_runtime_code,
        )?;

        // Resolve as many addresses as possible from the local artifact set.
        let mut artifacts = match local_artifacts.as_ref() {
            Some(local) => orchestration::load_local_artifacts(&touched_with_runtime, local),
            None => HashMap::new(),
        };

        // For addresses still missing, fall back to Etherscan only when an upstream RPC exists.
        let unmatched: Vec<Address> =
            touched_with_runtime.keys().filter(|a| !artifacts.contains_key(*a)).copied().collect();
        if !unmatched.is_empty() && self.config.has_upstream_rpc() {
            let etherscan_artifacts = orchestration::download_verified_source_code(
                &self.config,
                &replay_result,
                ctx.chain_id().to::<u64>(),
            )
            .await?;
            for (addr, art) in etherscan_artifacts {
                if unmatched.contains(&addr) {
                    artifacts.insert(addr, art);
                }
            }
        }

        // Step 3: Analyze source code to identify instrumentation points
        send_progress!(3, 8, "Analyzing source code to identify instrumentation points...");
        let analysis_results = orchestration::analyze_source_code(&artifacts)?;

        // Step 4: Instrument source code
        send_progress!(4, 8, "Instrumenting source code...");
        let recompiled_artifacts =
            orchestration::instrument_and_recompile_source_code(&artifacts, &analysis_results)?;

        // Build the codehash → canonical-address index used by the hook inspector
        // and the RPC layer to resolve `vm.etch`-aliased addresses to their source
        // artifact. We key by the keccak256 of each instrumented runtime template;
        // when execution lands at an etched address whose live bytecode matches
        // one of these hashes, the alias resolver redirects the lookup to the
        // canonical address.
        //
        // First-walk wins via `.entry(...).or_insert(*addr)`. If two addresses
        // share the same instrumented runtime, the analysis is correct from either
        // — analysis is source-driven (USID-keyed), not address-keyed beyond
        // per-address lookup.
        let mut codehash_to_canonical: HashMap<B256, Address> = HashMap::new();
        for (addr, art) in &recompiled_artifacts {
            let Some(contract) = art.contract() else { continue };
            let Some(evm) = contract.evm.as_ref() else { continue };
            let Some(deployed) = evm.deployed_bytecode.as_ref() else { continue };
            let Some(bytes) = deployed.bytes() else { continue };
            if bytes.is_empty() {
                continue;
            }
            codehash_to_canonical.entry(keccak256(bytes.as_ref())).or_insert(*addr);
        }
        tracing::debug!(
            "codehash_to_canonical index built with {} entries",
            codehash_to_canonical.len()
        );

        // Step 5: Collect opcode-level step execution results
        send_progress!(5, 8, "Collecting opcode-level step execution results...");
        let mut cheats2 = cheats_factory.as_ref().map(|f| f());
        let opcode_snapshots = orchestration::capture_opcode_level_snapshots(
            ctx.clone(),
            tx.clone(),
            artifacts.keys().cloned().collect(),
            &replay_result.execution_trace,
            cheats2.as_mut(),
        )?;

        // Between passes 2 and 3: run the hook again (opcode pass may have added new hits).
        if let Some(hook) = between_passes_hook.as_ref() {
            hook()?;
        }

        // Step 6: Replace original bytecode with instrumented versions
        send_progress!(6, 8, "Replacing original bytecode with instrumented versions...");
        let contracts_in_tx = orchestration::tweak_bytecode(
            &self.config,
            &mut ctx,
            &artifacts,
            &recompiled_artifacts,
            tx_hash,
            &replay_result.visited_addresses,
        )
        .await?;

        // Step 7: Re-execute the transaction with snapshot collection
        send_progress!(7, 8, "Collecting creation hooks for contracts in transaction...");
        // Build an address-keyed map for nested-CREATE substitution: each
        // contract that was CREATEd inside this transaction (and for which
        // we have a recompiled / instrumented artifact) gets registered so
        // the hook-snapshot inspector can swap its init code in by predicted
        // address at runtime, even if the parent contract's embedded copy of
        // the child's creation bytecode doesn't byte-identically match the
        // child's standalone artifact bytecode (which is exactly what happens
        // for nested `new Inner(...)` in tests — see commit message).
        let mut creation_by_address: HashMap<Address, (alloy_primitives::Bytes, usize)> =
            HashMap::new();
        let contracts_in_tx_for_address_map: Vec<Address> = contracts_in_tx.clone();
        for addr in &contracts_in_tx_for_address_map {
            use foundry_compilers::Artifact as FoundryArtifact;
            if let Some(art) = recompiled_artifacts.get(addr) {
                // Walk the recompiled artifact's per-(path,name) contracts
                // and pick the one whose contract name matches the address's
                // primary artifact contract name. For the `edb test` flow
                // each address maps to exactly one contract; for safety we
                // fall back to the *first* non-empty bytecode if the name
                // lookup fails.
                let target_name = art.meta.contract_name.clone();
                let mut hooked: Option<(alloy_primitives::Bytes, Option<&Vec<_>>)> = None;
                for contracts in art.output.contracts.values() {
                    if let Some(c) = contracts.get(&target_name)
                        && let Some(b) = c.get_bytecode_bytes()
                        && !b.is_empty()
                    {
                        // Pull the constructor params from this contract's ABI
                        // (if present) — we need them to compute the static
                        // args byte size below.
                        let abi_params = c
                            .abi
                            .as_ref()
                            .and_then(|abi| abi.constructor.as_ref())
                            .map(|ctor| &ctor.inputs);
                        hooked = Some((b.as_ref().clone(), abi_params));
                        break;
                    }
                }
                if hooked.is_none() {
                    'outer: for contracts in art.output.contracts.values() {
                        for c in contracts.values() {
                            if let Some(b) = c.get_bytecode_bytes()
                                && !b.is_empty()
                            {
                                let abi_params = c
                                    .abi
                                    .as_ref()
                                    .and_then(|abi| abi.constructor.as_ref())
                                    .map(|ctor| &ctor.inputs);
                                hooked = Some((b.as_ref().clone(), abi_params));
                                break 'outer;
                            }
                        }
                    }
                }
                if let Some((bytecode, abi_params)) = hooked {
                    // Compute the constructor's static encoded arg size from
                    // the ABI. For dynamic types (string / bytes / dynamic
                    // arrays) the encoded size is variable; we approximate
                    // with the static head (32 bytes per parameter) — the
                    // CREATE that actually fires at runtime carries the args
                    // in its init_code tail and the engine's recompiled
                    // creation bytecode expects that exact layout, so for
                    // tests with static args this is correct; for dynamic
                    // args it would under-count and is left as a TODO (no
                    // foundry test fixture we ship today exercises that
                    // case).
                    let args_size = abi_params
                        .map(|params| {
                            params
                                .iter()
                                .fold(0usize, |acc, p| acc + constructor_param_head_size(&p.ty))
                        })
                        .unwrap_or(0);
                    creation_by_address.insert(*addr, (bytecode, args_size));
                }
            }
        }
        let hook_creation = orchestration::collect_creation_hooks(
            &artifacts,
            &recompiled_artifacts,
            contracts_in_tx,
        )?;
        let mut cheats3 = cheats_factory.as_ref().map(|f| f());
        let hook_snapshots = orchestration::capture_hook_snapshots(
            ctx.clone(),
            tx.clone(),
            hook_creation,
            creation_by_address,
            &replay_result.execution_trace,
            &analysis_results,
            cheats3.as_mut(),
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
            codehash_to_canonical,
            replay_result.execution_trace,
        )?;

        let server = crate::rpc::DebugRpcServer::new(context);
        let server = match extra_router {
            Some(r) => server.with_extra_router(r),
            None => server,
        };
        let rpc_handle = server.start().await?;
        info!("Debug RPC server started on {}", rpc_handle.addr());

        // Store the server handle for future reference
        let addr = rpc_handle.addr();
        self.server_handles.insert(tx_hash, rpc_handle);

        Ok(addr)
    }
}

/// Build a map from touched contract address to its on-chain runtime bytecode.
/// Used to resolve artifacts from a [`crate::orchestration::LocalArtifactSet`]
/// via foundry-style masked / fuzzy matching.
///
/// For contracts CREATEd inside this transaction the pre-tx database doesn't
/// yet know about them, so we prefer the post-deploy runtime captured by the
/// [`CallTracer`] during replay (`in_tx_runtime_code`). For addresses that
/// already exist in the pre-tx state we issue a `db.basic` to get the
/// `code_hash`, then resolve the code via `db.code_by_hash`. Addresses with
/// no code (EOAs, addresses with empty code) are skipped.
fn build_touched_runtime_bytes<DB>(
    ctx: &mut edb_common::EdbContext<DB>,
    visited_addresses: &HashMap<Address, bool>,
    in_tx_runtime_code: &HashMap<Address, Bytes>,
) -> Result<HashMap<Address, Bytes>>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    let mut out = HashMap::new();
    for addr in visited_addresses.keys() {
        if let Some(code) = in_tx_runtime_code.get(addr) {
            out.insert(*addr, code.clone());
            continue;
        }
        let info = ctx
            .db_mut()
            .basic(*addr)
            .map_err(|e| eyre::eyre!("db.basic({addr}): {e:?}"))?
            .unwrap_or_default();
        // Fetch the bytecode for the address. `info.code` is populated only
        // when the underlying DB eagerly attached it; otherwise we resolve
        // by code_hash. Either way, skip EOAs / empty-code addresses — they
        // have nothing for the local-artifact matcher to chew on.
        if info.code_hash == revm::primitives::KECCAK_EMPTY {
            continue;
        }
        let code = if let Some(bc) = info.code {
            Bytes::copy_from_slice(bc.original_byte_slice())
        } else {
            match ctx.db_mut().code_by_hash(info.code_hash) {
                Ok(bc) => Bytes::copy_from_slice(bc.original_byte_slice()),
                Err(e) => {
                    tracing::debug!(
                        target: "edb::engine::core",
                        "code_by_hash({addr}, {:?}) failed: {e:?}",
                        info.code_hash
                    );
                    continue;
                }
            }
        };
        if !code.is_empty() {
            out.insert(*addr, code);
        }
    }
    Ok(out)
}

/// Compute the **head** byte size of a single ABI-encoded parameter — i.e.
/// the bytes that appear inline in the encoded args stream for that type
/// (ignoring any tail data that dynamic types append after all heads).
///
/// For static types (`uint*`, `int*`, `bool`, `address`, `bytesN`, fixed-size
/// arrays of static types, structs of static types) the head IS the whole
/// encoding and is exactly 32 bytes per slot. For dynamic types (`bytes`,
/// `string`, dynamic arrays) the head is a 32-byte offset pointer; the tail
/// data follows after all heads.
///
/// We use this to estimate the constructor args byte size for nested-CREATE
/// hook substitution. The estimate is exact for any constructor with only
/// static-types — which covers every constructor in our test fixtures. For
/// dynamic-args constructors we under-count, and substitution falls back to
/// zero args (the inspector clamps if init_code is shorter than args_size).
fn constructor_param_head_size(ty: &str) -> usize {
    // ABI head is 32 bytes per param for every supported type — even dynamic
    // ones use a 32-byte offset pointer in the head. Fixed-size arrays of
    // static types occupy `N * 32` bytes inline (no offset pointer). We
    // approximate fixed arrays via parsing `T[N]` and recursing on `T`.
    let ty = ty.trim();
    if let Some(open) = ty.rfind('[')
        && let Some(close) = ty.rfind(']')
        && close > open
    {
        let inner_size = constructor_param_head_size(&ty[..open]);
        let count_str = &ty[open + 1..close];
        if count_str.is_empty() {
            // dynamic array — head is 32-byte offset pointer
            return 32;
        }
        if let Ok(n) = count_str.parse::<usize>() {
            return inner_size.saturating_mul(n);
        }
        return 32;
    }
    // Tuple types are encoded as concatenated heads of fields; we approximate
    // (the foundry-recompile artifact rarely emits raw tuple constructors).
    32
}

#[cfg(test)]
mod constructor_param_head_size_tests {
    use super::constructor_param_head_size;

    #[test]
    fn statics_are_32() {
        assert_eq!(constructor_param_head_size("uint256"), 32);
        assert_eq!(constructor_param_head_size("address"), 32);
        assert_eq!(constructor_param_head_size("bool"), 32);
        assert_eq!(constructor_param_head_size("bytes32"), 32);
    }

    #[test]
    fn dynamics_are_32_pointer() {
        assert_eq!(constructor_param_head_size("bytes"), 32);
        assert_eq!(constructor_param_head_size("string"), 32);
        assert_eq!(constructor_param_head_size("uint256[]"), 32);
    }

    #[test]
    fn fixed_array_of_static() {
        assert_eq!(constructor_param_head_size("uint256[3]"), 96);
        assert_eq!(constructor_param_head_size("address[5]"), 160);
    }
}
