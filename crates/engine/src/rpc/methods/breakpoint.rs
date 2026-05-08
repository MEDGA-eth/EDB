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

//! Breakpoint management RPC methods.
//!
//! This module implements breakpoint functionality that allows setting and querying
//! breakpoints during debugging sessions. Breakpoints can be location-based (at specific
//! opcodes or source lines) and optionally include conditional expressions that must
//! evaluate to true for the breakpoint to trigger.
//!
//! # Available Methods
//!
//! - `edb_getBreakpointHits` - Retrieve all snapshots where a breakpoint was hit
//!
//! # Breakpoint Types
//!
//! The system supports two types of breakpoint locations:
//! - **Opcode Breakpoints**: Triggered at specific bytecode addresses and program counters
//! - **Source Breakpoints**: Triggered at specific source file lines (requires source mapping)
//!
//! # Conditional Breakpoints
//!
//! Breakpoints can include optional conditional expressions that must evaluate to true
//! for the breakpoint to be considered "hit". These expressions use the same evaluation
//! engine as the expression RPC methods.
//!
//! # Example Usage
//!
//! ```json
//! // Request - Opcode breakpoint with condition
//! {
//!   "method": "edb_getBreakpointHits",
//!   "params": [{
//!     "loc": {
//!       "Opcode": {
//!         "bytecode_address": "0x742d35Cc6C1C9e5b5b4A7F7c4F7f7f7f7f7f7f7f",
//!         "pc": 42
//!       }
//!     },
//!     "condition": "msg.value > 1000"
//!   }]
//! }
//!
//! // Response
//! {
//!   "result": [15, 23, 45]
//! }
//! ```

use std::sync::Arc;

use alloy_dyn_abi::DynSolValue;
use edb_common::types::{Breakpoint, BreakpointLocation};
use eyre::{bail, Result};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use revm::{database::CacheDB, Database, DatabaseCommit, DatabaseRef};
use serde_json::Value;
use tracing::debug;

use crate::{error_codes, eval, EngineContext, RpcError, Snapshot, SnapshotDetail};

/// Retrieve all snapshots where a breakpoint was hit.
///
/// This is the main breakpoint query method that takes a breakpoint specification
/// and returns all snapshot IDs where that breakpoint would have been triggered.
/// The method performs parallel evaluation across all available snapshots.
///
/// # Parameters
/// - `breakpoint` (Breakpoint) - The breakpoint specification to search for
///
/// # Returns
/// An array of snapshot IDs (numbers) where the breakpoint was hit:
/// ```json
/// [15, 23, 45, 67]
/// ```
///
/// # Breakpoint Structure
/// A breakpoint can specify:
/// - **Location** (optional): Either opcode-based or source-based location
/// - **Condition** (optional): Expression that must evaluate to true
///
/// # Location Types
/// - **Opcode**: `{"Opcode": {"bytecode_address": "0x...", "pc": 42}}`
/// - **Source**: `{"Source": {"bytecode_address": "0x...", "line_number": 15, "file_path": "Contract.sol"}}`
///
/// # Error Conditions
/// - Invalid breakpoint specification
/// - Malformed location or condition parameters
/// - Serialization errors in response
pub fn get_breakpoint_hits<DB>(
    context: &Arc<EngineContext<DB>>,
    params: Option<Value>,
) -> Result<Value, RpcError>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    // Parse the snapshot ID from parameters
    let breakpoint: Breakpoint = params
        .as_ref()
        .and_then(|p| p.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| RpcError {
            code: error_codes::INVALID_PARAMS,
            message: "Invalid params: expected [breakpoint]".to_string(),
            data: None,
        })?;

    let mut snapshot_ids = retrieve_breakpoint_snapshots(context, &breakpoint);
    snapshot_ids.sort();

    let json_value = serde_json::to_value(&snapshot_ids).map_err(|e| RpcError {
        code: error_codes::INTERNAL_ERROR,
        message: format!("Failed to serialize ABI: {e}"),
        data: None,
    })?;

    debug!("Retrieved snapshot IDs for breakpoint {:?}: {:?}", breakpoint, snapshot_ids);
    Ok(json_value)
}

