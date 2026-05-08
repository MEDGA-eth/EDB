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

use alloy_dyn_abi::{DynSolValue, FunctionExt, JsonAbiExt};
use alloy_json_abi::Function;
use alloy_primitives::{Address, Bytes, U256};
use edb_common::{
    DerivedContext, disable_nonce_check, relax_evm_context_constraints, relax_evm_tx_constraints,
};
use eyre::{Result, eyre};
use revm::{
    Context, Database, DatabaseCommit, DatabaseRef, ExecuteEvm, MainBuilder, MainContext,
    MainnetEvm,
    context::{result::ExecutionResult, tx::TxEnvBuilder},
    database::CacheDB,
};

use crate::EngineContext;

/// Trait providing EVM creation and expression evaluation capabilities on the EngineContext.
/// This trait allows creating derived EVM instances for specific snapshots,
/// sending transactions, and invoking contract function calls in the context of those snapshots.
pub trait ContextEvmTr<DB>
where
    DB: DatabaseRef,
{
    /// Create a derived EVM instance for a specific snapshot.
    ///
    /// This method creates a new EVM instance using the database state from the
    /// specified snapshot. The resulting EVM can be used for expression evaluation
    /// and function calls without affecting the original debugging state.
    ///
    /// # Arguments
    ///
    /// * `snapshot_id` - The snapshot ID to create the EVM for
    ///
    /// # Returns
    ///
    /// Returns a configured EVM instance or None if the snapshot doesn't exist.
    fn create_evm_for_snapshot(&self, snapshot_id: usize)
    -> Option<MainnetEvm<DerivedContext<DB>>>;

    /// Send a mock transaction in a derived EVM.
    ///
    /// This method executes a transaction in the EVM state at the specified snapshot
    /// without affecting the original debugging state. Used for expression evaluation
    /// that requires transaction execution.
    ///
    /// # Arguments
    ///
    /// * `snapshot_id` - The snapshot ID to use as the base state
    /// * `to` - The target address for the transaction
    /// * `data` - The transaction data (call data)
    /// * `value` - The value to send with the transaction
    ///
    /// # Returns
    ///
    /// Returns the execution result or an error if the transaction fails.
    fn send_transaction_in_derived_evm(
        &self,
        snapshot_id: usize,
        to: Address,
        data: &[u8],
        value: U256,
    ) -> Result<ExecutionResult>;

    /// Invoke a contract function call in a derived EVM.
    ///
    /// This method calls a specific contract function in the EVM state at the
    /// specified snapshot. It handles ABI encoding/decoding automatically and
    /// returns the decoded result.
    ///
    /// # Arguments
    ///
    /// * `snapshot_id` - The snapshot ID to use as the base state
    /// * `to` - The contract address to call
    /// * `function` - The ABI function definition
    /// * `args` - The function arguments
    /// * `value` - Optional value to send with the call
    ///
    /// # Returns
    ///
    /// Returns the decoded function result or an error if the call fails.
    fn call_in_derived_evm(
        &self,
        snapshot_id: usize,
        to: Address,
        function: &Function,
        args: &[DynSolValue],
        value: Option<U256>,
    ) -> Result<DynSolValue>;
}

// EVM creation and expression evaluation methods
impl<DB> ContextEvmTr<DB> for EngineContext<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    fn create_evm_for_snapshot(
        &self,
        snapshot_id: usize,
    ) -> Option<MainnetEvm<DerivedContext<DB>>> {
        let (_, snapshot) = self.snapshots.get(snapshot_id)?;

        let db = CacheDB::new(CacheDB::new(snapshot.db()));
        let cfg = self.cfg.clone();
        let block = self.block.clone();
        let transient_storage = snapshot.transient_storage();

        let mut ctx =
            Context::mainnet().with_db(db).with_cfg(cfg).with_block(block).modify_journal_chained(
                |journal| journal.transient_storage.extend(transient_storage.iter()),
            );
        relax_evm_context_constraints(&mut ctx);
        disable_nonce_check(&mut ctx);

        Some(ctx.build_mainnet())
    }

    fn send_transaction_in_derived_evm(
        &self,
        snapshot_id: usize,
        to: Address,
        data: &[u8],
        value: U256,
    ) -> Result<ExecutionResult> {
        let mut evm = self
            .create_evm_for_snapshot(snapshot_id)
            .ok_or(eyre!("No EVM found at snapshot {}", snapshot_id))?;

        let mut tx_env = TxEnvBuilder::new()
            .caller(self.tx.caller)
            .call(to)
            .value(value)
            .data(Bytes::copy_from_slice(data))
            .build_fill();
        relax_evm_tx_constraints(&mut tx_env);

        evm.transact_one(tx_env).map_err(|e| eyre!(e.to_string()))
    }

    fn call_in_derived_evm(
        &self,
        snapshot_id: usize,
        to: Address,
        function: &Function,
        args: &[DynSolValue],
        value: Option<U256>,
    ) -> Result<DynSolValue> {
        let data = function.abi_encode_input(args).map_err(|e| eyre!(e.to_string()))?;
        let value = value.unwrap_or_default();

        let result = self.send_transaction_in_derived_evm(snapshot_id, to, &data, value)?;

        match result {
            ExecutionResult::Success { output, .. } => {
                let decoded =
                    function.abi_decode_output(output.data()).map_err(|e| eyre!(e.to_string()))?;
                if decoded.len() == 1 {
                    Ok(decoded.into_iter().next().unwrap())
                } else {
                    Ok(DynSolValue::Tuple(decoded))
                }
            }
            ExecutionResult::Revert { output, .. } => {
                Err(eyre!("Call reverted with output: 0x{}", hex::encode(output)))
            }
            ExecutionResult::Halt { reason, .. } => {
                Err(eyre!("Call halted with reason: {:?}", reason))
            }
        }
    }
}
