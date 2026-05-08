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

//! Common evaluation utilities and convenience functions.
//!
//! This module provides high-level convenience functions for expression evaluation
//! that are commonly used throughout the EDB system.

use std::sync::Arc;

use alloy_dyn_abi::DynSolValue;
use eyre::Result;
use revm::{Database, DatabaseCommit, DatabaseRef, database::CacheDB};

use crate::{EngineContext, ExpressionEvaluator};

/// Evaluate a Solidity expression string within the context of a specific debug snapshot.
///
/// This is a convenience function that creates an EDB-configured expression evaluator
/// and evaluates the given expression against the specified snapshot.
///
/// # Arguments
/// * `context` - The EDB engine context containing snapshots and trace data
/// * `expr` - The expression string to evaluate (e.g., "balances[msg.sender]")
/// * `snapshot_id` - The ID of the debug snapshot to evaluate against
///
/// # Returns
/// The result of the expression evaluation as a [`DynSolValue`]
///
/// # Errors
/// Returns an error if:
/// - The expression cannot be parsed
/// - The snapshot ID is invalid
/// - The expression evaluation fails (e.g., variable not found, function call fails)
///
/// # Examples
/// ```rust,ignore
/// // Evaluate a balance check
/// let result = eval_on_snapshot(context, "balances[msg.sender] > 1000", snapshot_id)?;
///
/// // Evaluate a function call
/// let supply = eval_on_snapshot(context, "totalSupply()", snapshot_id)?;
///
/// // Evaluate blockchain context
/// let is_recent = eval_on_snapshot(context, "block.timestamp - lastUpdate < 3600", snapshot_id)?;
/// ```
pub fn eval_on_snapshot<DB>(
    context: Arc<EngineContext<DB>>,
    expr: &str,
    snapshot_id: usize,
) -> Result<DynSolValue>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    let evaluator = ExpressionEvaluator::new_edb(context);
    evaluator.eval(expr, snapshot_id)
}
