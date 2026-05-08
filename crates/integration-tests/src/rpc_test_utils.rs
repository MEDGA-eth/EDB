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

//! RPC test utilities for comprehensive JSON-RPC testing

use alloy_json_abi::JsonAbi;
use alloy_primitives::{Address, Bytes, TxHash, U256};
use edb_common::types::{Breakpoint, CallableAbiInfo, Code, EdbSolValue, SnapshotInfo, Trace};
use eyre::Result;
use futures::future::join_all;
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info};

use crate::test_utils::{engine, paths, proxy};

/// Test transaction fixtures used across all RPC tests
pub mod test_transactions {
    /// Simple transaction fixture with minimal complexity for basic testing.
    /// This transaction has few calls and is ideal for validating core RPC functionality.
    pub const SIMPLE: (&str, &str) =
        ("simple", "0x608d79d71287ca8a0351955a92fa4dce74d2c75cbfccfa08ed331b33de0ce4c2");

    /// Large transaction fixture with high complexity for stress testing.
    /// This transaction contains many calls and is useful for performance validation.
    pub const LARGE: (&str, &str) =
        ("large", "0x0886e768f9310a753e360738e1aa6647847aca57c2ce05f09ca1333e8cf81e8c");

    /// Uniswap V3 transaction fixture for DeFi protocol testing.
    /// This transaction involves complex swap operations and pool interactions.
    pub const UNISWAP_V3: (&str, &str) =
        ("uniswap_v3", "0x1282e09bb5118f619da81b6a24c97999e7057ee9975628562c7cecbb4aa9f5af");

    /// Another Uniswap V3 transaction fixture for additional DeFi testing.
    /// This transaction tests different swap paths and fee tiers.
    pub const UNISWAP_V3_ALT: (&str, &str) =
        ("uniswap_v3_alt", "0x4c1bde4613d88154600522068fa2c6a66141ab8150c41ad3c216c4d1d67175a1");

    /// Uniswap V4 transaction fixture for next-generation DeFi testing.
    /// This transaction demonstrates advanced DEX features and hook functionality.
    pub const UNISWAP_V4: (&str, &str) =
        ("uniswap_v4", "0x258b5a643ae7ccb8e45a4ea1e308f708c8e0eb6e2f535ec68f678c944c98b402");

    /// A transaction which previously triggers out-of-gas error when tweaking contracts.
    /// This is used to test gas limit relaxation at callsites.
    pub const OOG_TWEAK: (&str, &str) =
        ("oog_tweak", "0xc14075310349be0c3a1dc4ee58c761c22f25c2b72c12d22f5e1c06f66f94b958");

    /// Returns all available test transaction fixtures.
    /// This provides a complete set of transactions covering different complexity levels and use
    /// cases.
    pub fn all() -> Vec<(&'static str, &'static str)> {
        vec![SIMPLE, LARGE, UNISWAP_V3, UNISWAP_V3_ALT, UNISWAP_V4, OOG_TWEAK]
    }
}

/// Engine fixture containing RPC URL and expected responses
/// The rpc_handle keeps the RPC server alive as long as this struct exists
#[derive(Clone)]
pub struct EngineFixture {
    /// Transaction hash that this fixture is based on
    pub tx_hash: String,
    /// RPC server URL for making JSON-RPC calls to this engine instance
    pub rpc_url: String,
    /// Handle to the replay test result containing the engine state and server
    /// Proxy URL used for this engine instance
    pub proxy_url: String,
    /// IMPORTANT: This keeps the RPC server alive - don't drop it!
    pub rpc_handle: Arc<engine::ReplayTestResult>,
}

type FixtureMap = Lazy<Arc<RwLock<HashMap<String, Arc<Mutex<Option<EngineFixture>>>>>>>;

/// Thread-safe storage for engine fixtures with fine-grained locking
/// Each fixture has its own lock to allow independent creation
static ENGINE_FIXTURES: FixtureMap = Lazy::new(|| {
    let mut map = HashMap::new();
    // Pre-populate with empty entries for known transactions
    for (name, _) in test_transactions::all() {
        map.insert(name.to_string(), Arc::new(Mutex::new(None)));
    }
    Arc::new(RwLock::new(map))
});

/// Get or create a single engine fixture for a specific transaction
pub async fn get_or_create_fixture(name: &str) -> Result<EngineFixture> {
    // First, get the mutex for this specific fixture (read lock)
    let fixture_mutex = {
        let fixtures_read = ENGINE_FIXTURES.read().await;
        fixtures_read
            .get(name)
            .ok_or_else(|| eyre::eyre!("Unknown transaction name: {}", name))?
            .clone()
    };

    // Now lock only this specific fixture
    let mut fixture_guard = fixture_mutex.lock().await;

    // Check if this specific fixture already exists
    if let Some(fixture) = fixture_guard.as_ref() {
        info!("Using cached fixture for '{}' at {}", name, fixture.rpc_url);
        return Ok(fixture.clone());
    }

    // Get transaction hash for this name
    let tx_hash_str = test_transactions::all()
        .into_iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| eyre::eyre!("Unknown transaction name: {}", name))?
        .1;

    info!("Creating engine fixture for '{}' transaction: {}", name, tx_hash_str);

    // Create a dedicated proxy for this transaction
    info!("Setting up dedicated proxy for '{}'...", name);
    let proxy_url = proxy::setup_test_proxy_configurable(7200) // 2 hours timeout
        .await
        .map_err(|error| eyre::eyre!("Failed to setup test proxy: {error}"))?;

    info!("Proxy for '{}' started at: {}", name, proxy_url);
    proxy::register_with_proxy(&proxy_url).await.ok();

    // Create engine for this specific transaction
    let tx_hash: TxHash = tx_hash_str.parse().expect("valid tx hash");
    let result = engine::replay_transaction_test(tx_hash, &proxy_url, false, None)
        .await
        .map_err(|error| eyre::eyre!("Failed to replay transaction: {error}"))?;

    if !result.success {
        panic!("Engine creation failed for {}: {:?}", name, result.errors);
    }

    let rpc_url = format!("http://{}", result.rpc_handle_addr);
    info!("Engine RPC server started at: {}", rpc_url);

    let fixture = EngineFixture {
        tx_hash: tx_hash_str.to_string(),
        rpc_url,
        proxy_url,
        rpc_handle: Arc::new(result),
    };

    *fixture_guard = Some(fixture.clone());
    info!("Fixture for '{}' created and cached successfully", name);
    Ok(fixture)
}

