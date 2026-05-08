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

use std::sync::Arc;

use alloy_primitives::{U256, map::HashMap};
use revm::{Database, DatabaseCommit, DatabaseRef, database::CacheDB};
use serde_json::Value;
use tracing::debug;

use crate::{EngineContext, RpcError, error_codes};

pub fn get_storage_diff<DB>(
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
            message: "Invalid params: expected [snapshot_id, slot]".to_string(),
            data: None,
        })? as usize;

    let (f_id, snapshot) = context.snapshots.get(snapshot_id).ok_or_else(|| RpcError {
        code: error_codes::SNAPSHOT_OUT_OF_BOUNDS,
        message: format!("Snapshot with id {snapshot_id} not found"),
        data: None,
    })?;

    let target_address = context
        .trace
        .get(f_id.trace_entry_id())
        .ok_or_else(|| RpcError {
            code: error_codes::INTERNAL_ERROR,
            message: format!("Execution frame id {f_id} not found in trace"),
            data: None,
        })?
        .target;

    let empty_storage = HashMap::default();
    let dst_db = snapshot.db();
    let dst_cached_storage = dst_db
        .cache
        .accounts
        .get(&target_address)
        .map(|acc| &acc.storage)
        .unwrap_or(&empty_storage);

    let src_db = context
        .snapshots
        .first()
        .ok_or_else(|| RpcError {
            code: error_codes::SNAPSHOT_OUT_OF_BOUNDS,
            message: "Initial snapshot (id 0) not found".to_string(),
            data: None,
        })?
        .1
        .db();

    let mut changes = HashMap::new();
    for (slot, dst_value) in dst_cached_storage.iter() {
        let src_value = src_db.storage_ref(target_address, *slot).map_err(|e| RpcError {
            code: error_codes::INTERNAL_ERROR,
            message: format!("Failed to retrieve storage at {target_address} for slot {slot}: {e}"),
            data: None,
        })?;
        if &src_value != dst_value {
            changes.insert(*slot, (src_value, *dst_value));
        }
    }

    // Serialize the SnapshotInfo enum to JSON
    let json_value = serde_json::to_value(changes).map_err(|e| RpcError {
        code: error_codes::INTERNAL_ERROR,
        message: format!("Failed to serialize storage diff: {e}"),
        data: None,
    })?;

    debug!("Retrieved storage diff for snapshot {}", snapshot_id);
    Ok(json_value)
}

pub fn get_storage<DB>(
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
            message: "Invalid params: expected [snapshot_id, slot]".to_string(),
            data: None,
        })? as usize;

    // Parse recompiled as the second argument
    let slot: U256 = params
        .as_ref()
        .and_then(|p| p.as_array())
        .and_then(|arr| arr.get(1))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| RpcError {
            code: error_codes::INVALID_PARAMS,
            message: "Invalid params: expected [snapshot_id, slot]".to_string(),
            data: None,
        })?;

    let (f_id, snapshot) = context.snapshots.get(snapshot_id).ok_or_else(|| RpcError {
        code: error_codes::SNAPSHOT_OUT_OF_BOUNDS,
        message: format!("Snapshot with id {snapshot_id} not found"),
        data: None,
    })?;

    let target_address = context
        .trace
        .get(f_id.trace_entry_id())
        .ok_or_else(|| RpcError {
            code: error_codes::INTERNAL_ERROR,
            message: format!("Execution frame id {f_id} not found in trace"),
            data: None,
        })?
        .target;

    let db = snapshot.db();
    let value = db.storage_ref(target_address, slot).map_err(|e| RpcError {
        code: error_codes::INTERNAL_ERROR,
        message: format!("Failed to retrieve storage at {target_address} for slot {slot}: {e}"),
        data: None,
    })?;

    // Serialize the SnapshotInfo enum to JSON
    let json_value = serde_json::to_value(value).map_err(|e| RpcError {
        code: error_codes::INTERNAL_ERROR,
        message: format!("Failed to serialize storage info: {e}"),
        data: None,
    })?;

    debug!("Retrieved storage info for snapshot {}", snapshot_id);
    Ok(json_value)
}
