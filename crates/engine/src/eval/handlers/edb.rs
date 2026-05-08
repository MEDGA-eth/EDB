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

//! EDB handler implementations for the expression evaluator.
//!
//! This module provides concrete implementations of all handler traits that work
//! with EDB's debug snapshot and execution context. The handlers enable real-time
//! expression evaluation over debug modes (opcode mode and source mode).
//!
//! # Key Features
//!
//! - **Variable Resolution**: Access local variables, state variables, and `this`
//! - **Function Calls**: Execute contract functions and EDB pre-compiled functions
//! - **Storage Access**: Read from contract storage, transient storage, memory, and stack
//! - **Blockchain Context**: Access `msg`, `tx`, and `block` global variables
//! - **Cross-Contract Calls**: Function calls and state access on different addresses
//!
//! # Pre-compiled Functions (EDB-Version)
//!
//! - `edb_sload(address, slot)` - Read storage slot
//! - `edb_tsload(address, slot)` - Read transient storage (opcode mode only)
//! - `edb_stack(index)` - Read EVM stack (opcode mode only)
//! - `edb_memory(offset, size)` - Read EVM memory (opcode mode only)
//! - `edb_calldata(offset, size)` - Read call data slice
//! - `keccak256(bytes)` - Compute keccak256 hash
//! - `edb_help()` - Show help information
//!
//! # Usage
//!
//! ```rust,ignore
//! let handlers = EdbHandler::create_handlers(engine_context);
//! let evaluator = ExpressionEvaluator::new(handlers);
//! let result = evaluator.eval("balances[msg.sender]", snapshot_id)?;
//! ```

use std::{collections::HashSet, sync::Arc};

use alloy_dyn_abi::DynSolValue;
use alloy_primitives::U256;
use edb_common::types::{parse_callable_abi_entries, CallableAbiEntry, TraceEntry};
use eyre::{bail, eyre, Result};
use revm::{database::CacheDB, Database, DatabaseCommit, DatabaseRef};
use tracing::debug;

use super::*;
use crate::{ContextEvmTr, ContextQueryTr, EngineContext, Snapshot, SnapshotDetail};

static EDB_EVAL_PLACEHOLDER_MAGIC: &str = "edb_eval_placeholder";

fn from_abi_info(entry: &CallableAbiEntry) -> Option<DynSolValue> {
    if entry.is_function() {
        return None; // Only handle non-function entries
    }
    let magic = DynSolValue::String(EDB_EVAL_PLACEHOLDER_MAGIC.to_string());
    let serial_abi = DynSolValue::String(serde_json::to_string(entry).ok()?);

    Some(DynSolValue::Tuple(vec![magic, serial_abi]))
}

fn into_abi_info(value: &DynSolValue) -> Option<CallableAbiEntry> {
    if let DynSolValue::Tuple(elements) = value
        && elements.len() == 2
            && let DynSolValue::String(magic) = &elements[0]
                && magic == EDB_EVAL_PLACEHOLDER_MAGIC
                    && let DynSolValue::String(serial_abi) = &elements[1] {
                        return serde_json::from_str(serial_abi).ok();
                    }
    None
}

/// EDB-specific handler that uses EdbContext to resolve values
pub struct EdbHandler<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    context: Arc<EngineContext<DB>>,
}

impl<DB> EdbHandler<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    /// Create a new EDB handler with the given engine context.
    ///
    /// # Arguments
    /// * `context` - The EDB engine context containing snapshots, trace, and recompiled artifacts
    ///
    /// # Returns
    /// A new [`EdbHandler`] instance
    pub fn new(context: Arc<EngineContext<DB>>) -> Self {
        Self { context }
    }

    /// Create all handlers using this EDB context
    pub fn create_handlers(context: Arc<EngineContext<DB>>) -> EvaluatorHandlers {
        let handler = Arc::new(Self::new(context));

        EvaluatorHandlers::new()
            .with_variable_handler(Box::new(EdbVariableHandler(handler.clone())))
            .with_mapping_array_handler(Box::new(EdbMappingArrayHandler(handler.clone())))
            .with_function_call_handler(Box::new(EdbFunctionCallHandler(handler.clone())))
            .with_member_access_handler(Box::new(EdbMemberAccessHandler(handler.clone())))
            .with_msg_handler(Box::new(EdbMsgHandler(handler.clone())))
            .with_tx_handler(Box::new(EdbTxHandler(handler.clone())))
            .with_block_handler(Box::new(EdbBlockHandler(handler.clone())))
            .with_validation_handler(Box::new(EdbValidationHandler(handler)))
    }
}

