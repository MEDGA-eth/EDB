use std::sync::Arc;

use alloy_primitives::TxHash;
use alloy_provider::Provider;
use alloy_rpc_types::{BlockTransactions, BlockTransactionsKind};
use clap::Parser;
use edb_debug_backend::DebugBackend;
use eyre::{ensure, eyre, Result};
use foundry_block_explorers::Client;
use foundry_common::{is_known_system_sender, SYSTEM_TRANSACTION_TYPE};
use foundry_evm::{
    fork::database::ForkedDatabase,
    utils::{configure_tx_env, new_evm_with_inspector},
};
use revm::{inspectors::NoOpInspector, primitives::EnvWithHandlerCfg};

use crate::{
    init_progress, opts::{EtherscanOpts, RpcOpts}, update_progress, utils::evm::{setup_block_env, setup_fork_db}
};


/// CLI arguments for `edb replay`.
#[derive(Clone, Debug, Parser)]
pub struct ReplayArgs {
    /// The hash of the transaction under replay.
    pub tx_hash: TxHash,

    /// Executes the transaction only with the state from the previous block.
    /// Note that this also include transactions that are used for tweaking code.
    ///
    /// May result in different results than the live execution!
    #[arg(long, short)]
    pub quick: bool,

    /// Skips validation of transactions replayed before the target transaction.
    #[arg(long, short)]
    pub no_validation: bool,

    #[command(flatten)]
    pub etherscan: EtherscanOpts,

    #[command(flatten)]
    pub rpc: RpcOpts,
}

impl ReplayArgs {
    pub async fn run(mut self) -> Result<()> {
        if self.quick {
            // enforce no validation when quick is enabled
            self.no_validation = true;
        }

        let (db, env) = self.prepare().await?;
        self.debug(db, env, true).await?;
        Ok(())
    }

    pub async fn debug(
        &self,
        db: ForkedDatabase,
        env: EnvWithHandlerCfg,
        enable_ui: bool,
    ) -> Result<()> {
        let mut backend = DebugBackend::<ForkedDatabase>::builder()
            .chain(self.etherscan.chain.unwrap_or_default())
            .etherscan_api_key(self.etherscan.key().unwrap_or_default())
            .build::<ForkedDatabase>()?;
        let artifact = backend.debug(db, env).await?;
        Ok(())
    }

    pub async fn prepare(&self) -> Result<(ForkedDatabase, EnvWithHandlerCfg)> {
        let Self { tx_hash, quick, rpc, no_validation, etherscan } = self;
        let fork_url = rpc.url(true)?.unwrap().to_string();

        // step 0. prepare rpc provider
        let chain = etherscan.chain.unwrap_or_default();
        let etherscan_api_key = etherscan.key().unwrap_or_default();
        let _client = Client::new(chain, etherscan_api_key)?;

        let compute_units_per_second =
            if rpc.no_rate_limit { Some(u64::MAX) } else { rpc.compute_units_per_second };
        let mut provider_builder = foundry_common::provider::ProviderBuilder::new(&fork_url)
            .compute_units_per_second_opt(compute_units_per_second);
        if let Some(jwt) = rpc.jwt_secret.as_deref() {
            provider_builder = provider_builder.jwt(jwt);
        }
        let provider = Arc::new(provider_builder.build()?);

        // step 1. get the transaction and block data
        let tx = provider
            .get_transaction_by_hash(*tx_hash)
            .await?
            .ok_or(eyre!("transaction not found"))?;
        let tx_block_number: u64 =
            tx.block_number.ok_or(eyre!("transaction may still be pending"))?;
        let block = provider
            .get_block(tx_block_number.into(), BlockTransactionsKind::Full)
            .await?
            .ok_or(eyre!("block not found"))?;
        let BlockTransactions::Full(txs_in_block) = block.transactions else {
            return Err(eyre::eyre!("block transactions not found"));
        };

        // step 2. set enviroment and database
        // note that database should be set to tx_block_number - 1
        let mut db =
            setup_fork_db(Arc::clone(&provider), &fork_url, Some(tx_block_number - 1)).await?;
        let mut env = setup_block_env(Arc::clone(&provider), Some(tx_block_number)).await?;

        // step 3. replay all transactions before the target transaction
        // we use cumulative_gas_used as a quick validator for the correctness of the replay
        let mut cumulative_gas_used = 0u128;
        // prepare txs
        let mut txs = vec![];
        if !quick {
            txs.extend(txs_in_block.into_iter().take_while(|tx| &tx.hash != tx_hash));
        };
        txs.push(tx.inner.clone());

        let pb = init_progress!(txs, "replaying preceeding txs");
        pb.set_position(0);
        for (index, tx) in txs.into_iter().enumerate() {
            update_progress!(pb, index);

            // System transactions such as on L2s don't contain any pricing info so
            // we skip them otherwise this would cause
            // reverts
            if is_known_system_sender(tx.from) ||
                tx.transaction_type == Some(SYSTEM_TRANSACTION_TYPE)
            {
                continue;
            }

            // execute the transaction
            trace!("Executing transaction: {:?}", tx.hash);
            configure_tx_env(&mut env, &tx);
            let mut evm = new_evm_with_inspector(&mut db, env.clone(), NoOpInspector);
            let result = if &tx.hash == tx_hash {
                // we don't commit the target transaction
                evm.transact()?.result
            } else {
                evm.transact_commit()?
            };

            let tx_receipt = provider
                .get_transaction_receipt(tx.hash)
                .await?
                .ok_or(eyre!("transaction receipt not found"))?;

            cumulative_gas_used += result.gas_used() as u128;
            ensure!(
                *no_validation ||
                    cumulative_gas_used ==
                        tx_receipt.inner.inner.inner.receipt.cumulative_gas_used,
                "gas used mismatch ({:?}): {} vs {}",
                tx.hash,
                cumulative_gas_used,
                tx_receipt.inner.inner.inner.receipt.cumulative_gas_used
            );
        }

        Ok((db, env))
    }
}
