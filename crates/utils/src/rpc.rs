use std::ops::{Deref, DerefMut};

use alloy_chains::{Chain, NamedChain};
use alloy_network::Network;
use alloy_primitives::TxHash;
use alloy_provider::{Provider, ProviderCall, RootProvider};
use alloy_rpc_types::{BlockId, BlockNumberOrTag, BlockTransactionsKind};
use alloy_transport::{Transport, TransportError, TransportResult};
use eyre::{bail, Result};

use crate::cache::{Cache, CachePath, EDBCache};

type Receipt<N> = Option<<N as Network>::ReceiptResponse>;
type Transaction<N> = Option<<N as Network>::TransactionResponse>;

#[derive(Debug, Clone)]
pub struct CachedProvider<P, N>
where
    N: Network,
{
    provider: P,

    // Cache for the provider
    receipt_cache: Option<EDBCache<Receipt<N>>>,
    tx_cache: Option<EDBCache<Transaction<N>>>,
    block_cache: Option<EDBCache<Option<N::BlockResponse>>>,
}

impl<P, N> Deref for CachedProvider<P, N>
where
    N: Network,
{
    type Target = P;

    fn deref(&self) -> &Self::Target {
        &self.provider
    }
}

impl<P, N> DerefMut for CachedProvider<P, N>
where
    N: Network,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.provider
    }
}

impl<P, N> CachedProvider<P, N>
where
    N: Network,
{
    pub fn new(provider: P) -> Self {
        Self { provider, receipt_cache: None, tx_cache: None, block_cache: None }
    }

    pub fn with_cache<C: CachePath>(
        mut self,
        chain: impl Into<Chain>,
        cache_path: C,
    ) -> Result<Self> {
        let chain = chain.into();

        if cache_path.is_valid() {
            let named_chain: NamedChain = chain.id().try_into().map_err(|_| {
                eyre::eyre!(
                    "The provider does not support caching for unnamed chain: {}",
                    chain.id()
                )
            })?;
            if named_chain == NamedChain::Dev || named_chain == NamedChain::AnvilHardhat {
                bail!("The provider does not support caching for dev chain: {}", chain.id());
            }
        }

        self.receipt_cache = EDBCache::new(cache_path.rpc_receipt_cache_dir(chain), None)?;
        self.tx_cache = EDBCache::new(cache_path.rpc_tx_cache_dir(chain), None)?;
        self.block_cache = EDBCache::new(cache_path.rpc_block_cache_dir(chain), None)?;

        Ok(self)
    }
}

impl<P, N> Unpin for CachedProvider<P, N> where N: Network {}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl<P, T, N> Provider<T, N> for CachedProvider<P, N>
where
    P: Provider<T, N>,
    T: Transport + Clone,
    N: Network,
{
    fn root(&self) -> &RootProvider<T, N> {
        self.provider.root()
    }

    fn get_transaction_receipt(
        &self,
        hash: TxHash,
    ) -> ProviderCall<T, (TxHash,), Option<N::ReceiptResponse>> {
        if let Some(recipt) = self.receipt_cache.load_cache(hash.to_string()) {
            ProviderCall::ready(Ok(recipt))
        } else {
            let provider_call = self.provider.get_transaction_receipt(hash);
            let cache = self.receipt_cache.clone();

            ProviderCall::BoxedFuture(Box::pin(async move {
                let response = provider_call.await;
                if let Ok(receipt) = response {
                    match cache.save_cache(hash.to_string(), &receipt) {
                        Ok(_) => Ok(receipt),
                        Err(e) => {
                            TransportResult::Err(TransportError::local_usage_str(&e.to_string()))
                        }
                    }
                } else {
                    response
                }
            }))
        }
    }

    fn get_transaction_by_hash(
        &self,
        hash: TxHash,
    ) -> ProviderCall<T, (TxHash,), Option<N::TransactionResponse>> {
        if let Some(tx) = self.tx_cache.load_cache(hash.to_string()) {
            ProviderCall::ready(Ok(tx))
        } else {
            let provider_call = self.provider.get_transaction_by_hash(hash);
            let cache = self.tx_cache.clone();

            ProviderCall::BoxedFuture(Box::pin(async move {
                let response = provider_call.await;
                if let Ok(tx) = response {
                    match cache.save_cache(hash.to_string(), &tx) {
                        Ok(_) => Ok(tx),
                        Err(e) => {
                            TransportResult::Err(TransportError::local_usage_str(&e.to_string()))
                        }
                    }
                } else {
                    response
                }
            }))
        }
    }

    async fn get_block(
        &self,
        number: BlockId,
        kind: BlockTransactionsKind,
    ) -> TransportResult<Option<N::BlockResponse>> {
        // We only cache canonical blocks with number or hash
        let block_str = match number {
            BlockId::Number(BlockNumberOrTag::Number(n)) => n.to_string(),
            BlockId::Hash(h) => h.as_ref().to_string(),
            _ => {
                return self.provider.get_block(number, kind).await;
            }
        };
        let is_full: bool = kind.into();
        let cache_str = format!("{block_str}_{is_full}");

        if let Some(block) = self.block_cache.load_cache(&cache_str) {
            return Ok(block);
        } else {
            let rv = self.provider.get_block(number, kind).await?;

            match self.block_cache.save_cache(&cache_str, &rv) {
                Ok(_) => Ok(rv),
                Err(e) => TransportResult::Err(TransportError::local_usage_str(&e.to_string())),
            }
        }
    }
}