// Wrapper structs for each handler trait

/// EDB implementation of [`VariableHandler`].
///
/// Resolves variable names to their values using debug snapshot context.
/// Supports local variables, state variables, mappings, arrays, and the special `this` variable.
#[derive(Clone)]
pub struct EdbVariableHandler<DB>(Arc<EdbHandler<DB>>)
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync;

/// EDB implementation of [`MappingArrayHandler`].
///
/// Handles mapping and array access operations by either reading from debug snapshot
/// data or making EVM calls for storage-based mappings.
#[derive(Clone)]
pub struct EdbMappingArrayHandler<DB>(Arc<EdbHandler<DB>>)
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync;

/// EDB implementation of [`FunctionCallHandler`].
///
/// Executes function calls including:
/// - Contract functions (view/pure functions)
/// - EDB pre-compiled functions (`edb_sload`, `edb_stack`, etc.)
/// - Built-in functions (`keccak256`)
/// - Cross-contract function calls
#[derive(Clone)]
pub struct EdbFunctionCallHandler<DB>(Arc<EdbHandler<DB>>)
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync;

/// EDB implementation of [`MemberAccessHandler`].
///
/// Handles member access operations on addresses, enabling access to contract
/// state variables and view functions on different addresses.
#[derive(Clone)]
pub struct EdbMemberAccessHandler<DB>(Arc<EdbHandler<DB>>)
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync;

/// EDB implementation of [`MsgHandler`].
///
/// Provides access to transaction context variables (`msg.sender`, `msg.value`)
/// from the current debug snapshot's trace entry.
#[derive(Clone)]
pub struct EdbMsgHandler<DB>(Arc<EdbHandler<DB>>)
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync;

/// EDB implementation of [`TxHandler`].
///
/// Provides access to transaction-level context (`tx.origin`) from the root
/// trace entry of the current debug session.
#[derive(Clone)]
pub struct EdbTxHandler<DB>(Arc<EdbHandler<DB>>)
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync;

/// EDB implementation of [`BlockHandler`].
///
/// Provides access to blockchain context (`block.number`, `block.timestamp`)
/// from the EDB engine's fork information and block data.
#[derive(Clone)]
pub struct EdbBlockHandler<DB>(Arc<EdbHandler<DB>>)
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync;

/// EDB implementation of [`ValidationHandler`].
///
/// Validates final expression results, ensuring that placeholder values (like ABI info)
/// are not returned as final results and must be resolved to concrete values.
#[allow(dead_code)]
#[derive(Clone)]
pub struct EdbValidationHandler<DB>(Arc<EdbHandler<DB>>)
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync;

// Implement handler traits for each wrapper
impl<DB> VariableHandler for EdbVariableHandler<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    fn get_variable_value(&self, name: &str, snapshot_id: usize) -> Result<DynSolValue> {
        let snapshot = &self
            .0
            .context
            .snapshots
            .get(snapshot_id)
            .ok_or_else(|| {
                eyre::eyre!(
                    "Snapshot ID {} not found in EdbHandler::get_variable_value",
                    snapshot_id
                )
            })?
            .1;

        if name == "this" {
            return Ok(DynSolValue::Address(snapshot.target_address()));
        }

        let SnapshotDetail::Hook(detail) = &snapshot.detail() else {
            bail!("Cannot get variable value from opcode snapshot (except this)");
        };

        // Let's first check whether this could be a local or state variable
        if let Some(Some(value)) =
            detail.locals.get(name).or_else(|| detail.state_variables.get(name))
        {
            return Ok((**value).clone().into());
        }

        // Next, it might be a mapping/array variable
        let bytecode_address = snapshot.bytecode_address();
        let Some(contract) = self
            .0
            .context
            .recompiled_artifacts
            .get(&bytecode_address)
            .and_then(|art| art.contract())
        else {
            bail!("No contract found for bytecode address {:?}", bytecode_address);
        };

        for entry in parse_callable_abi_entries(contract) {
            if entry.name == name && entry.is_state_variable()
                && let Some(value) = from_abi_info(&entry) {
                    return Ok(value);
                }
        }

        bail!("No value found for name='{}', snapshot_id={}", name, snapshot_id)
    }
}

