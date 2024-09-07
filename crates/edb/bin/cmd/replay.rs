use std::sync::Arc;

use alloy_primitives::TxHash;
use alloy_provider::Provider;
use alloy_rpc_types::{serde_helpers::WithOtherFields, BlockTransactionsKind};
use clap::Parser;
use edb_backend::DebugBackend;
use edb_frontend::DebugFrontend;
use edb_utils::{cache::CachePath, init_progress, update_progress};
use eyre::{ensure, OptionExt, Result};
use foundry_common::{is_known_system_sender, SYSTEM_TRANSACTION_TYPE};
use foundry_evm::{fork::database::ForkedDatabase, utils::new_evm_with_inspector};
use revm::{inspectors::NoOpInspector, primitives::EnvWithHandlerCfg};

use crate::{
    opts::{CacheOpts, EtherscanOpts, RpcOpts},
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
    #[arg(long)]
    pub no_validation: bool,

    #[command(flatten)]
    pub cache: CacheOpts,

    #[command(flatten)]
    pub etherscan: EtherscanOpts,

    #[command(flatten)]
    pub rpc: RpcOpts,
}

impl ReplayArgs {
    pub async fn run(mut self) -> Result<()> {
        if self.quick {
            // Enforce no validation when quick is enabled.
            self.no_validation = true;
        }

        let (db, env) = self.prepare().await?;
        self.debug(db, env).await?;
        Ok(())
    }

    #[allow(unused_mut, unused_variables, unreachable_code)]
    // XXX: Remove this after finishing the backend design
    pub async fn debug(&self, db: ForkedDatabase, env: EnvWithHandlerCfg) -> Result<()> {
        let chain = self.etherscan.chain();
        let client = self.etherscan.client()?;
        let cache_path = self.cache.cache_path();
        let debug_artifact = DebugBackend::<ForkedDatabase>::builder()
            .chain(Some(chain))
            .cache_path(cache_path)
            .etherscan_client(Some(client))
            .build::<ForkedDatabase>(&db, env)?
            .analyze()
            .await?;

        let mut frontend = DebugFrontend::builder().build(debug_artifact);
        todo!("Implement the backend for the frontend");

        frontend.render().await?;
        Ok(())
    }

