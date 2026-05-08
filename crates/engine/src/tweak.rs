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

//! Contract bytecode modification for debugging through creation transaction replay.
//!
//! This module provides the [`CodeTweaker`] utility for modifying deployed contract bytecode
//! by replaying their creation transactions with replacement init code. This enables debugging
//! with instrumented or modified contracts without requiring redeployment to the network.
//!
//! # Core Functionality
//!
//! ## Contract Bytecode Replacement
//! The [`CodeTweaker`] handles the complete process of:
//! 1. **Creation Transaction Discovery**: Finding the original deployment transaction
//! 2. **Transaction Replay**: Re-executing the creation with modified init code
//! 3. **Bytecode Extraction**: Capturing the resulting runtime bytecode
//! 4. **State Update**: Replacing the deployed bytecode in the debugging database
//!
//! ## Etherscan Integration
//! - **Creation Data Caching**: Local caching of contract creation transaction data
//! - **API Key Management**: Automatic API key rotation for rate limit handling
//! - **Chain Support**: Multi-chain support through configurable Etherscan endpoints
//!
//! # Workflow Integration
//!
//! The code tweaking process is typically used in the debugging workflow to:
//! 1. Replace original contracts with instrumented versions for hook-based debugging
//! 2. Substitute contracts with modified versions for testing different scenarios
//! 3. Enable debugging of contracts that weren't originally compiled with debug information
//!
//! # Usage Example
//!
//! ```rust,ignore
//! let mut tweaker = CodeTweaker::new(&mut edb_context, rpc_url, etherscan_api_key);
//! tweaker.tweak(&contract_address, &original_artifact, &instrumented_artifact, false).await?;
//! ```
//!
//! This replaces the deployed bytecode at `contract_address` with the instrumented version,
//! enabling advanced debugging features on the modified contract.

use std::env;

use alloy_primitives::{Address, Bytes, TxHash};
use edb_common::{
    Cache, CachePath, EdbCache, EdbCachePath, EdbContext, ForkResult, fork_and_prepare,
    relax_evm_constraints,
};
use eyre::Result;
use foundry_block_explorers::{Client, contract::ContractCreationData};
use revm::{
    Database, DatabaseCommit, DatabaseRef, InspectEvm, MainBuilder,
    context::{Cfg, ContextTr},
    database::CacheDB,
    primitives::KECCAK_EMPTY,
    state::Bytecode,
};
use tracing::{debug, error};

use crate::{Artifact, TweakInspector, next_etherscan_api_key};

/// Utility for modifying deployed contract bytecode through creation transaction replay.
///
/// The [`CodeTweaker`] enables replacing deployed contract bytecode by:
/// 1. Finding the original contract creation transaction
/// 2. Replaying that transaction with modified init code from recompiled artifacts
/// 3. Extracting the resulting runtime bytecode
/// 4. Updating the contract's bytecode in the debugging database
///
/// This allows debugging with instrumented contracts without requiring network redeployment.
pub struct CodeTweaker<'a, DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    ctx: &'a mut EdbContext<DB>,
    rpc_url: String,
    etherscan_api_key: Option<String>,
}