impl<DB> MappingArrayHandler for EdbMappingArrayHandler<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    fn get_mapping_or_array_value(
        &self,
        root: DynSolValue,
        indices: Vec<DynSolValue>,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        if let Some(abi_info) = into_abi_info(&root) {
            let (_, info) = self.0.context.snapshots.get(snapshot_id).ok_or_else(|| {
                eyre::eyre!(
                    "Snapshot ID {} not found in EdbHandler::get_mapping_or_array_value",
                    snapshot_id
                )
            })?;
            let to = info.target_address();

            self.0.context.call_in_derived_evm(snapshot_id, to, &abi_info.abi, &indices, None)
        } else {
            // Handle direct DynSolValue types recursively
            if indices.is_empty() {
                return Ok(root);
            }

            let (first_index, remaining_indices) = indices.split_first().unwrap();

            let next_value = match &root {
                DynSolValue::Tuple(elements) => {
                    // For tuples, index should be a uint
                    match first_index {
                        DynSolValue::Uint(idx, _) => {
                            let idx = idx.to::<usize>();
                            if idx >= elements.len() {
                                bail!(
                                    "Index {} out of bounds for tuple with {} elements",
                                    idx,
                                    elements.len()
                                );
                            }
                            elements[idx].clone()
                        }
                        _ => bail!(
                            "Invalid index type for tuple access: expected uint, got {:?}",
                            first_index
                        ),
                    }
                }
                DynSolValue::Array(elements) => {
                    // For arrays, index should be a uint
                    match first_index {
                        DynSolValue::Uint(idx, _) => {
                            let idx = idx.to::<usize>();
                            if idx >= elements.len() {
                                bail!(
                                    "Index {} out of bounds for array with {} elements",
                                    idx,
                                    elements.len()
                                );
                            }
                            elements[idx].clone()
                        }
                        _ => bail!(
                            "Invalid index type for array access: expected uint, got {:?}",
                            first_index
                        ),
                    }
                }
                DynSolValue::FixedArray(elements) => {
                    // For fixed arrays, index should be a uint
                    match first_index {
                        DynSolValue::Uint(idx, _) => {
                            let idx = idx.to::<usize>();
                            if idx >= elements.len() {
                                bail!(
                                    "Index {} out of bounds for fixed array with {} elements",
                                    idx,
                                    elements.len()
                                );
                            }
                            elements[idx].clone()
                        }
                        _ => bail!(
                            "Invalid index type for fixed array access: expected uint, got {:?}",
                            first_index
                        ),
                    }
                }
                _ => {
                    bail!(
                        "Cannot index into value of type {:?} with index {:?}",
                        root,
                        first_index
                    );
                }
            };

            // Recursively handle remaining indices - this is key because next_value might have abi_info
            self.get_mapping_or_array_value(next_value, remaining_indices.to_vec(), snapshot_id)
        }
    }
}

fn edb_keccak256(bytes: DynSolValue) -> Result<DynSolValue> {
    match bytes {
        DynSolValue::Bytes(b) => {
            let hash = alloy_primitives::keccak256(&b);
            Ok(DynSolValue::Bytes(hash.to_vec()))
        }
        DynSolValue::FixedBytes(b, n_bytes) => {
            // Change the bytes to a Vec<u8> of length n_bytes
            if n_bytes > 32 {
                bail!("FixedBytes length {} exceeds maximum of 32", n_bytes);
            } else {
                let mut v = vec![0u8; n_bytes];
                v.copy_from_slice(&b[..n_bytes]);
                let hash = alloy_primitives::keccak256(&v);
                Ok(DynSolValue::Bytes(hash.to_vec()))
            }
        }
        _ => {
            bail!("Invalid argument to edb_keccak256: expected bytes, got {:?}", bytes);
        }
    }
}

fn edb_sload<DB>(
    snapshot: &Snapshot<DB>,
    address: &DynSolValue,
    slot: &DynSolValue,
) -> Result<DynSolValue>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    match (address, slot) {
        (DynSolValue::Address(address), DynSolValue::Uint(slot, ..)) => {
            let db = snapshot.db();
            let cached_storage = db
                .cache
                .accounts
                .get(address)
                .map(|acc| &acc.storage)
                .ok_or(eyre!("Account {:?} not found in edb_sload", address))?;
            let value = cached_storage.get(slot).cloned().unwrap_or_default();

            Ok(DynSolValue::Uint(value, 256))
        }
        _ => {
            bail!(
                "Invalid arguments to edb_sload: expected (address, u256), got ({:?}, {:?})",
                address,
                slot
            );
        }
    }
}

