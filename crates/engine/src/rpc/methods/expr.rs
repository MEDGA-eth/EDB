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

//! Expression evaluation RPC methods.
//!
//! This module implements the core expression evaluation functionality that allows
//! real-time evaluation of Solidity-like expressions against any debugging snapshot.
//! This is one of the most powerful features of EDB, enabling interactive debugging
//! and inspection of contract state.
//!
//! # Available Methods
//!
//! - `edb_evalOnSnapshot` - Evaluate an expression against a specific snapshot
//!
//! # Supported Expressions
//!
//! The evaluator supports a wide range of Solidity-compatible expressions:
//! - **Variables**: `balance`, `owner`, `this`
//! - **Mappings/Arrays**: `balances[user]`, `data[0]`
//! - **Function Calls**: `balanceOf(user)`, `totalSupply()`
//! - **Member Access**: `token.symbol`, `addr.balance`
//! - **Arithmetic**: `balance * price / 1e18`
//! - **Comparisons**: `msg.sender == owner`
//! - **Blockchain Context**: `msg.sender`, `msg.value`, `block.timestamp`
//! - **Type Casting**: `uint256(value)`, `address(0x123...)`
//! - **Logical Operations**: `approved && amount > 0`
//!
//! # Example Usage
//!
//! ```json
//! // Request
//! {
//!   "method": "edb_evalOnSnapshot",
//!   "params": [150, "balances[msg.sender] > 1000"]
//! }
//!
//! // Response
//! {
//!   "result": {
//!     "type": "bool",
//!     "value": true
//!   }
//! }
//! ```

use std::sync::Arc;

use edb_common::types::EdbSolValue;
use revm::{Database, DatabaseCommit, DatabaseRef, database::CacheDB};
use serde_json::Value;
use tracing::debug;

use crate::{EngineContext, RpcError, error_codes, eval};

/// Evaluate a Solidity-like expression against a specific snapshot.
///
/// This is the core debugging method that enables real-time expression evaluation.
/// It takes a snapshot ID and an expression string, then evaluates the expression
/// in the context of that snapshot's execution state.
///
/// # Parameters
/// - `snapshot_id` (number) - The snapshot ID to evaluate against (0-indexed)
/// - `expr` (string) - The expression to evaluate
///
/// # Returns
/// An object containing the evaluated result with type information:
/// ```json
/// {
///   "type": "uint256",
///   "value": "1000000000000000000"
/// }
/// ```
///
/// # Supported Expression Types
/// - Variables and state access
/// - Function calls (view/pure functions)
/// - Arithmetic and logical operations
/// - Type casting and comparisons
/// - Blockchain context (msg, tx, block)
///
/// # Error Conditions
/// - Invalid snapshot ID (out of bounds)
/// - Expression parsing errors
/// - Runtime evaluation errors (e.g., division by zero)
/// - Type resolution failures
pub fn eval_on_snapshot<DB>(
    context: &Arc<EngineContext<DB>>,
    params: Option<Value>,
) -> Result<serde_json::Value, RpcError>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    // Parse the snapshot ID as the first argument
    let snapshot_id = params
        .as_ref()
        .and_then(|p| p.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError {
            code: error_codes::INVALID_PARAMS,
            message: "Invalid params: expected [snapshot_id, expr]".to_string(),
            data: None,
        })? as usize;

    // Parse the expression as the second argument
    let expr = params
        .as_ref()
        .and_then(|p| p.as_array())
        .and_then(|arr| arr.get(1))
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcError {
            code: error_codes::INVALID_PARAMS,
            message: "Invalid params: expected [snapshot_id, expr]".to_string(),
            data: None,
        })?;

    let value: Result<EdbSolValue, String> =
        eval::eval_on_snapshot(context.clone(), expr, snapshot_id)
            .map(|v| v.into())
            .map_err(|e| e.to_string());

    let json_value = serde_json::to_value(value).map_err(|e| RpcError {
        code: error_codes::INTERNAL_ERROR,
        message: format!("Failed to serialize ABI: {e}"),
        data: None,
    })?;

    debug!("Evaluated expression '{}' on snapshot {}: {:?}", expr, snapshot_id, json_value);
    Ok(json_value)
}
