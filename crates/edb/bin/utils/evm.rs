use std::{path::PathBuf, sync::Arc};

use alloy_chains::NamedChain;
use alloy_consensus::TxType;
use alloy_primitives::{TxKind, U256};
use alloy_provider::{network::AnyNetwork, Provider};
use alloy_rpc_types::{BlockNumberOrTag, Transaction};
use alloy_transport::{Transport, TransportError};
use anvil::Hardfork;
use eyre::{eyre, Result};
use foundry_common::constants::NON_ARCHIVE_NODE_WARNING;
use foundry_evm::{
    fork::database::ForkedDatabase, utils::apply_chain_and_block_specific_env_changes,
};
use foundry_fork_db::{cache::BlockchainDbMeta, BlockchainDb, SharedBackend};
use revm::primitives::{BlobExcessGasAndPrice, BlockEnv, Env, EnvWithHandlerCfg};

use edb_utils::cache::CachePath;

pub async fn setup_block_env<
    T: Transport + Clone + Unpin,
    P: Provider<T, AnyNetwork> + Unpin + 'static + Clone,
>(
    provider: Arc<P>,
    fork_block_number: Option<u64>,
) -> Result<EnvWithHandlerCfg> {
    let mut env = EnvWithHandlerCfg::default();

    let (fork_block_number, fork_chain_id) = if let Some(fork_block_number) = fork_block_number {
        // auto adjust hardfork if not specified
        // but only if we're forking mainnet
        let chain_id = provider.get_chain_id().await?;
        if NamedChain::Mainnet == chain_id {
            let hardfork: Hardfork = fork_block_number.into();
            env.handler_cfg.spec_id = hardfork.into();
        }

        (fork_block_number, Some(U256::from(chain_id)))
    } else {
        // pick the last block number but also ensure it's not pending anymore
        let bn = find_latest_fork_block(Arc::clone(&provider)).await?;
        (bn, None)
    };

    let block = provider
        .get_block(BlockNumberOrTag::Number(fork_block_number).into(), false.into())
        .await?;

    let block = if let Some(block) = block {
        block
    } else {
        if let Ok(latest_block) = provider.get_block_number().await {
            let mut message = format!(
                "failed to get block for block number: {fork_block_number}\n\
latest block number: {latest_block}"
            );
            // If the `eth_getBlockByNumber` call succeeds, but returns null instead of
            // the block, and the block number is less than equal the latest block, then
            // the user is forking from a non-archive node with an older block number.
            if fork_block_number <= latest_block {
                message.push_str(&format!("\n{NON_ARCHIVE_NODE_WARNING}"));
            }
            return Err(eyre!("{}", message));
        }
        return Err(eyre!("failed to get block for block number: {fork_block_number}"));
    };
    trace!("current block header: {:?}", block.header);

    // we only use the gas limit value of the block if it is non-zero and the block gas
    // limit is enabled, since there are networks where this is not used and is always
    // `0x0` which would inevitably result in `OutOfGas` errors as soon as the evm is about to record gas, See also <https://github.com/foundry-rs/foundry/issues/3247>
    let gas_limit =
        if block.header.gas_limit == 0 { u64::MAX as u128 } else { block.header.gas_limit };

    env.block = BlockEnv {
        number: U256::from(fork_block_number),
        timestamp: U256::from(block.header.timestamp),
        difficulty: block.header.difficulty,
        // Ensures prevrandao is set.
        prevrandao: Some(block.header.mix_hash.unwrap_or_default()),
        gas_limit: U256::from(gas_limit),
        // Ensures coinbase is set (since coinbase will always be treated as a warm account in EVM
        // after EIP-3651).
        coinbase: block.header.miner,
        ..Default::default()
    };

    if let Some(base_fee) = block.header.base_fee_per_gas {
        env.block.basefee = U256::from(base_fee);
    }
    if let (Some(blob_excess_gas), Some(_)) =
        (block.header.excess_blob_gas, block.header.blob_gas_used)
    {
        env.block.blob_excess_gas_and_price =
            Some(BlobExcessGasAndPrice::new(blob_excess_gas as u64));
    }

    let chain_id = if let Some(fork_chain_id) = fork_chain_id {
        fork_chain_id.to()
    } else {
        provider.get_chain_id().await.unwrap()
    };
    // need to update the dev signers and env with the chain id
    env.cfg.chain_id = chain_id;
    env.tx.chain_id = chain_id.into();

    // apply changes such as difficulty -> prevrandao and chain specifics for current chain id
    apply_chain_and_block_specific_env_changes(&mut env, &block);

    Ok(env)
}

pub async fn setup_fork_db<
    T: Transport + Clone + Unpin,
    P: Provider<T, AnyNetwork> + Unpin + 'static + Clone,
>(
    provider: Arc<P>,
    eth_rpc_url: &str,
    fork_block_number: Option<u64>,
    cache_path: Option<PathBuf>,
) -> Result<ForkedDatabase> {
    let env = setup_block_env(Arc::clone(&provider), fork_block_number).await?;

    let chain_id = env.cfg.chain_id;
    let fork_block_number = env.block.number.try_into()?;

    let meta = BlockchainDbMeta::new(*env.env.clone(), eth_rpc_url.to_string());
    let block_chain_db = BlockchainDb::new_skip_check(
        meta,
        cache_path.or(CachePath::edb_block_cache_file(chain_id, fork_block_number)),
    );

    // This will spawn the background thread that will use the provider to fetch
    // blockchain data from the other client
    let backend = SharedBackend::spawn_backend_thread(
        Arc::clone(&provider),
        block_chain_db.clone(),
        Some(fork_block_number.into()),
    );

    Ok(ForkedDatabase::new(backend, block_chain_db))
}