fn edb_stack<DB>(snapshot: &Snapshot<DB>, index: &DynSolValue) -> Result<DynSolValue>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    let SnapshotDetail::Opcode(detail) = snapshot.detail() else {
        bail!("edb_stack can only be called from an opcode snapshot");
    };

    match index {
        DynSolValue::Uint(idx, ..) => {
            let idx = idx.to::<usize>();
            let stack = &detail.stack;
            if idx >= stack.len() {
                bail!("Index {} out of bounds for stack with {} elements", idx, stack.len());
            }
            stack.peek(idx).ok_or(eyre!("Failed to peek stack")).map(|v| DynSolValue::Uint(*v, 256))
        }
        _ => {
            bail!("Invalid argument to edb_stack: expected u256, got {:?}", index);
        }
    }
}

fn edb_tsload<DB>(
    snapshot: &Snapshot<DB>,
    address: &DynSolValue,
    slot: &DynSolValue,
) -> Result<DynSolValue>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    let SnapshotDetail::Opcode(detail) = snapshot.detail() else {
        bail!("edb_tsload can only be called from an opcode snapshot");
    };

    match (address, slot) {
        (DynSolValue::Address(addr), DynSolValue::Uint(idx, ..)) => {
            let value = detail.transient_storage.get(&(*addr, *idx)).cloned().unwrap_or_default();
            Ok(DynSolValue::Uint(value, 256))
        }
        _ => {
            bail!(
                "Invalid arguments to edb_tsload: expected (address, u256), got ({:?}, {:?})",
                address,
                slot
            );
        }
    }
}

fn edb_memory<DB>(
    snapshot: &Snapshot<DB>,
    offset: &DynSolValue,
    size: &DynSolValue,
) -> Result<DynSolValue>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    let SnapshotDetail::Opcode(detail) = snapshot.detail() else {
        bail!("edb_memory can only be called from an opcode snapshot");
    };

    match (offset, size) {
        (DynSolValue::Uint(off, ..), DynSolValue::Uint(sz, ..)) => {
            let off = off.to::<usize>();
            let sz = sz.to::<usize>();
            let memory = &detail.memory;
            if off + sz > memory.len() {
                bail!(
                    "edb_memory out of bounds: offset {} + size {} > memory length {}",
                    off,
                    sz,
                    memory.len()
                );
            }
            let slice = &memory[off..off + sz];
            Ok(DynSolValue::Bytes(slice.to_vec()))
        }
        _ => {
            bail!(
                "Invalid arguments to edb_memory: expected (u256, u256), got ({:?}, {:?})",
                offset,
                size
            );
        }
    }
}

fn edb_calldata(
    entry: &TraceEntry,
    offset: &DynSolValue,
    size: &DynSolValue,
) -> Result<DynSolValue> {
    match (offset, size) {
        (DynSolValue::Uint(off, ..), DynSolValue::Uint(sz, ..)) => {
            let off = off.to::<usize>();
            let sz = sz.to::<usize>();
            let calldata = &entry.input;
            if off + sz > calldata.len() {
                bail!(
                    "edb_calldata out of bounds: offset {} + size {} > calldata length {}",
                    off,
                    sz,
                    calldata.len()
                );
            }
            let slice = &calldata[off..off + sz];
            Ok(DynSolValue::Bytes(slice.to_vec()))
        }
        _ => {
            bail!(
                "Invalid arguments to edb_calldata: expected (u256, u256), got ({:?}, {:?})",
                offset,
                size
            );
        }
    }
}

