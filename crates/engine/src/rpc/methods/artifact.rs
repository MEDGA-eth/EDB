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

//! Code retrieval RPC method implementation

use std::{collections::HashMap, sync::Arc};

use alloy_primitives::{Address, Bytes};
use edb_common::types::{Code, OpcodeInfo, SourceInfo};
use revm::{Database, DatabaseCommit, DatabaseRef, database::CacheDB};
use serde_json::Value;
use tracing::debug;

use crate::{EngineContext, SnapshotDetail, error_codes, utils::disasm::disassemble};

use super::super::types::RpcError;

/// Get code for a specific snapshot
///
/// This method returns either disassembled opcodes (for opcode snapshots)
/// or source code (for hook snapshots).
///
/// # Parameters
/// - `id`: The snapshot ID (0-indexed)
///
/// # Returns
/// - For opcode snapshots: Disassembled bytecode with PC mappings
/// - For hook snapshots: Source code files from the artifact
pub fn get_code<DB>(
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

    let bytecode_address = trace_entry.code_address;

    let code = match snapshot.detail() {
        SnapshotDetail::Opcode(..) => {
            // For opcode snapshots, return disassembled bytecode
            // Get the bytecode from the database
            let bytecode = trace_entry.bytecode.as_ref().ok_or_else(|| RpcError {
                code: error_codes::CODE_NOT_FOUND,
                message: format!("No bytecode found for trace entry {}", frame_id.trace_entry_id()),
                data: None,
            })?;

            let codes = get_disassembled_code(bytecode);

            Code::Opcode(OpcodeInfo { bytecode_address, codes })
        }
        SnapshotDetail::Hook(..) => {
            // Get the artifact for this address. If there is no direct
            // entry, the address may have been etched (e.g. via `vm.etch`)
            // with another contract's instrumented runtime; in that case
            // the codehash-alias index maps the live bytecode back to a
            // canonical address whose artifact we do have.
            let artifact =
                context.resolve_artifact_via_codehash(bytecode_address).ok_or_else(|| {
                    RpcError {
                        code: error_codes::INVALID_ADDRESS,
                        message: format!("No artifact found for address {bytecode_address}"),
                        data: None,
                    }
                })?;

            // Extract sources from the SolcInput
            let mut sources = HashMap::new();
            for (path, source) in &artifact.input.sources {
                sources.insert(path.clone(), source.content.to_string());
            }

            Code::Source(SourceInfo { bytecode_address, sources })
        }
    };

    // Serialize the Code enum to JSON
    let json_value = serde_json::to_value(code).map_err(|e| RpcError {
        code: error_codes::INTERNAL_ERROR,
        message: format!("Failed to serialize code: {e}"),
        data: None,
    })?;

    debug!("Retrieved code for snapshot {}", snapshot_id);
    Ok(json_value)
}

/// Get code for a specific address
///
/// This method retrieves the code (either opcode or source) associated with a given
/// contract address, if available in the artifact metadata.
///
/// # Parameters
/// - `address`: The contract address
///
/// # Returns
/// - For opcode snapshots: Disassembled bytecode with PC mappings
/// - For hook snapshots: Source code files from the artifact
pub fn get_code_by_address<DB>(
    context: &Arc<EngineContext<DB>>,
    params: Option<Value>,
) -> Result<serde_json::Value, RpcError>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    // Parse the address as the first argument
    let address: Address = params
        .as_ref()
        .and_then(|p| p.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| RpcError {
            code: error_codes::INVALID_PARAMS,
            message: "Invalid params: expected [address]".to_string(),
            data: None,
        })?;

    // Resolve an artifact, trying the direct address first and then
    // falling back through the codehash alias index. The fallback covers
    // the case where `vm.etch(target, address(impl).code)` installs the
    // instrumented runtime of `impl` at `target`: `target` has no
    // artifact of its own, but its live bytecode hashes to a key the
    // engine built from `impl`'s recompiled artifact.
    let artifact = context.resolve_artifact_via_codehash(address);

    let code = match artifact {
        Some(artifact) => {
            // Extract sources from the SolcInput
            let mut sources = HashMap::new();
            for (path, source) in &artifact.input.sources {
                sources.insert(path.clone(), source.content.to_string());
            }
            Code::Source(SourceInfo { bytecode_address: address, sources })
        }
        None => context
            .trace
            .iter()
            .find_map(|entry| {
                if entry.code_address == address {
                    let bytecode = entry.bytecode.as_ref()?;
                    let codes = get_disassembled_code(bytecode);
                    Some(Code::Opcode(OpcodeInfo { bytecode_address: address, codes }))
                } else {
                    None
                }
            })
            .ok_or_else(|| RpcError {
                code: error_codes::CODE_NOT_FOUND,
                message: format!("No code found for address {address}"),
                data: None,
            })?,
    };

    let json_value = serde_json::to_value(code).map_err(|e| RpcError {
        code: error_codes::INTERNAL_ERROR,
        message: format!("Failed to serialize code: {e}"),
        data: None,
    })?;
    debug!("Retrieved code for address {}", address);
    Ok(json_value)
}

/// Get constructor arguments for a contract at a specific address
///
/// This method retrieves the constructor arguments used during the deployment
/// of a contract, if available in the artifact metadata.
///
/// # Parameters
/// - `address`: The contract address
/// # Returns
/// - The constructor arguments as a JSON value, or null if not available
pub fn get_constructor_args<DB>(
    context: &Arc<EngineContext<DB>>,
    params: Option<Value>,
) -> Result<serde_json::Value, RpcError>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    // Parse the address as the first argument
    let address: Address = params
        .as_ref()
        .and_then(|p| p.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| RpcError {
            code: error_codes::INVALID_PARAMS,
            message: "Invalid params: expected [address]".to_string(),
            data: None,
        })?;

    // No codehash fallback: `vm.etch` does not run a constructor, so an
    // etched address has no constructor arguments. Falling back to a
    // canonical artifact's constructor args would silently misrepresent
    // the etched contract's deployment.
    let args =
        context.artifacts.get(&address).map(|artifact| artifact.meta.constructor_arguments.clone());

    let json_value = serde_json::to_value(args).map_err(|e| RpcError {
        code: error_codes::INTERNAL_ERROR,
        message: format!("Failed to serialize ABI: {e}"),
        data: None,
    })?;

    debug!("Retrieved contract ABI for address {}", address);
    Ok(json_value)
}

fn get_disassembled_code(bytecode: &Bytes) -> HashMap<usize, String> {
    let disasm_result = disassemble(bytecode);

    let mut codes = HashMap::new();
    for instruction in disasm_result.instructions {
        let pc = instruction.pc;
        let opcode_str = if instruction.is_push() && !instruction.push_data.is_empty() {
            // Format PUSH instructions with their data
            let data_hex = hex::encode(&instruction.push_data);
            format!("{} 0x{}", instruction.opcode, data_hex)
        } else {
            instruction.opcode.to_string()
        };
        codes.insert(pc, opcode_str);
    }
    codes
}
