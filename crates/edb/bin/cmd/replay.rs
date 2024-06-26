use std::sync::Arc;

use alloy_primitives::{TxHash, U256};
use alloy_provider::Provider;
use alloy_rpc_types::{BlockTransactions, BlockTransactionsKind};
use clap::Parser;
use edb_debug_backend::DebugBackend;
use edb_debug_frontend::DebugFrontend;
use edb_utils::{init_progress, update_progress};
use eyre::{ensure, eyre, Result};
use foundry_common::{is_known_system_sender, SYSTEM_TRANSACTION_TYPE};
use foundry_evm::{fork::database::ForkedDatabase, utils::new_evm_with_inspector};
use revm::{inspectors::NoOpInspector, primitives::EnvWithHandlerCfg};

use crate::{
    opts::{EtherscanOpts, RpcOpts},
    utils::evm::{fill_tx_env, setup_block_env, setup_fork_db},
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
        self.debug(db, env).await?;
        Ok(())
    }

    pub async fn debug(&self, db: ForkedDatabase, env: EnvWithHandlerCfg) -> Result<()> {
        let backend = DebugBackend::<ForkedDatabase>::builder()
            .chain(self.etherscan.chain.unwrap_or_default())
            .etherscan_api_key(self.etherscan.key().unwrap_or_default())
            .build::<ForkedDatabase>(&db, env)?;
        let mut frontend = DebugFrontend::builder().build(backend.analyze().await?);
        frontend.render().await?;
        Ok(())
    }

    pub async fn prepare(&self) -> Result<(ForkedDatabase, EnvWithHandlerCfg)> {
        let Self { tx_hash, quick, rpc, no_validation, etherscan: EtherscanOpts { chain, .. } } =
            self;
        let fork_url = rpc.url(true)?.unwrap().to_string();

        // step 0. prepare rpc provider
        let compute_units_per_second =
            if rpc.no_rate_limit { Some(u64::MAX) } else { rpc.compute_units_per_second };
        let mut provider_builder = foundry_common::provider::ProviderBuilder::new(&fork_url)
            .compute_units_per_second_opt(compute_units_per_second);
        if let Some(jwt) = rpc.jwt_secret.as_deref() {
            provider_builder = provider_builder.jwt(jwt);
        }
        let provider = Arc::new(provider_builder.build()?);
        ensure!(
            provider.get_chain_id().await? == chain.unwrap_or_default().id(),
            "inconsistent chain id"
        );

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

        let pb = init_progress!(txs, "Setting up the replay environment");
        pb.set_position(0);
        for (index, tx) in txs.into_iter().enumerate() {
            // System transactions such as on L2s don't contain any pricing info so
            // we skip them otherwise this would cause
            // reverts
            if is_known_system_sender(tx.from) ||
                tx.transaction_type == Some(SYSTEM_TRANSACTION_TYPE)
            {
                update_progress!(pb, index);
                continue;
            }

            // execute the transaction
            trace!("Executing transaction: {:?}", tx.hash);

            fill_tx_env(&mut env, &tx)?;
            let mut evm = new_evm_with_inspector(&mut db, env.clone(), NoOpInspector);
            let result = if &tx.hash == tx_hash {
                // we don't commit the target transaction
                evm.transact()?.result
            } else {
                evm.transact_commit()?
            };
            drop(evm);

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
            update_progress!(pb, index);
        }

        Ok((db, env))
    }
}