fn edb_help() -> Result<DynSolValue> {
    let help_text = r#"EDB Expression Evaluator Help

OVERVIEW:
Real-time expression evaluation over debug modes. Evaluate Solidity-like expressions
against contract state, variables, and blockchain context at any execution point.

VARIABLES:
• this                    - Current contract address
• Local variables         - Access by name (e.g., balance, owner)
• State variables         - Access by name (e.g., totalSupply, users)
• Mapping/array variables - Access with bracket notation (e.g., balances[addr])

BLOCKCHAIN CONTEXT:
• msg.sender     - Transaction sender address
• msg.value      - Transaction value in wei
• tx.origin      - Original transaction sender
• block.number   - Current block number
• block.timestamp - Current block timestamp

PRE-COMPILED FUNCTIONS (EDB-VERSION):
• edb_sload(address, slot)      - Read storage slot from address
• edb_tsload(address, slot)     - Read transient storage (opcode mode only)
• edb_stack(index)              - Read EVM stack value (opcode mode only)
• edb_memory(offset, size)      - Read EVM memory (opcode mode only)
• edb_calldata(offset, size)    - Read call data slice
• keccak256(bytes)              - Compute keccak256 hash
• edb_help()                    - Show this help

CONTRACT FUNCTIONS:
• Call any contract function by name with arguments
• Access state variables and view functions
• Member access on addresses (e.g., addr.balanceOf(user))
• Cross-contract calls (e.g., token.transfer(to, amount))
• State variable access on different addresses (e.g., addr.owner)

OPERATORS:
• Arithmetic: +, -, *, /, %, **
• Comparison: ==, !=, <, <=, >, >=
• Logical: &&, ||, !
• Bitwise: &, |, ^, ~, <<, >>
• Ternary: condition ? true_value : false_value

EXAMPLES:
• balances[msg.sender]
• totalSupply() > 1000000
• block.timestamp - lastUpdate > 3600
• edb_sload(this, 0x123...)
• owner == msg.sender && msg.value > 0
• token.balanceOf(user) * price / 1e18
• addr.owner == this
• contractAddr.getUserBalance(msg.sender)

Note: Use 'this' to reference the current contract address in expressions."#;

    Ok(DynSolValue::String(help_text.to_string()))
}