/// Retrieve all snapshot IDs associated with a given breakpoint.
/// This includes snapshots where the breakpoint was hit.
///
/// # Arguments
/// * `context` - The engine context containing snapshots and evaluation capabilities
/// * `breakpoint` - The breakpoint to query
///
/// # Returns
/// A vector of snapshot IDs where the breakpoint was hit
/// or an empty vector if none were found.
fn retrieve_breakpoint_snapshots<DB>(
    context: &Arc<EngineContext<DB>>,
    breakpoint: &Breakpoint,
) -> Vec<usize>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    context
        .snapshots
        .par_iter()
        .filter_map(|(_, snapshot)| check_breakpoint_hit(context, snapshot, breakpoint).ok())
        .collect()
}

/// Check if a given snapshot hits the specified breakpoint.
///
/// This function performs the core breakpoint matching logic by evaluating both
/// location criteria and optional conditional expressions. It supports both opcode-level
/// and source-level breakpoints with different matching strategies.
///
/// # Arguments
/// * `context` - The engine context containing artifacts and analysis results
/// * `snapshot` - The snapshot to check against the breakpoint
/// * `breakpoint` - The breakpoint specification to match
///
/// # Returns
/// * `Ok(snapshot_id)` if the breakpoint is hit at this snapshot
/// * `Err` if the breakpoint is not hit or evaluation fails
///
/// # Matching Logic
/// - **Location matching**: Compares snapshot location against breakpoint location
/// - **Condition matching**: Evaluates optional expression in snapshot context
/// - Both conditions must be satisfied for a successful match
fn check_breakpoint_hit<DB>(
    context: &Arc<EngineContext<DB>>,
    snapshot: &Snapshot<DB>,
    breakpoint: &Breakpoint,
) -> Result<usize>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    let loc_match = check_location_match(context, snapshot, breakpoint);

    let expr_match = match &breakpoint.condition {
        // No expression specified in breakpoint, match any
        None => true,
        Some(expr) => {
            eval::eval_on_snapshot(
                context.clone(),
                &format!("bool({expr})"),
                snapshot.id(), // Use snapshot ID directly
            )
            .is_ok_and(|v| v == DynSolValue::Bool(true))
        }
    };

    if loc_match && expr_match {
        Ok(snapshot.id())
    } else {
        bail!("Breakpoint not hit");
    }
}

fn check_location_match<DB>(
    context: &Arc<EngineContext<DB>>,
    snapshot: &Snapshot<DB>,
    breakpoint: &Breakpoint,
) -> bool
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    match (snapshot.detail(), &breakpoint.loc) {
        // No location specified in breakpoint, match any
        (_, None) => true,
        (
            SnapshotDetail::Opcode(detail),
            Some(BreakpointLocation::Opcode { bytecode_address, pc }),
        ) => detail.bytecode_address == *bytecode_address && detail.pc == *pc,
        (
            SnapshotDetail::Hook(detail),
            Some(BreakpointLocation::Source { bytecode_address, line_number, file_path }),
        ) => {
            if detail.bytecode_address != *bytecode_address {
                false
            } else {
                let Some(analysis_result) = context.analysis_results.get(bytecode_address) else {
                    return false;
                };

                let Some(step_src) =
                    analysis_result.usid_to_step.get(&detail.usid).map(|step| step.src())
                else {
                    return false;
                };

                if analysis_result
                    .sources
                    .get(&step_src.file)
                    .map(|s| s.path != *file_path)
                    .unwrap_or(false)
                {
                    return false;
                }

                context
                    .artifacts
                    .get(bytecode_address)
                    .and_then(|artifact| artifact.input.sources.get(file_path))
                    .zip(Some(step_src.start))
                    .is_some_and(|(source, offset)| {
                        source.content[..offset + 1].lines().count() == *line_number
                    })
            }
        }
        _ => false,
    }
}