    /// Prepare the environment and database for the replay.
    ///  - cache_root: the path to the rpc cache directory. If not provided, the default cache
    ///    directory will be used.
    pub async fn prepare(&self) -> Result<(ForkedDatabase, EnvWithHandlerCfg)> {
        let Self { tx_hash, quick, rpc, no_validation, etherscan, cache, .. } = self;
        let fork_url = rpc.url(true)?.unwrap().to_string();
        let cache_path = cache.cache_path();
        let chain = etherscan.chain();

        // step 0. prepare rpc provider
        let provider = Arc::new(rpc.provider(true)?.with_cache(chain, cache_path)?);
        ensure!(provider.get_chain_id().await? == chain.id(), "inconsistent chain id");

        // step 1. get the transaction and block data
        let tx = provider
            .get_transaction_by_hash(*tx_hash)
            .await?
            .ok_or_eyre("transaction not found")?;
        let tx_block_number: u64 =
            tx.block_number.ok_or_eyre("transaction may still be pending")?;
        let mut block = provider
            .get_block(tx_block_number.into(), BlockTransactionsKind::Full)
            .await?
            .ok_or_eyre("block not found")?;
        if !block.transactions.is_full() {
            return Err(eyre::eyre!("block transactions not found"));
        };
        trace!(tx_block_number=?tx_block_number, "transaction block number");

        // step 2. set enviroment and database
        // note that database should be set to tx_block_number - 1
        let cache_path = self.cache.cache_path().rpc_storage_cache_file(chain, tx_block_number - 1);
        let mut db =
            setup_fork_db(Arc::clone(&provider), &fork_url, Some(tx_block_number - 1), cache_path)
                .await?;
        let mut env = setup_block_env(Arc::clone(&provider), Some(tx_block_number)).await?;

        // step 3. replay all transactions before the target transaction
        // we use cumulative_gas_used as a quick validator for the correctness of the replay
        let mut cumulative_gas_used = 0u128;
        // prepare txs
        let mut txs = vec![];
        if !quick {
            let block_transactions = std::mem::take(&mut block.transactions);
            txs.extend(block_transactions.into_transactions().take_while(|tx| &tx.hash != tx_hash));
        };
        txs.push(WithOtherFields::new(tx.inner.clone()));

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
            trace!("replay transaction ({:?}): {:?}", tx.hash, tx);

            fill_tx_env(&mut env, &tx)?;
            trace!("revm env: {:?}", env);
            let mut evm = new_evm_with_inspector(&mut db, env.clone(), NoOpInspector);
            let result = if &tx.hash == tx_hash {
                // we don't commit the target transaction
                evm.transact()?.result
            } else {
                evm.transact_commit()?
            };
            trace!("repaly result: {:?}", result);
            drop(evm);

            let tx_receipt = provider
                .get_transaction_receipt(tx.hash)
                .await?
                .ok_or_eyre("transaction receipt not found")?;

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

#[cfg(test)]
mod tests {
    use std::{env, path::PathBuf, str::FromStr};

    use super::*;
    use serial_test::serial;

    async fn run_e2e_test(tx_hash: &str) -> Result<()> {
        let args = ReplayArgs {
            tx_hash: TxHash::from_str(tx_hash)?,
            quick: false,
            no_validation: false,
            cache: CacheOpts {
                no_cache: false,
                cache_root: Some(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/cache"),
                ),
            },
            etherscan: EtherscanOpts::default(),
            rpc: RpcOpts {
                url: option_env!("ETH_RPC_URL")
                    .map(|s| s.to_string())
                    .or(Some("https://rpc.mevblocker.io".to_string())),
                jwt_secret: None,
                no_rate_limit: false,
                flashbots: false,
                compute_units_per_second: None,
            },
        };

        let cache = args.cache.cache_path();
        let chain = args.etherscan.chain();
        let client = args.etherscan.client()?;

        let (db, env) = args.prepare().await?;
        let backend = DebugBackend::<ForkedDatabase>::builder()
            .chain(Some(chain))
            .cache_path(cache)
            .etherscan_client(Some(client))
            .build::<ForkedDatabase>(&db, env)?;
        let _ = backend.analyze().await?;

        Ok(())
    }

    /// Fetch the analysis results into cache, so that other tests can directly use the cache.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "this test is used to dump mock data from Etherscan"]
    async fn test_dump_cache() {
        run_e2e_test("0x6fbeed67e902cfe2934c4eda8e5d4a756c8b1a4a2a64d6b52309f32567e958c0")
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_replay_e2e_tx1() {
        run_e2e_test("0x1282e09bb5118f619da81b6a24c97999e7057ee9975628562c7cecbb4aa9f5af")
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_replay_e2e_tx2() {
        run_e2e_test("0xd253e3b563bf7b8894da2a69db836a4e98e337157564483d8ac72117df355a9d")
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_replay_e2e_tx3() {
        run_e2e_test("0x6f4d3b21b69335e210202c8f47867761315e824c5c360d1ab8910f5d7ce5d526")
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_replay_e2e_tx4() {
        run_e2e_test("0x0fe2542079644e107cbf13690eb9c2c65963ccb79089ff96bfaf8dced2331c92")
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_replay_e2e_tx5() {
        run_e2e_test("0x2c7d074e9d26ff1ab906c60fd014ed9dfb8103cfb64b5c9d49cfe732295a7e5b")
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_replay_e2e_constract_with_library() {
        run_e2e_test("0x9404771a145b4df4a6694a9896509d263448f5f27c2fd55ec8c47f37c9468b76")
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_replay_e2e_creation() {
        run_e2e_test("0x1e20cd6d47d7021ae7e437792823517eeadd835df09dde17ab45afd7a5df4603")
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_replay_coinbase_consistency() {
        run_e2e_test("0xc445aa7724e2b8b96a3e3b0c4d921a9329c12a9b2dda00368bb5f7b5da0b3e96")
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_replay_penpie_hack_tx1() {
        run_e2e_test("0xca87f257280e19378dc1890a478514195f068857affacde0b92c851b897dff9e")
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_replay_penpie_hack_tx2() {
        run_e2e_test("0x56e09abb35ff12271fdb38ff8a23e4d4a7396844426a94c4d3af2e8b7a0a2813")
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_replay_penpie_hack_tx3() {
        run_e2e_test("0x663b55a1ee992603f7636ef23ff5cf19d3b261ab81494d06e218c86482df5342")
            .await
            .unwrap();
    }
}