impl<DB> FunctionCallHandler for EdbFunctionCallHandler<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    fn call_function(
        &self,
        name: &str,
        args: &[DynSolValue],
        callee: Option<&DynSolValue>,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        debug!(
            "EdbHandler::call_function name='{}', args={:?}, callee={:?}, snapshot_id={}",
            name, args, callee, snapshot_id
        );

        let (frame_id, snapshot) = self.0.context.snapshots.get(snapshot_id).ok_or_else(|| {
            eyre::eyre!("Snapshot ID {} not found in EdbHandler::call_function", snapshot_id)
        })?;

        let entry = self.0.context.trace.get(frame_id.trace_entry_id()).ok_or_else(|| {
            eyre::eyre!("Frame ID {} not found in EdbHandler::call_function", frame_id)
        })?;

        // Let's first handle our edb-specific pseudo-functions
        if name == "edb_sload" && args.len() == 2 {
            return edb_sload(snapshot, &args[0], &args[1]);
        } else if name == "edb_tsload" && args.len() == 2 {
            return edb_tsload(snapshot, &args[0], &args[1]);
        } else if name == "edb_stack" && args.len() == 1 {
            return edb_stack(snapshot, &args[0]);
        } else if name == "edb_calldata" && args.len() == 2 {
            return edb_calldata(entry, &args[0], &args[1]);
        } else if name == "edb_memory" && args.len() == 2 {
            return edb_memory(snapshot, &args[0], &args[1]);
        } else if name == "keccak256" && args.len() == 1 {
            return edb_keccak256(args[0].clone());
        } else if name == "edb_help" && args.is_empty() {
            return edb_help();
        }

        // Let's then handle calls to functions in the contract's ABI
        let to = if let Some(v) = callee {
            match v {
                DynSolValue::Address(addr) => *addr,
                _ => {
                    bail!("Callee must be an address, got {:?}", callee);
                }
            }
        } else {
            snapshot.target_address()
        };

        let mut address_candidates = self
            .0
            .context
            .address_code_address_map()
            .get(&to)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        address_candidates.insert(0, to); // Prioritize the direct address

        let mut errors = Vec::new();
        for address_candidate in address_candidates {
            if let Some(contract) = self
                .0
                .context
                .recompiled_artifacts
                .get(&address_candidate)
                .and_then(|art| art.contract())
            {
                for entry in parse_callable_abi_entries(contract) {
                    if entry.name == name && entry.inputs.len() == args.len() {
                        match self.0.context.call_in_derived_evm(
                            snapshot_id,
                            to,
                            &entry.abi,
                            args,
                            None,
                        ) {
                            Ok(result) => return Ok(result),
                            Err(e) => {
                                errors.push(e);
                                debug!(
                                    "Function '{}' found in contract at address {:?}, but call failed",
                                    name, address_candidate
                                );
                            }
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            bail!(
                "No function found for name='{}', args={:?}, callee={:?}, snapshot_id={}",
                name,
                args,
                callee,
                snapshot_id
            )
        } else {
            let combined_error = errors
                .into_iter()
                .map(|e| format!("{e}"))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(";\n");
            bail!(
                "Function call failed for name='{}', args={:?}, callee={:?}, snapshot_id={}:\n{}",
                name,
                args,
                callee,
                snapshot_id,
                combined_error
            )
        }
    }
}

impl<DB> MemberAccessHandler for EdbMemberAccessHandler<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    fn access_member(
        &self,
        value: DynSolValue,
        member: &str,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        if let DynSolValue::Address(addr) = value {
            let mut address_candidates = self
                .0
                .context
                .address_code_address_map()
                .get(&addr)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect::<Vec<_>>();
            address_candidates.insert(0, addr); // Prioritize the direct address

            for address_candidate in address_candidates {
                if let Some(contract) = self
                    .0
                    .context
                    .recompiled_artifacts
                    .get(&address_candidate)
                    .and_then(|art| art.contract())
                {
                    for entry in parse_callable_abi_entries(contract) {
                        if entry.name == member
                            && entry.is_state_variable()
                            && entry.inputs.is_empty()
                        {
                            match self.0.context.call_in_derived_evm(
                                snapshot_id,
                                addr,
                                &entry.abi,
                                &[],
                                None,
                            ) {
                                Ok(result) => return Ok(result),
                                Err(_e) => {
                                    debug!(
                                    "Function '{}' found in contract at address {:?}, but call failed",
                                    member, address_candidate
                                );
                                }
                            }
                        }
                    }
                }
            }
        }

        bail!(
            "Invalid member access for value={:?}, member='{}', snapshot_id={}",
            value,
            member,
            snapshot_id
        )
    }
}

impl<DB> MsgHandler for EdbMsgHandler<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    fn get_msg_sender(&self, snapshot_id: usize) -> Result<DynSolValue> {
        let (frame_id, _) = self.0.context.snapshots.get(snapshot_id).ok_or_else(|| {
            eyre::eyre!("Snapshot ID {} not found in EdbHandler::get_msg_sender", snapshot_id)
        })?;

        let entry = self.0.context.trace.get(frame_id.trace_entry_id()).ok_or_else(|| {
            eyre::eyre!("Frame ID {} not found in EdbHandler::get_msg_sender", frame_id)
        })?;

        Ok(DynSolValue::Address(entry.caller))
    }

    fn get_msg_value(&self, snapshot_id: usize) -> Result<DynSolValue> {
        let (frame_id, _) = self.0.context.snapshots.get(snapshot_id).ok_or_else(|| {
            eyre::eyre!("Snapshot ID {} not found in EdbHandler::get_msg_value", snapshot_id)
        })?;

        let entry = self.0.context.trace.get(frame_id.trace_entry_id()).ok_or_else(|| {
            eyre::eyre!("Frame ID {} not found in EdbHandler::get_msg_value", frame_id)
        })?;

        Ok(DynSolValue::Uint(entry.value, 256))
    }
}

impl<DB> TxHandler for EdbTxHandler<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    fn get_tx_origin(&self, _snapshot_id: usize) -> Result<DynSolValue> {
        let entry = self
            .0
            .context
            .trace
            .first()
            .ok_or_else(|| eyre::eyre!("No trace entry found in EdbHandler::get_tx_origin"))?;

        Ok(DynSolValue::Address(entry.caller))
    }
}

impl<DB> BlockHandler for EdbBlockHandler<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    fn get_block_number(&self, _snapshot_id: usize) -> Result<DynSolValue> {
        Ok(DynSolValue::Uint(U256::from(self.0.context.fork_info.block_number), 256))
    }

    fn get_block_timestamp(&self, _snapshot_id: usize) -> Result<DynSolValue> {
        Ok(DynSolValue::Uint(self.0.context.block.timestamp, 256))
    }
}

impl<DB> ValidationHandler for EdbValidationHandler<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    fn validate_value(&self, value: DynSolValue) -> Result<DynSolValue> {
        if into_abi_info(&value).is_some() {
            bail!("Mapping or array value cannot be directly returned; please access a member or call a function to get a concrete value");
        } else {
            Ok(value)
        }
    }
}
