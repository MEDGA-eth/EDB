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

//! Snapshot management RPC methods.
//!
//! This module implements RPC methods for accessing and navigating execution snapshots.
//! Snapshots capture the execution state at specific points, allowing detailed inspection
//! of variables, memory, stack, and storage at any moment during contract execution.
//!
//! # Available Methods
//!
//! - `edb_getSnapshotCount` - Get the total number of available snapshots
//! - `edb_getSnapshotInfo` - Get detailed information about a specific snapshot
//!
//! # Snapshot Types
//!
//! The system supports two types of snapshots:
//! - **Opcode snapshots** - Low-level EVM state (PC, stack, memory, storage)
//! - **Hook snapshots** - High-level source information (variables, source location)

use std::sync::Arc;

use edb_common::types::{
    HookSnapshotInfoDetail, OpcodeSnapshotInfoDetail, SnapshotInfo, SnapshotInfoDetail,
};
use revm::{database::CacheDB, Database, DatabaseCommit, DatabaseRef};
use serde_json::Value;
use tracing::debug;

use crate::{error_codes, EngineContext, SnapshotDetail};

use super::super::types::RpcError;

/// Get detailed information for a specific snapshot
///
/// This method returns structured information about a snapshot, including:
/// - For opcode snapshots: PC, opcode, memory, stack, calldata, and storage state
/// - For hook snapshots: Source location information (path, offset, length)
///
/// # Parameters
/// - `id`: The snapshot ID (0-indexed)
///
/// # Returns
/// - For opcode snapshots: Complete execution state at that opcode
/// - For hook snapshots: Source code location and debugging info
pub fn get_snapshot_info<DB>(
    context: &Arc<EngineContext<DB>>,
    params: Option<Value>,
) -> Result<Value, RpcError>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    // Parse the snapshot ID from parameters
    let snapshot_id = params
        .as_ref()
        .and_then(|p| p.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError {
            code: error_codes::INVALID_PARAMS,
            message: "Invalid params: expected [snapshot_id]".to_string(),
            data: None,
        })? as usize;

    // Get the snapshot at the specified index
    let (frame_id, snapshot) = context.snapshots.get(snapshot_id).ok_or_else(|| RpcError {
        code: error_codes::SNAPSHOT_OUT_OF_BOUNDS,
        message: format!("Snapshot with id {snapshot_id} not found"),
        data: None,
    })?;

    let trace_entry = context.trace.get(frame_id.trace_entry_id()).ok_or_else(|| RpcError {
        code: error_codes::TRACE_ENTRY_NOT_FOUND,
        message: format!("Trace entry with id {} not found", frame_id.trace_entry_id()),
        data: None,
    })?;

    let snapshot_info = match snapshot.detail() {
        SnapshotDetail::Opcode(opcode_snapshot) => {
            // For opcode snapshots, return complete execution state
            SnapshotInfo {
                id: snapshot.id(),
                frame_id: snapshot.frame_id(),
                next_id: snapshot.next_id().ok_or_else(|| RpcError {
                    code: error_codes::INTERNAL_ERROR,
                    message: format!("We do not find next id for Snapshot {}", snapshot.id()),
                    data: None,
                })?,
                prev_id: snapshot.prev_id().ok_or_else(|| RpcError {
                    code: error_codes::INTERNAL_ERROR,
                    message: format!("We do not find previous id for Snapshot {}", snapshot.id()),
                    data: None,
                })?,
                target_address: snapshot.target_address(),
                bytecode_address: snapshot.bytecode_address(),
                detail: SnapshotInfoDetail::Opcode(OpcodeSnapshotInfoDetail {
                    id: snapshot.id(),
                    frame_id: *frame_id,
                    pc: opcode_snapshot.pc,
                    opcode: opcode_snapshot.opcode,
                    memory: opcode_snapshot.memory.as_ref().clone(),
                    stack: opcode_snapshot.stack.to_vec(),
                    calldata: opcode_snapshot.calldata.as_ref().clone(),
                    transient_storage: opcode_snapshot.transient_storage.as_ref().clone(),
                }),
            }
        }
        SnapshotDetail::Hook(hook_snapshot) => {
            // For hook snapshots, get source location from analysis results
            let bytecode_address = trace_entry.code_address;
            let usid = hook_snapshot.usid;

            // Get the analysis result for this address
            let analysis_result =
                context.analysis_results.get(&bytecode_address).ok_or_else(|| RpcError {
                    code: error_codes::INVALID_ADDRESS,
                    message: format!("No analysis result found for address {bytecode_address}"),
                    data: None,
                })?;

            // Get the step from USID
            let step_ref = analysis_result.usid_to_step.get(&usid).ok_or_else(|| RpcError {
                code: error_codes::USID_NOT_FOUND,
                message: format!("No step found for USID {}", u64::from(usid)),
                data: None,
            })?;

            // Get step details
            let source_location = step_ref.src();

            // Find the source analysis using the source index
            let source_index = source_location.file;
            let source_analysis =
                analysis_result.sources.get(&source_index).ok_or_else(|| RpcError {
                    code: error_codes::CODE_NOT_FOUND,
                    message: format!("No source analysis found for index {source_index}"),
                    data: None,
                })?;

            // Find local variables at this snapshot
            let locals = hook_snapshot.locals.clone();

            // Find state variables at this snapshot
            let state_variables = hook_snapshot.state_variables.clone();

            SnapshotInfo {
                id: snapshot.id(),
                frame_id: snapshot.frame_id(),
                next_id: snapshot.next_id().ok_or_else(|| RpcError {
                    code: error_codes::INTERNAL_ERROR,
                    message: format!("We do not find next id for Snapshot {}", snapshot.id()),
                    data: None,
                })?,
                prev_id: snapshot.prev_id().ok_or_else(|| RpcError {
                    code: error_codes::INTERNAL_ERROR,
                    message: format!("We do not find previous id for Snapshot {}", snapshot.id()),
                    data: None,
                })?,
                target_address: snapshot.target_address(),
                bytecode_address: snapshot.bytecode_address(),
                detail: SnapshotInfoDetail::Hook(HookSnapshotInfoDetail {
                    id: snapshot.id(),
                    frame_id: *frame_id,
                    locals,
                    state_variables,
                    path: source_analysis.path.clone(),
                    offset: source_location.start,
                    length: source_location.length,
                }),
            }
        }
    };

    // Serialize the SnapshotInfo enum to JSON
    let json_value = serde_json::to_value(&snapshot_info).map_err(|e| RpcError {
        code: error_codes::INTERNAL_ERROR,
        message: format!("Failed to serialize snapshot info: {e}"),
        data: None,
    })?;

    debug!("Retrieved snapshot info for snapshot {}", snapshot_id);
    Ok(json_value)
}

/// Get the total number of available snapshots.
///
/// Returns the count of all snapshots captured during the debugging session.
/// This includes both opcode-level and hook-based snapshots.
///
/// # Parameters
/// None - this method takes no parameters
///
/// # Returns
/// The total number of snapshots as a JSON number
///
/// # Example Response
/// ```json
/// 150
/// ```
pub fn get_snapshot_count<DB>(context: &Arc<EngineContext<DB>>) -> Result<Value, RpcError>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    let total_snapshots = context.snapshots.len();
    serde_json::to_value(total_snapshots).map_err(|e| RpcError {
        code: error_codes::INTERNAL_ERROR,
        message: format!("Failed to serialize total snapshots: {e}"),
        data: None,
    })
}