impl<'a, DB> CodeTweaker<'a, DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Creates a new `CodeTweaker` instance.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Mutable reference to the EDB context containing the database
    /// * `rpc_url` - RPC endpoint URL for fetching blockchain data
    /// * `etherscan_api_key` - Optional Etherscan API key for fetching contract creation data
    pub fn new(
        ctx: &'a mut EdbContext<DB>,
        rpc_url: String,
        etherscan_api_key: Option<String>,
    ) -> Self {
        Self { ctx, rpc_url, etherscan_api_key }
    }

    /// Replaces deployed contract bytecode with instrumented bytecode from artifacts.
    ///
    /// This method performs the complete bytecode replacement workflow:
    /// 1. Finds the contract creation transaction using Etherscan API
    /// 2. Replays the transaction with the recompiled artifact's init code
    /// 3. Extracts the resulting runtime bytecode
    /// 4. Updates the contract's bytecode in the debugging database
    ///
    /// # Arguments
    ///
    /// * `addr` - Address of the deployed contract to modify
    /// * `artifact` - Original compiled artifact for constructor argument extraction
    /// * `recompiled_artifact` - Recompiled artifact containing the replacement init code
    /// * `quick` - Whether to use quick mode (faster but potentially less accurate)
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the bytecode replacement succeeds, or an error if any step fails.
    pub async fn tweak(
        &mut self,
        addr: &Address,
        artifact: &Artifact,
        recompiled_artifact: &Artifact,
        quick: bool,
    ) -> Result<()> {
        let tweaked_code =
            self.get_tweaked_code(addr, artifact, recompiled_artifact, quick).await?;
        if tweaked_code.is_empty() {
            error!(addr=?addr, quick=?quick, "Tweaked code is empty");
        }

        let db = self.ctx.db_mut();

        let mut info = db
            .basic(*addr)
            .map_err(|e| eyre::eyre!("Failed to get account info for {}: {}", addr, e))?
            .unwrap_or_default();
        // Code hash will be update within `db.insert_account_info(&mut info);`
        info.code_hash = KECCAK_EMPTY;
        info.code = Some(Bytecode::new_raw(tweaked_code));
        db.insert_account_info(*addr, info);

        Ok(())
    }

    /// Generate the tweaked runtime bytecode by replaying the creation transaction.
    ///
    /// This internal method handles the complex process of:
    /// 1. Forking the blockchain state at the creation transaction
    /// 2. Setting up the replay environment with modified constraints
    /// 3. Using the TweakInspector to intercept and modify the deployment
    /// 4. Extracting the resulting runtime bytecode
    async fn get_tweaked_code(
        &self,
        addr: &Address,
        artifact: &Artifact,
        recompiled_artifact: &Artifact,
        quick: bool,
    ) -> Result<Bytes> {
        let creation_tx_hash = self.get_creation_tx(addr).await?;
        debug!("Creation tx: {} -> {}", creation_tx_hash, addr);

        // Create replay environment
        let ForkResult { context: mut replay_ctx, target_tx_env: mut creation_tx_env, .. } =
            fork_and_prepare(&self.rpc_url, creation_tx_hash, quick).await?;
        relax_evm_constraints(&mut replay_ctx, &mut creation_tx_env);

        // Get init code
        let contract = artifact.contract().ok_or(eyre::eyre!("Failed to get contract"))?;

        let recompiled_contract =
            recompiled_artifact.contract().ok_or(eyre::eyre!("Failed to get contract"))?;

        let constructor_args = recompiled_artifact.constructor_arguments();

        let mut inspector =
            TweakInspector::new(*addr, contract, recompiled_contract, constructor_args);

        let mut evm = replay_ctx.build_mainnet_with_inspector(&mut inspector);

        evm.inspect_one_tx(creation_tx_env)
            .map_err(|e| eyre::eyre!("Failed to inspect the target transaction: {:?}", e))?;

        inspector.into_deployed_code()
    }

    /// Retrieves the transaction hash that created a contract at the given address.
    ///
    /// This method first checks the local cache for the creation transaction data.
    /// If not cached, it queries Etherscan API and caches the result for future use.
    /// Note that Etherscan Client will NOT cache the result itself.
    ///
    /// # Arguments
    ///
    /// * `addr` - Address of the deployed contract
    ///
    /// # Returns
    ///
    /// Returns the transaction hash that deployed the contract, or an error if not found.
    pub async fn get_creation_tx(&self, addr: &Address) -> Result<TxHash> {
        let chain_id = self.ctx.cfg().chain_id();

        // Cache directory
        let etherscan_cache_dir = EdbCachePath::new(env::var(edb_common::env::EDB_CACHE_DIR).ok())
            .etherscan_chain_cache_dir(chain_id)
            .map(|p| p.join("contract_creation_txs"));

        let cache = EdbCache::<ContractCreationData>::new(etherscan_cache_dir, None)?;
        let label = addr.to_string();

        if let Some(creation_data) = cache.load_cache(&label) {
            Ok(creation_data.transaction_hash)
        } else {
            let etherscan_api_key =
                self.etherscan_api_key.clone().unwrap_or(next_etherscan_api_key());

            // Build client
            let etherscan = Client::builder()
                .with_api_key(etherscan_api_key)
                .chain(chain_id.into())?
                .build()?;

            // Get creation tx
            let creation_data = etherscan.contract_creation_data(*addr).await?;
            cache.save_cache(&label, &creation_data)?;
            Ok(creation_data.transaction_hash)
        }
    }
}
