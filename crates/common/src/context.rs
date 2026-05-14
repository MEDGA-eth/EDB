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

//! Context-related types and traits
//! This module provides types and traits for working with the EVM context.

use revm::{
    Context, Database, DatabaseCommit, DatabaseRef,
    context::{BlockEnv, CfgEnv, TxEnv},
    database::CacheDB,
    database_interface::DBErrorMarker,
    primitives::{Address, AddressMap, B256, U256},
    state::{Account, AccountInfo, Bytecode},
};
use std::{fmt, sync::Arc};

/// Type alias for the EDB context in terms of revm's Context
pub type EdbContext<DB> = Context<BlockEnv, TxEnv, CfgEnv, CacheDB<DB>>;

/// Type alias for the derived context with Arc-wrapped CacheDB.
/// This context is used for those derived EVM instances at each snapshot.
pub type DerivedContext<DB> = EdbContext<CacheDB<Arc<CacheDB<DB>>>>;

/// Relax the constraints for EVM execution in the given context and transaction
pub fn relax_evm_constraints<DB: Database + DatabaseRef>(
    context: &mut EdbContext<DB>,
    tx: &mut TxEnv,
) {
    relax_evm_context_constraints(context);
    relax_evm_tx_constraints(tx);
}

/// Relax the constraints for EVM execution in the given context
pub fn relax_evm_context_constraints<DB: Database + DatabaseRef>(context: &mut EdbContext<DB>) {
    let cfg = &mut context.cfg;
    cfg.disable_base_fee = true;
    cfg.disable_block_gas_limit = true;
    cfg.disable_balance_check = true;
    cfg.tx_gas_limit_cap = Some(u64::MAX);
    cfg.limit_contract_initcode_size = Some(usize::MAX);
    cfg.limit_contract_code_size = Some(usize::MAX);
}

/// Disable nonce check in the given context
pub fn disable_nonce_check<DB: Database + DatabaseRef>(context: &mut EdbContext<DB>) {
    context.cfg.disable_nonce_check = true;
}

/// Relax the constraints for EVM execution in the given transaction.
///
/// Used by EDB's derived-EVM execution path (`call_in_derived_evm` and
/// friends in `engine::context::evm`) to construct synthetic transactions
/// for state-variable getter reads, expression evaluation, etc. EDB does
/// not model gas faithfully and does not enforce chain-id pinning on
/// internally-generated transactions, so the synthetic tx should pass
/// EVM validation regardless of what the actual cfg has set.
pub fn relax_evm_tx_constraints(tx: &mut TxEnv) {
    tx.gas_limit = u64::MAX; // Relax gas limit for execution
    tx.gas_price = 0; // Relax gas price for execution
    tx.gas_priority_fee = Some(0); // Relax gas priority fee for execution
    // Synthetic transactions don't need EIP-155 chain-id pinning — they're
    // never broadcast, only run inside the derived EVM. Leaving the default
    // `TxEnvBuilder` chain_id (mainnet 1) in place caused REVM to reject
    // synthetic state-variable reads with "invalid chain ID" whenever the
    // cfg's chain_id differed (e.g. after `vm.chainId(...)` or in forked mode
    // against a non-mainnet chain).
    tx.chain_id = None;
}

/// A cloneable error type for EdbDB
#[derive(Clone, Debug)]
pub struct EdbDBError {
    message: String,
}

impl EdbDBError {
    /// Create a new error with a message
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    /// Create from any error type
    pub fn from_error<E: std::error::Error>(err: E) -> Self {
        Self::new(err.to_string())
    }
}

impl fmt::Display for EdbDBError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EdbDB Error: {}", self.message)
    }
}

impl std::error::Error for EdbDBError {}

impl DBErrorMarker for EdbDBError {}

/// A wrapper database that provides a cloneable error type
/// This allows the database to be used in contexts requiring Clone
#[derive(Clone)]
pub struct EdbDB<DB> {
    inner: DB,
}

impl<DB> EdbDB<DB> {
    /// Create a new EdbDB wrapping an inner database
    pub fn new(inner: DB) -> Self {
        Self { inner }
    }

    /// Get a reference to the inner database
    pub fn inner(&self) -> &DB {
        &self.inner
    }

    /// Get a mutable reference to the inner database
    pub fn inner_mut(&mut self) -> &mut DB {
        &mut self.inner
    }

    /// Consume self and return the inner database
    pub fn into_inner(self) -> DB {
        self.inner
    }
}

impl<DB> Database for EdbDB<DB>
where
    DB: Database,
    <DB as Database>::Error: std::error::Error,
{
    type Error = EdbDBError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.inner.basic(address).map_err(EdbDBError::from_error)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.inner.code_by_hash(code_hash).map_err(EdbDBError::from_error)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.inner.storage(address, index).map_err(EdbDBError::from_error)
    }

    fn block_hash(&mut self, block: u64) -> Result<B256, Self::Error> {
        self.inner.block_hash(block).map_err(EdbDBError::from_error)
    }
}

impl<DB> DatabaseCommit for EdbDB<DB>
where
    DB: DatabaseCommit + Database,
    <DB as Database>::Error: std::error::Error,
{
    fn commit(&mut self, changes: AddressMap<Account>) {
        self.inner.commit(changes)
    }
}

impl<DB> DatabaseRef for EdbDB<DB>
where
    DB: DatabaseRef + Database,
    <DB as Database>::Error: std::error::Error,
{
    type Error = EdbDBError;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.inner.basic_ref(address).map_err(EdbDBError::from_error)
    }

    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.inner.code_by_hash_ref(code_hash).map_err(EdbDBError::from_error)
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.inner.storage_ref(address, index).map_err(EdbDBError::from_error)
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        self.inner.block_hash_ref(number).map_err(EdbDBError::from_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relax_evm_tx_constraints() {
        let mut tx = TxEnv::default();

        relax_evm_tx_constraints(&mut tx);

        assert_eq!(tx.gas_limit, u64::MAX);
        assert_eq!(tx.gas_price, 0);
        assert_eq!(tx.gas_priority_fee, Some(0));
    }

    #[test]
    fn test_edb_db_error_new() {
        let error = EdbDBError::new("test error");
        assert_eq!(error.message, "test error");
    }

    #[test]
    fn test_edb_db_error_display() {
        let error = EdbDBError::new("display test");
        assert_eq!(format!("{error}"), "EdbDB Error: display test");
    }
}
