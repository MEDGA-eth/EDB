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

//! Async REVM database adapter backed by the workspace Alloy provider.

use alloy_network::{BlockResponse, Network, primitives::HeaderResponse};
use alloy_provider::Provider;
use alloy_rpc_types::BlockId;
use revm::{
    database_interface::{DBErrorMarker, async_db::DatabaseAsyncRef},
    primitives::{Address, B256, StorageKey, StorageValue},
    state::{AccountInfo, Bytecode},
};
use std::fmt::{self, Display};

/// Error type for provider-backed database operations.
#[derive(Debug)]
pub enum ProviderDbError {
    /// Transport or RPC-level failure from the upstream provider.
    Transport(String),
    /// Requested block could not be loaded from the upstream provider.
    BlockNotFound(u64),
}

impl DBErrorMarker for ProviderDbError {}

impl Display for ProviderDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(err) => write!(f, "Transport error: {err}"),
            Self::BlockNotFound(number) => write!(f, "Block not found: {number}"),
        }
    }
}

impl std::error::Error for ProviderDbError {}

/// Minimal async database adapter matching REVM's removed-in-practice AlloyDB path, but using the
/// workspace Alloy 2 provider stack instead of REVM's older Alloy 1 integration.
#[derive(Debug)]
pub struct ProviderDb<N: Network, P: Provider<N>> {
    provider: P,
    block_number: BlockId,
    marker: core::marker::PhantomData<fn() -> N>,
}

impl<N: Network, P: Provider<N>> ProviderDb<N, P> {
    /// Creates a new provider-backed database for the selected block.
    pub fn new(provider: P, block_number: BlockId) -> Self {
        Self { provider, block_number, marker: core::marker::PhantomData }
    }
}

impl<N: Network, P: Provider<N>> DatabaseAsyncRef for ProviderDb<N, P> {
    type Error = ProviderDbError;

    async fn basic_async_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let nonce = self.provider.get_transaction_count(address).block_id(self.block_number);
        let balance = self.provider.get_balance(address).block_id(self.block_number);
        let code = self.provider.get_code_at(address).block_id(self.block_number);

        let (nonce, balance, code) = tokio::join!(nonce, balance, code);

        let balance = balance.map_err(|err| ProviderDbError::Transport(err.to_string()))?;
        let code = code.map_err(|err| ProviderDbError::Transport(err.to_string()))?;
        let nonce = nonce.map_err(|err| ProviderDbError::Transport(err.to_string()))?;

        let code = Bytecode::new_raw(code.0.into());
        let code_hash = code.hash_slow();

        Ok(Some(AccountInfo::new(balance, nonce, code_hash, code)))
    }

    async fn block_hash_async_ref(&self, number: u64) -> Result<B256, Self::Error> {
        let block = self
            .provider
            .get_block_by_number(number.into())
            .await
            .map_err(|err| ProviderDbError::Transport(err.to_string()))?;

        match block {
            Some(block) => Ok(B256::new(*block.header().hash())),
            None => Err(ProviderDbError::BlockNotFound(number)),
        }
    }

    async fn code_by_hash_async_ref(&self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        panic!(
            "code_by_hash_async_ref should not be called because code is loaded via basic_async_ref"
        );
    }

    async fn storage_async_ref(
        &self,
        address: Address,
        index: StorageKey,
    ) -> Result<StorageValue, Self::Error> {
        self.provider
            .get_storage_at(address, index)
            .block_id(self.block_number)
            .await
            .map_err(|err| ProviderDbError::Transport(err.to_string()))
    }
}