/// Get or create all engine fixtures for all test transactions
pub async fn get_or_create_fixtures() -> Result<HashMap<String, EngineFixture>> {
    // Create fixtures in parallel for better performance
    let fixture_futures: Vec<_> = test_transactions::all()
        .into_iter()
        .map(|(name, _)| async move {
            let result = get_or_create_fixture(name).await;
            (name.to_string(), result)
        })
        .collect();

    let fixture_results = join_all(fixture_futures).await;

    let mut all_fixtures = HashMap::new();
    for (name, result) in fixture_results {
        let fixture = result?;
        all_fixtures.insert(name, fixture);
    }

    Ok(all_fixtures)
}

/// RPC client for making JSON-RPC calls
pub struct RpcTestClient {
    client: Client,
    url: String,
}

impl RpcTestClient {
    /// Create a new RPC test client
    pub fn new(url: &str) -> Self {
        Self { client: Client::new(), url: url.to_string() }
    }

    /// Make a raw JSON-RPC call
    pub async fn call_raw(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let request = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params.unwrap_or(json!([])),
            "id": 1
        });

        let response = self.client.post(&self.url).json(&request).send().await?;

        let body: Value = response.json().await?;

        if let Some(error) = body.get("error") {
            return Err(eyre::eyre!("RPC error: {}", error));
        }

        Ok(body["result"].clone())
    }

    // Typed RPC method calls

    /// Get the complete execution trace for the transaction.
    /// Returns all call frames with their input/output data and gas usage.
    pub async fn get_trace(&self) -> Result<Trace> {
        let value = self.call_raw("edb_getTrace", None).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Get the contract code at a specific snapshot.
    /// Returns either opcode or source code information depending on availability.
    pub async fn get_code(&self, snapshot_id: usize) -> Result<Code> {
        let value = self.call_raw("edb_getCode", Some(json!([snapshot_id]))).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Get contract code by its address.
    /// Retrieves code information for the specified contract address.
    pub async fn get_code_by_address(&self, address: Address) -> Result<Code> {
        let value = self.call_raw("edb_getCodeByAddress", Some(json!([address]))).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Get constructor arguments for a contract deployment.
    /// Returns the encoded constructor parameters if available.
    pub async fn get_constructor_args(&self, address: Address) -> Result<Option<Bytes>> {
        let value = self.call_raw("edb_getConstructorArgs", Some(json!([address]))).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Get the total number of snapshots available for this transaction.
    /// Each snapshot represents a point in execution where state can be inspected.
    pub async fn get_snapshot_count(&self) -> Result<usize> {
        let value = self.call_raw("edb_getSnapshotCount", None).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Get detailed information about a specific snapshot.
    /// Includes frame information and execution context for the given snapshot.
    pub async fn get_snapshot_info(&self, snapshot_id: usize) -> Result<SnapshotInfo> {
        let value = self.call_raw("edb_getSnapshotInfo", Some(json!([snapshot_id]))).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Get the ABI for a contract at the specified address.
    /// The recompiled flag determines whether to use recompiled or original ABI data.
    pub async fn get_contract_abi(
        &self,
        address: Address,
        recompiled: bool,
    ) -> Result<Option<JsonAbi>> {
        let value = self.call_raw("edb_getContractABI", Some(json!([address, recompiled]))).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Get callable function information for a contract.
    /// Returns ABI entries for functions that can be invoked on the contract.
    pub async fn get_callable_abi(&self, address: Address) -> Result<Vec<CallableAbiInfo>> {
        let value = self.call_raw("edb_getCallableABI", Some(json!([address]))).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Navigate to the next function call from the current snapshot.
    /// Returns the snapshot ID of the next call frame in execution order.
    pub async fn get_next_call(&self, snapshot_id: usize) -> Result<usize> {
        let value = self.call_raw("edb_getNextCall", Some(json!([snapshot_id]))).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Navigate to the previous function call from the current snapshot.
    /// Returns the snapshot ID of the previous call frame in execution order.
    pub async fn get_prev_call(&self, snapshot_id: usize) -> Result<usize> {
        let value = self.call_raw("edb_getPrevCall", Some(json!([snapshot_id]))).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Get storage value at a specific slot and snapshot.
    /// Retrieves the contract storage state at the given execution point.
    pub async fn get_storage(&self, snapshot_id: usize, slot: U256) -> Result<U256> {
        let value = self.call_raw("edb_getStorage", Some(json!([snapshot_id, slot]))).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Get storage changes (diff) at a specific snapshot.
    /// Returns a map of storage slots to their (old_value, new_value) pairs.
    pub async fn get_storage_diff(
        &self,
        snapshot_id: usize,
    ) -> Result<HashMap<U256, (U256, U256)>> {
        let value = self.call_raw("edb_getStorageDiff", Some(json!([snapshot_id]))).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Evaluate a Solidity expression at a specific snapshot.
    /// Executes the expression in the context of the given execution state.
    pub async fn eval_on_snapshot(
        &self,
        snapshot_id: usize,
        expr: &str,
    ) -> Result<core::result::Result<EdbSolValue, String>> {
        let value = self.call_raw("edb_evalOnSnapshot", Some(json!([snapshot_id, expr]))).await?;
        serde_json::from_value(value).map_err(Into::into)
    }

    /// Get all snapshots where a breakpoint condition would trigger.
    /// Returns snapshot IDs where the breakpoint condition evaluates to true.
    pub async fn get_breakpoint_hits(&self, breakpoint: &Breakpoint) -> Result<Vec<usize>> {
        let value = self.call_raw("edb_getBreakpointHits", Some(json!([breakpoint]))).await?;
        serde_json::from_value(value).map_err(Into::into)
    }
}

/// Comprehensive baseline data structure
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ComprehensiveBaseline {
    /// Metadata about when and how the baseline was captured
    pub metadata: BaselineMetadata,
    /// Transaction-specific baseline data indexed by transaction name
    pub transactions: HashMap<String, TransactionAnalysisResult>,
}

/// Metadata about the baseline capture
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct BaselineMetadata {
    /// ISO timestamp when the baseline capture was performed
    pub capture_timestamp: String,
    /// Total number of transactions captured in this baseline
    pub total_transactions: usize,
    /// Version of EDB used for baseline capture
    pub edb_version: String,
    /// Test environment identifier where baseline was captured
    pub test_environment: String,
    /// Total time taken to capture all baseline data in milliseconds
    pub total_capture_time_ms: u128,
}

/// Baseline data for a single transaction
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionAnalysisResult {
    /// Transaction hash that this baseline represents
    pub tx_hash: String,
    /// RPC server URL used during baseline capture
    pub rpc_url: String,
    /// Basic execution statistics for the transaction
    pub basic_stats: BasicStats,
    /// Analysis of the execution trace structure and content
    pub trace_analysis: TraceAnalysis,
    /// Analysis of snapshot data and distribution
    pub snapshot_analysis: SnapshotAnalysis,
    /// Analysis of contract code and bytecode information
    pub code_analysis: CodeAnalysis,
    /// Analysis of storage slots and state changes
    pub storage_analysis: StorageAnalysis,
    /// Results of expression evaluation testing
    pub expression_evaluation: ExpressionEvaluation,
    /// Analysis of call navigation and flow control
    pub navigation_analysis: NavigationAnalysis,
    /// Analysis of contract ABI and interface information
    pub abi_analysis: AbiAnalysis,
    /// Analysis of breakpoint conditions and hit patterns
    pub breakpoint_analysis: BreakpointAnalysis,
    /// Performance metrics for RPC calls and operations
    pub performance_metrics: PerformanceMetrics,
}

impl PartialEq for TransactionAnalysisResult {
    fn eq(&self, other: &Self) -> bool {
        if self.tx_hash != other.tx_hash {
            error!("Transaction hash mismatch: {} vs {}", self.tx_hash, other.tx_hash);
            return false;
        }

        if self.basic_stats != other.basic_stats {
            error!("Basic stats mismatch");
            return false;
        }

        if self.trace_analysis != other.trace_analysis {
            error!("Trace analysis mismatch");
            return false;
        }

        if self.snapshot_analysis != other.snapshot_analysis {
            error!("Snapshot analysis mismatch");
            return false;
        }

        if self.code_analysis != other.code_analysis {
            error!("Code analysis mismatch");
            return false;
        }

        if self.storage_analysis != other.storage_analysis {
            error!("Storage analysis mismatch");
            return false;
        }

        if self.expression_evaluation != other.expression_evaluation {
            error!("Expression evaluation mismatch");
            return false;
        }

        if self.navigation_analysis != other.navigation_analysis {
            error!("Navigation analysis mismatch");
            return false;
        }

        if self.abi_analysis != other.abi_analysis {
            error!("ABI analysis mismatch");
            return false;
        }

        if self.breakpoint_analysis != other.breakpoint_analysis {
            error!("Breakpoint analysis mismatch");
            return false;
        }

        // Performance metrics are observational and vary across runs, so they are not part of
        // baseline correctness checks.
        true
    }
}

/// Basic statistics about a transaction
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct BasicStats {
    /// Total number of execution snapshots captured
    pub snapshot_count: usize,
    /// Total number of trace entries in the execution
    pub trace_entries: usize,
    /// Whether the transaction executed successfully
    pub success: bool,
}

/// Analysis of trace data
#[derive(Debug, Serialize, Deserialize)]
pub struct TraceAnalysis {
    /// Total number of trace entries captured
    pub total_entries: usize,
    /// JSON representation of the first trace entry
    pub first_entry: Value,
    /// JSON representation of the last trace entry, if available
    pub last_entry: Option<Value>,
    /// Distribution of call types (Call, StaticCall, etc.) and their counts
    pub call_types_distribution: HashMap<String, usize>,
    /// Distribution of call depths and how many calls occurred at each depth
    pub depth_distribution: HashMap<usize, usize>,
    /// Set of unique contract addresses encountered in the trace
    pub unique_addresses: HashSet<String>,
    /// Maximum call depth reached during execution
    pub max_depth: usize,
}

impl PartialEq for TraceAnalysis {
    fn eq(&self, other: &Self) -> bool {
        if self.total_entries != other.total_entries {
            error!(
                "Trace total entries mismatch: {} vs {}",
                self.total_entries, other.total_entries
            );
            return false;
        }

        if self.first_entry != other.first_entry {
            error!("Trace first entry mismatch");
            return false;
        }

        if self.last_entry != other.last_entry {
            error!("Trace last entry mismatch");
            return false;
        }

        if self.call_types_distribution != other.call_types_distribution {
            error!("Trace call types distribution mismatch");
            return false;
        }

        if self.depth_distribution != other.depth_distribution {
            error!("Trace depth distribution mismatch");
            return false;
        }

        if self.unique_addresses != other.unique_addresses {
            error!("Trace unique addresses mismatch");
            return false;
        }

        if self.max_depth != other.max_depth {
            error!("Trace max depth mismatch: {} vs {}", self.max_depth, other.max_depth);
            return false;
        }

        true
    }
}

/// Analysis of snapshot data
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct SnapshotAnalysis {
    /// Total number of snapshots available for analysis
    pub total_count: usize,
    /// Sample snapshot data indexed by snapshot ID for validation
    pub sample_snapshots: HashMap<String, Value>,
    /// Patterns of frame IDs and their occurrence frequencies
    pub frame_id_patterns: HashMap<String, usize>,
}

/// Analysis of contract code
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct CodeAnalysis {
    /// Detailed information about each contract indexed by address
    pub contracts: HashMap<String, ContractCodeInfo>,
    /// Total number of contracts analyzed
    pub total_contracts: usize,
    /// Number of contracts that have verified source code
    pub verified_contracts: usize,
}

/// Information about a contract's code
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ContractCodeInfo {
    /// Contract address as a hex string
    pub address: String,
    /// Type of code representation (Opcode or Source)
    pub code_type: String,
    /// Number of opcodes if available
    pub opcode_count: Option<usize>,
    /// Number of source lines if available
    pub source_count: Option<usize>,
    /// First program counter value
    pub first_pc: Option<usize>,
    /// Last program counter value
    pub last_pc: Option<usize>,
    /// Length of constructor arguments in bytes
    pub constructor_args_length: Option<usize>,
    /// Verification status of the contract
    pub verification_status: String,
}

/// Analysis of storage data
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct StorageAnalysis {
    /// Sample storage slot values indexed by slot number
    pub slot_samples: HashMap<String, String>,
    /// Storage diffs per snapshot showing number of changes
    pub storage_diffs: HashMap<String, usize>,
    /// Set of storage slots that contain non-zero values
    pub non_zero_slots: HashSet<String>,
    /// Total number of storage slots examined
    pub total_slots_checked: usize,
}

/// Analysis of expression evaluation
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct ExpressionEvaluation {
    /// Successful evaluations indexed by snapshot ID and expression
    pub successful_evaluations: HashMap<String, HashMap<String, Value>>,
    /// Failed evaluations with error messages indexed by snapshot ID and expression
    pub failed_evaluations: HashMap<String, HashMap<String, String>>,
    /// Blockchain context values like block number, timestamp, etc.
    pub blockchain_context: HashMap<String, Value>,
    /// Overall success rate as a percentage (0.0 to 1.0)
    pub success_rate: f64,
}

/// Analysis of navigation methods
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct NavigationAnalysis {
    /// Next call navigation results indexed by snapshot ID
    pub next_calls: HashMap<String, Option<usize>>,
    /// Previous call navigation results indexed by snapshot ID
    pub prev_calls: HashMap<String, Option<usize>>,
    /// Maximum depth of the call chain
    pub call_chain_depth: usize,
    /// Number of snapshots that support navigation
    pub navigable_snapshots: usize,
}

/// Analysis of ABI data
#[derive(Debug, Serialize, Deserialize)]
pub struct AbiAnalysis {
    /// Contract ABI information indexed by contract address
    pub contracts_with_abi: HashMap<String, AbiInfo>,
    /// Callable function signatures indexed by contract address
    pub callable_functions: HashMap<String, Vec<CallableAbiInfo>>,
    /// Total number of functions across all contracts
    pub total_functions: usize,
    /// Total number of events across all contracts
    pub total_events: usize,
}

impl PartialEq for AbiAnalysis {
    fn eq(&self, other: &Self) -> bool {
        if self.contracts_with_abi != other.contracts_with_abi {
            error!("ABI contracts mismatch");
            return false;
        }

        // It is hard to do a deep comparison of callable functions due to ordering issues.
        // We will compare the number of functions, their types, and addresses instead.
        if self.callable_functions.len() != other.callable_functions.len() {
            error!("The number of callable functions mismatch");
            return false;
        }
        for (addr, funcs) in &self.callable_functions {
            let Some(other_funcs) = other.callable_functions.get(addr) else {
                error!("Callable functions missing for address: {}", addr);
                return false;
            };

            let mut func_n_sorted = funcs
                .iter()
                .map(|abi| (abi.entries.len(), abi.contract_ty, abi.address))
                .collect::<Vec<_>>();
            func_n_sorted.sort_unstable();
            let mut other_func_n_sorted = other_funcs
                .iter()
                .map(|abi| (abi.entries.len(), abi.contract_ty, abi.address))
                .collect::<Vec<_>>();
            other_func_n_sorted.sort_unstable();

            if func_n_sorted != other_func_n_sorted {
                error!("Callable functions mismatch for address: {}", addr);
                return false;
            }
        }

        if self.total_functions != other.total_functions {
            error!(
                "Total functions mismatch: {} vs {}",
                self.total_functions, other.total_functions
            );
            return false;
        }

        if self.total_events != other.total_events {
            error!("Total events mismatch: {} vs {}", self.total_events, other.total_events);
            return false;
        }

        true
    }
}

/// Information about a contract's ABI
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct AbiInfo {
    /// Number of functions defined in the ABI
    pub functions_count: usize,
    /// Number of events defined in the ABI
    pub events_count: usize,
    /// Whether the contract has a fallback function
    pub has_fallback: bool,
    /// Whether the contract has a receive function
    pub has_receive: bool,
}

/// Analysis of breakpoint behavior
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct BreakpointAnalysis {
    /// Breakpoint condition results indexed by condition string
    pub condition_breakpoints: HashMap<String, usize>,
    /// Total number of breakpoint conditions tested
    pub total_conditions_tested: usize,
    /// Number of conditions that had at least one hit
    pub conditions_with_hits: usize,
}

/// Performance metrics for RPC calls
#[derive(Debug, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// RPC call execution times indexed by method name in milliseconds
    pub rpc_call_times: HashMap<String, u128>,
    /// Response data sizes indexed by method name in bytes
    pub data_sizes: HashMap<String, usize>,
    /// Total number of RPC calls made during analysis
    pub total_rpc_calls: usize,
    /// Total time spent on analysis in milliseconds
    pub analysis_time_ms: u128,
}

impl PartialEq for PerformanceMetrics {
    fn eq(&self, other: &Self) -> bool {
        // Exclude analysis_time_ms and rpc_call_times from equality check
        self.data_sizes == other.data_sizes && self.total_rpc_calls == other.total_rpc_calls
    }
}

/// Load baseline data from captured JSON files
pub struct BaselineLoader {
    baseline_dir: PathBuf,
}

impl BaselineLoader {
    /// Create a new baseline loader
    pub fn new() -> Self {
        let baseline_dir = paths::get_baseline_dir();
        Self { baseline_dir }
    }

    /// Load baseline data for a specific transaction
    pub fn load_transaction_baseline(&self, tx_name: &str) -> Result<Value> {
        let file_path = self.baseline_dir.join(format!("{tx_name}_baseline.json"));
        let content = fs::read_to_string(&file_path)?;
        let baseline: Value = serde_json::from_str(&content)?;
        Ok(baseline)
    }

    /// Check if baseline files exist
    pub fn baseline_files_exist(&self) -> bool {
        Path::new(&self.baseline_dir).exists()
    }

    /// Load transaction baseline data as TransactionBaseline for direct comparison
    pub fn load_comprehensive_analysis_result(
        &self,
        tx_name: &str,
    ) -> Result<TransactionAnalysisResult> {
        let baseline = self.load_transaction_baseline(tx_name)?;
        info!("Loaded baseline for transaction '{tx_name}'");

        let result: TransactionAnalysisResult = serde_json::from_value(baseline)?;
        Ok(result)
    }
}

impl Default for BaselineLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Analyze a transaction comprehensively and return the analysis result
pub async fn analyze_transaction_comprehensive(
    fixture: &EngineFixture,
) -> TransactionAnalysisResult {
    let tx_start = Instant::now();
    let client = RpcTestClient::new(&fixture.rpc_url);
    let mut perf_metrics = PerformanceMetrics {
        rpc_call_times: HashMap::new(),
        data_sizes: HashMap::new(),
        total_rpc_calls: 0,
        analysis_time_ms: 0,
    };

    info!("📊 Analyzing basic stats...");
    let basic_stats = analyze_basic_stats(&client, &mut perf_metrics).await;

    info!("🔍 Analyzing trace...");
    let trace_analysis = analyze_trace_comprehensive(&client, &mut perf_metrics).await;

    info!("📸 Analyzing snapshots...");
    let snapshot_analysis =
        analyze_snapshots_comprehensive(&client, basic_stats.snapshot_count, &mut perf_metrics)
            .await;

    info!("💻 Analyzing code...");
    let code_analysis = analyze_code_comprehensive(&client, &mut perf_metrics).await;

    info!("💾 Analyzing storage...");
    let storage_analysis =
        analyze_storage_comprehensive(&client, basic_stats.snapshot_count, &mut perf_metrics).await;

    info!("🧮 Analyzing expressions...");
    let expression_evaluation =
        analyze_expressions_comprehensive(&client, basic_stats.snapshot_count, &mut perf_metrics)
            .await;

    info!("🧭 Analyzing navigation...");
    let navigation_analysis =
        analyze_navigation_comprehensive(&client, basic_stats.snapshot_count, &mut perf_metrics)
            .await;

    info!("📋 Analyzing ABIs...");
    let abi_analysis = analyze_abi_comprehensive(&client, &code_analysis, &mut perf_metrics).await;

    info!("🎯 Analyzing breakpoints...");
    let breakpoint_analysis = analyze_breakpoints_comprehensive(&client, &mut perf_metrics).await;

    perf_metrics.analysis_time_ms = tx_start.elapsed().as_millis();

    TransactionAnalysisResult {
        tx_hash: fixture.tx_hash.clone(),
        rpc_url: fixture.rpc_url.clone(),
        basic_stats,
        trace_analysis,
        snapshot_analysis,
        code_analysis,
        storage_analysis,
        expression_evaluation,
        navigation_analysis,
        abi_analysis,
        breakpoint_analysis,
        performance_metrics: perf_metrics,
    }
}

async fn analyze_basic_stats(client: &RpcTestClient, perf: &mut PerformanceMetrics) -> BasicStats {
    let start = Instant::now();
    let snapshot_count = client.get_snapshot_count().await.unwrap();
    record_rpc_call(perf, "edb_getSnapshotCount", start, 8);

    let start = Instant::now();
    let trace = client.get_trace().await.unwrap();
    record_rpc_call(perf, "edb_getTrace", start, serde_json::to_string(&trace).unwrap().len());

    BasicStats {
        snapshot_count,
        trace_entries: trace.len(),
        success: snapshot_count > 0 && !trace.is_empty(),
    }
}

async fn analyze_trace_comprehensive(
    client: &RpcTestClient,
    perf: &mut PerformanceMetrics,
) -> TraceAnalysis {
    let start = Instant::now();
    let trace = client.get_trace().await.unwrap();
    record_rpc_call(
        perf,
        "edb_getTrace_detailed",
        start,
        serde_json::to_string(&trace).unwrap().len(),
    );

    if trace.is_empty() {
        return TraceAnalysis {
            total_entries: 0,
            first_entry: json!(null),
            last_entry: None,
            call_types_distribution: HashMap::new(),
            depth_distribution: HashMap::new(),
            unique_addresses: HashSet::new(),
            max_depth: 0,
        };
    }

    let mut call_types = HashMap::new();
    let mut depths = HashMap::new();
    let mut addresses = HashSet::new();
    let mut max_depth = 0;

    for entry in trace.iter() {
        // Count call types
        let call_type_str = format!("{:?}", entry.call_type);
        *call_types.entry(call_type_str).or_insert(0) += 1;

        // Count depths
        *depths.entry(entry.depth).or_insert(0) += 1;
        max_depth = max_depth.max(entry.depth);

        // Collect unique addresses
        addresses.insert(format!("{:?}", entry.caller));
        addresses.insert(format!("{:?}", entry.target));
        addresses.insert(format!("{:?}", entry.code_address));
    }

    let first_entry = serde_json::to_value(&trace[0]).unwrap();
    let last_entry = if trace.len() > 1 {
        Some(serde_json::to_value(&trace[trace.len() - 1]).unwrap())
    } else {
        None
    };

    TraceAnalysis {
        total_entries: trace.len(),
        first_entry,
        last_entry,
        call_types_distribution: call_types,
        depth_distribution: depths,
        unique_addresses: addresses.into_iter().collect(),
        max_depth,
    }
}

async fn analyze_snapshots_comprehensive(
    client: &RpcTestClient,
    snapshot_count: usize,
    perf: &mut PerformanceMetrics,
) -> SnapshotAnalysis {
    let mut sample_snapshots = HashMap::new();
    let mut frame_patterns = HashMap::new();

    if snapshot_count == 0 {
        return SnapshotAnalysis {
            total_count: 0,
            sample_snapshots,
            frame_id_patterns: frame_patterns,
        };
    }

    // Sample key snapshots
    let sample_indices = vec![
        ("first", 0),
        ("early", snapshot_count.min(10)),
        ("quarter", snapshot_count / 4),
        ("half", snapshot_count / 2),
        ("three_quarter", (snapshot_count * 3) / 4),
        ("late", snapshot_count.saturating_sub(10)),
        ("last", snapshot_count - 1),
    ];

    for (label, idx) in sample_indices {
        if idx < snapshot_count {
            let start = Instant::now();
            let snapshot = client.get_snapshot_info(idx).await.unwrap();
            let snapshot_json = serde_json::to_value(&snapshot).unwrap();
            sample_snapshots.insert(format!("{label}_{idx}"), snapshot_json);

            // Track frame patterns
            let frame_pattern = format!("{}.{}", snapshot.frame_id.0, snapshot.frame_id.1);
            *frame_patterns.entry(frame_pattern).or_insert(0) += 1;

            record_rpc_call(
                perf,
                "edb_getSnapshotInfo",
                start,
                serde_json::to_string(&snapshot).unwrap().len(),
            );
        }
    }

    SnapshotAnalysis {
        total_count: snapshot_count,
        sample_snapshots,
        frame_id_patterns: frame_patterns,
    }
}

async fn analyze_code_comprehensive(
    client: &RpcTestClient,
    perf: &mut PerformanceMetrics,
) -> CodeAnalysis {
    let mut contracts = HashMap::new();
    let mut verified_count = 0;

    // Get code at snapshot 0 to find main contract
    let start = Instant::now();
    let code = client.get_code(0).await.unwrap();
    let address = code.bytecode_address();
    let addr_str = format!("{address:?}");

    let contract_info = analyze_contract_code(client, address, &code, perf).await;
    if contract_info.verification_status == "verified" {
        verified_count += 1;
    }
    contracts.insert(addr_str, contract_info);

    record_rpc_call(perf, "edb_getCode", start, serde_json::to_string(&code).unwrap().len());

    CodeAnalysis { total_contracts: contracts.len(), verified_contracts: verified_count, contracts }
}

async fn analyze_contract_code(
    client: &RpcTestClient,
    address: Address,
    code: &Code,
    perf: &mut PerformanceMetrics,
) -> ContractCodeInfo {
    let addr_str = format!("{address:?}");

    let (code_type, opcode_count, source_count, first_pc, last_pc) = match code {
        Code::Opcode(info) => {
            let first_pc = info.codes.keys().min().copied();
            let last_pc = info.codes.keys().max().copied();
            ("Opcode".to_string(), Some(info.codes.len()), None, first_pc, last_pc)
        }
        Code::Source(info) => ("Source".to_string(), None, Some(info.sources.len()), None, None),
    };

    // Check constructor args
    let start = Instant::now();
    let constructor_args_length = match client.get_constructor_args(address).await {
        Ok(Some(args)) => Some(args.len()),
        Ok(None) => Some(0),
        Err(_) => None,
    };
    record_rpc_call(perf, "edb_getConstructorArgs", start, 32);

    ContractCodeInfo {
        address: addr_str,
        code_type,
        opcode_count,
        source_count,
        first_pc,
        last_pc,
        constructor_args_length,
        verification_status: if source_count.is_some() && source_count.unwrap() > 0 {
            "verified".to_string()
        } else {
            "unverified".to_string()
        },
    }
}

async fn analyze_storage_comprehensive(
    client: &RpcTestClient,
    snapshot_count: usize,
    perf: &mut PerformanceMetrics,
) -> StorageAnalysis {
    let mut slot_samples = HashMap::new();
    let mut storage_diffs = HashMap::new();
    let mut non_zero_slots = HashSet::new();
    let total_slots_checked = 20; // Check first 20 slots

    if snapshot_count == 0 {
        return StorageAnalysis {
            slot_samples,
            storage_diffs,
            non_zero_slots,
            total_slots_checked: 0,
        };
    }

    // Sample storage slots at snapshot 0
    for i in 0..total_slots_checked {
        let slot = U256::from(i);
        let start = Instant::now();
        let value = client.get_storage(0, slot).await.unwrap();
        let value_str = format!("{value:?}");
        slot_samples.insert(format!("slot_{i}"), value_str.clone());

        if value != U256::ZERO {
            non_zero_slots.insert(format!("slot_{i}"));
        }

        record_rpc_call(perf, "edb_getStorage", start, 32);
    }

    // Check storage diffs at various snapshots
    let diff_snapshots =
        vec![snapshot_count / 4, snapshot_count / 2, (snapshot_count * 3) / 4, snapshot_count - 1];

    for snapshot_idx in diff_snapshots {
        if snapshot_idx < snapshot_count {
            let start = Instant::now();
            let diff = client.get_storage_diff(snapshot_idx).await.unwrap();
            storage_diffs.insert(format!("snapshot_{snapshot_idx}"), diff.len());
            record_rpc_call(
                perf,
                "edb_getStorageDiff",
                start,
                serde_json::to_string(&diff).unwrap().len(),
            );
        }
    }

    StorageAnalysis { slot_samples, storage_diffs, non_zero_slots, total_slots_checked }
}

async fn analyze_expressions_comprehensive(
    client: &RpcTestClient,
    snapshot_count: usize,
    perf: &mut PerformanceMetrics,
) -> ExpressionEvaluation {
    let mut successful_evaluations = HashMap::new();
    let mut failed_evaluations = HashMap::new();
    let mut blockchain_context = HashMap::new();

    if snapshot_count == 0 {
        return ExpressionEvaluation {
            successful_evaluations,
            failed_evaluations,
            blockchain_context,
            success_rate: 0.0,
        };
    }

    let test_expressions = vec![
        "1 + 1",
        "2 * 3",
        "msg.sender",
        "msg.value",
        "msg.data.length",
        "block.number",
        "block.timestamp",
        "block.difficulty",
        "tx.gasprice",
        "tx.origin",
        "address(this)",
        "gasleft()",
    ];

    let test_snapshots = vec![0, snapshot_count / 2];
    let mut total_evaluations = 0;
    let mut successful_count = 0;

    for snap_idx in test_snapshots {
        if snap_idx >= snapshot_count {
            continue;
        }

        let snap_key = format!("snapshot_{snap_idx}");
        let mut snap_success = HashMap::new();
        let mut snap_failed = HashMap::new();

        for expr in &test_expressions {
            let start = Instant::now();
            total_evaluations += 1;

            match client.eval_on_snapshot(snap_idx, expr).await {
                Ok(result) => {
                    match result {
                        Ok(value) => {
                            successful_count += 1;
                            let value_json = json!(format!("{:?}", value));
                            snap_success.insert(expr.to_string(), value_json.clone());

                            // Store important blockchain context
                            if snap_idx == 0 {
                                match expr {
                                    &"msg.sender" | &"block.number" | &"tx.gasprice" => {
                                        blockchain_context.insert(expr.to_string(), value_json);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(err) => {
                            snap_failed.insert(expr.to_string(), err);
                        }
                    }
                }
                Err(e) => {
                    snap_failed.insert(expr.to_string(), format!("RPC Error: {e}"));
                }
            }

            record_rpc_call(perf, "edb_evalOnSnapshot", start, 64);
        }

        if !snap_success.is_empty() {
            successful_evaluations.insert(snap_key.clone(), snap_success);
        }
        if !snap_failed.is_empty() {
            failed_evaluations.insert(snap_key, snap_failed);
        }
    }

    let success_rate = if total_evaluations > 0 {
        successful_count as f64 / total_evaluations as f64
    } else {
        0.0
    };

    ExpressionEvaluation {
        successful_evaluations,
        failed_evaluations,
        blockchain_context,
        success_rate,
    }
}

async fn analyze_navigation_comprehensive(
    client: &RpcTestClient,
    snapshot_count: usize,
    perf: &mut PerformanceMetrics,
) -> NavigationAnalysis {
    let mut next_calls = HashMap::new();
    let mut prev_calls = HashMap::new();
    let mut navigable_count = 0;

    if snapshot_count == 0 {
        return NavigationAnalysis {
            next_calls,
            prev_calls,
            call_chain_depth: 0,
            navigable_snapshots: 0,
        };
    }

    let test_snapshots = vec![
        0,
        snapshot_count / 8,
        snapshot_count / 4,
        snapshot_count / 2,
        (snapshot_count * 3) / 4,
        snapshot_count.saturating_sub(10),
    ];

    for snap_idx in test_snapshots {
        if snap_idx >= snapshot_count {
            continue;
        }

        let snap_key = format!("snapshot_{snap_idx}");

        // Test next call
        let start = Instant::now();
        match client.get_next_call(snap_idx).await {
            Ok(next_idx) => {
                next_calls.insert(snap_key.clone(), Some(next_idx));
                navigable_count += 1;
            }
            Err(_) => {
                next_calls.insert(snap_key.clone(), None);
            }
        }
        record_rpc_call(perf, "edb_getNextCall", start, 8);

        // Test prev call
        let start = Instant::now();
        match client.get_prev_call(snap_idx).await {
            Ok(prev_idx) => {
                prev_calls.insert(snap_key, Some(prev_idx));
            }
            Err(_) => {
                prev_calls.insert(snap_key, None);
            }
        }
        record_rpc_call(perf, "edb_getPrevCall", start, 8);
    }

    NavigationAnalysis {
        next_calls,
        prev_calls,
        call_chain_depth: snapshot_count / 10, // Rough estimate
        navigable_snapshots: navigable_count,
    }
}

async fn analyze_abi_comprehensive(
    client: &RpcTestClient,
    code_analysis: &CodeAnalysis,
    perf: &mut PerformanceMetrics,
) -> AbiAnalysis {
    let mut contracts_with_abi = HashMap::new();
    let mut callable_functions = HashMap::new();
    let mut total_functions = 0;
    let mut total_events = 0;

    for addr_str in code_analysis.contracts.keys() {
        let address = addr_str.parse::<Address>().unwrap();
        // Test contract ABI
        let start = Instant::now();
        if let Some(abi) = client.get_contract_abi(address, false).await.unwrap() {
            let abi_info = AbiInfo {
                functions_count: abi.functions.len(),
                events_count: abi.events.len(),
                has_fallback: abi.fallback.is_some(),
                has_receive: abi.receive.is_some(),
            };

            total_functions += abi_info.functions_count;
            total_events += abi_info.events_count;

            contracts_with_abi.insert(addr_str.clone(), abi_info);

            record_rpc_call(
                perf,
                "edb_getContractABI",
                start,
                serde_json::to_string(&abi).unwrap().len(),
            );
        }

        // Test callable ABI
        let start = Instant::now();
        let callable: Vec<CallableAbiInfo> = client.get_callable_abi(address).await.unwrap();

        record_rpc_call(
            perf,
            "edb_getCallableABI",
            start,
            serde_json::to_string(&callable).unwrap().len(),
        );

        if !callable.is_empty() {
            callable_functions.insert(addr_str.clone(), callable.into_iter().collect());
        }
    }

    AbiAnalysis { contracts_with_abi, callable_functions, total_functions, total_events }
}

async fn analyze_breakpoints_comprehensive(
    client: &RpcTestClient,
    perf: &mut PerformanceMetrics,
) -> BreakpointAnalysis {
    let mut condition_breakpoints = HashMap::new();
    let total_conditions = 6;
    let mut conditions_with_hits = 0;

    let test_conditions = vec![
        "msg.sender != address(0)",
        "msg.value > 0",
        "msg.value == 0",
        "block.number > 0",
        "tx.gasprice > 1000000000", // > 1 gwei
        "gasleft() > 21000",
    ];

    for condition in test_conditions {
        let breakpoint = Breakpoint { loc: None, condition: Some(condition.to_string()) };

        let start = Instant::now();
        match client.get_breakpoint_hits(&breakpoint).await {
            Ok(hits) => {
                condition_breakpoints.insert(condition.to_string(), hits.len());
                if !hits.is_empty() {
                    conditions_with_hits += 1;
                }
            }
            Err(_) => {
                condition_breakpoints.insert(condition.to_string(), 0);
            }
        }
        record_rpc_call(perf, "edb_getBreakpointHits", start, 32);
    }

    BreakpointAnalysis {
        condition_breakpoints,
        total_conditions_tested: total_conditions,
        conditions_with_hits,
    }
}

fn record_rpc_call(perf: &mut PerformanceMetrics, method: &str, start: Instant, data_size: usize) {
    let elapsed = start.elapsed().as_millis();
    perf.rpc_call_times.insert(method.to_string(), elapsed);
    perf.data_sizes.insert(method.to_string(), data_size);
    perf.total_rpc_calls += 1;
}

/// Create a summary report from the comprehensive baseline data
pub fn create_summary(baseline: &ComprehensiveBaseline) -> Value {
    let mut summary = json!({
        "metadata": baseline.metadata,
        "overview": {
            "total_transactions": baseline.transactions.len(),
            "successful_transactions": 0,
            "total_snapshots": 0,
            "total_trace_entries": 0,
            "total_contracts": 0,
            "verified_contracts": 0,
        },
        "performance": {
            "avg_analysis_time_ms": 0,
            "total_rpc_calls": 0,
            "most_expensive_rpc": "",
        },
        "transaction_summaries": {}
    });

    let mut total_snapshots = 0;
    let mut total_trace_entries = 0;
    let mut total_contracts = 0;
    let mut verified_contracts = 0;
    let mut successful_transactions = 0;
    let mut total_analysis_time = 0u128;
    let mut total_rpc_calls = 0;
    let mut max_rpc_time = 0u128;
    let mut most_expensive_rpc = String::new();

    for (tx_name, tx_baseline) in &baseline.transactions {
        if tx_baseline.basic_stats.success {
            successful_transactions += 1;
        }

        total_snapshots += tx_baseline.basic_stats.snapshot_count;
        total_trace_entries += tx_baseline.basic_stats.trace_entries;
        total_contracts += tx_baseline.code_analysis.total_contracts;
        verified_contracts += tx_baseline.code_analysis.verified_contracts;
        total_analysis_time += tx_baseline.performance_metrics.analysis_time_ms;
        total_rpc_calls += tx_baseline.performance_metrics.total_rpc_calls;

        // Find most expensive RPC call
        for (method, time) in &tx_baseline.performance_metrics.rpc_call_times {
            if *time > max_rpc_time {
                max_rpc_time = *time;
                most_expensive_rpc = method.clone();
            }
        }

        // Add transaction summary
        summary["transaction_summaries"][tx_name] = json!({
            "snapshots": tx_baseline.basic_stats.snapshot_count,
            "trace_entries": tx_baseline.basic_stats.trace_entries,
            "contracts": tx_baseline.code_analysis.total_contracts,
            "success": tx_baseline.basic_stats.success,
            "analysis_time_ms": tx_baseline.performance_metrics.analysis_time_ms,
        });
    }

    summary["overview"]["successful_transactions"] = json!(successful_transactions);
    summary["overview"]["total_snapshots"] = json!(total_snapshots);
    summary["overview"]["total_trace_entries"] = json!(total_trace_entries);
    summary["overview"]["total_contracts"] = json!(total_contracts);
    summary["overview"]["verified_contracts"] = json!(verified_contracts);

    summary["performance"]["avg_analysis_time_ms"] = json!(if !baseline.transactions.is_empty() {
        total_analysis_time / baseline.transactions.len() as u128
    } else {
        0
    });
    summary["performance"]["total_rpc_calls"] = json!(total_rpc_calls);
    summary["performance"]["most_expensive_rpc"] = json!(most_expensive_rpc);

    summary
}