/// Finds the latest appropriate block to fork
///
/// This fetches the "latest" block and checks whether the `Block` is fully populated (`hash` field
/// is present).
async fn find_latest_fork_block<
    T: Transport + Clone + Unpin,
    P: Provider<T, AnyNetwork> + Unpin + 'static + Clone,
>(
    provider: Arc<P>,
) -> Result<u64, TransportError> {
    let mut num = provider.get_block_number().await?;

    // walk back from the head of the chain, but at most 2 blocks, which should be more than enough
    // leeway
    for _ in 0..2 {
        if let Some(block) = provider.get_block(num.into(), false.into()).await? {
            if block.header.hash.is_some() {
                break;
            }
        }
        // block not actually finalized, so we try the block before
        num = num.saturating_sub(1)
    }

    Ok(num)
}

/// Fill transaction environment from a [Transaction] and the given sender address.
pub fn fill_tx_env(env: &mut Env, tx: &Transaction) -> Result<()> {
    env.tx.caller = tx.from;

    match tx.transaction_type.unwrap_or_default().try_into()? {
        TxType::Legacy => {
            env.tx.gas_limit = tx.gas as u64;
            env.tx.gas_price = U256::from(tx.gas_price.unwrap_or_default());
            env.tx.gas_priority_fee = None;
            env.tx.transact_to = tx.to.map(TxKind::Call).unwrap_or(TxKind::Create);
            env.tx.value = tx.value;
            env.tx.data = tx.input.clone();
            env.tx.chain_id = tx.chain_id;
            env.tx.nonce = Some(tx.nonce);
            env.tx.access_list.clear();
            env.tx.blob_hashes.clear();
            env.tx.max_fee_per_blob_gas.take();
        }
        TxType::Eip2930 => {
            env.tx.gas_limit = tx.gas as u64;
            env.tx.gas_price = U256::from(tx.gas_price.unwrap_or_default());
            env.tx.gas_priority_fee = None;
            env.tx.transact_to = tx.to.map(TxKind::Call).unwrap_or(TxKind::Create);
            env.tx.value = tx.value;
            env.tx.data = tx.input.clone();
            env.tx.chain_id = tx.chain_id;
            env.tx.nonce = Some(tx.nonce);
            env.tx.access_list = tx
                .access_list
                .clone()
                .ok_or(eyre::eyre!("missing access list"))?
                .iter()
                .map(|l| {
                    (l.address, l.storage_keys.iter().map(|k| U256::from_be_bytes(k.0)).collect())
                })
                .collect();
            env.tx.blob_hashes.clear();
            env.tx.max_fee_per_blob_gas.take();
        }
        TxType::Eip1559 => {
            env.tx.gas_limit = tx.gas as u64;
            env.tx.gas_price = U256::from(tx.gas_price.unwrap_or_default());
            env.tx.gas_priority_fee = Some(U256::from(
                tx.max_priority_fee_per_gas.ok_or(eyre::eyre!("missing max priority fee"))?,
            ));
            env.tx.transact_to = tx.to.map(TxKind::Call).unwrap_or(TxKind::Create);
            env.tx.value = tx.value;
            env.tx.data = tx.input.clone();
            env.tx.chain_id = tx.chain_id;
            env.tx.nonce = Some(tx.nonce);
            env.tx.access_list = tx
                .access_list
                .clone()
                .ok_or(eyre::eyre!("missing access list"))?
                .iter()
                .map(|l| {
                    (l.address, l.storage_keys.iter().map(|k| U256::from_be_bytes(k.0)).collect())
                })
                .collect();
            env.tx.blob_hashes.clear();
            env.tx.max_fee_per_blob_gas.take();
        }
        TxType::Eip4844 => {
            env.tx.gas_limit = tx.gas as u64;
            env.tx.gas_price = U256::from(tx.gas_price.unwrap_or_default());
            env.tx.gas_priority_fee = Some(U256::from(
                tx.max_priority_fee_per_gas.ok_or(eyre::eyre!("missing max priority fee"))?,
            ));
            env.tx.transact_to = TxKind::Call(tx.to.ok_or(eyre::eyre!("missing to in eip4844"))?);
            env.tx.value = tx.value;
            env.tx.data = tx.input.clone();
            env.tx.chain_id = tx.chain_id;
            env.tx.nonce = Some(tx.nonce);
            env.tx.blob_hashes.clone_from(
                &(tx.blob_versioned_hashes.clone().ok_or(eyre::eyre!("missing blob hashes"))?),
            );
            env.tx.max_fee_per_blob_gas = tx.max_fee_per_blob_gas.map(U256::from);
            env.tx.access_list = tx
                .access_list
                .clone()
                .ok_or(eyre::eyre!("missing access list"))?
                .iter()
                .map(|l| {
                    (l.address, l.storage_keys.iter().map(|k| U256::from_be_bytes(k.0)).collect())
                })
                .collect();
        }
        #[cfg(feature = "optimism")]
        Transaction::Deposit(tx) => {
            env.tx.access_list.clear();
            env.tx.gas_limit = tx.gas_limit;
            env.tx.gas_price = U256::ZERO;
            env.tx.gas_priority_fee = None;
            env.tx.transact_to = tx.to;
            env.tx.value = tx.value;
            env.tx.data = tx.input.clone();
            env.tx.chain_id = None;
            env.tx.nonce = None;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Hardfork;

    #[test]
    fn test_hardfork_blocks() {
        let hf: Hardfork = 12_965_000u64.into();
        assert_eq!(hf, Hardfork::London);

        let hf: Hardfork = 4370000u64.into();
        assert_eq!(hf, Hardfork::Byzantium);

        let hf: Hardfork = 12244000u64.into();
        assert_eq!(hf, Hardfork::Berlin);
    }
}
