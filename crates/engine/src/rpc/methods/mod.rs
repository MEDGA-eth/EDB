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

//! JSON-RPC method implementations for EDB debugging API.
//!
//! This module contains all the RPC method implementations organized by functionality.
//! Each sub-module provides specific debugging capabilities:
//!
//! # Method Categories
//!
//! ## Artifact Management ([`artifact`])
//! - `edb_getCode` - Retrieve contract bytecode
//! - `edb_getConstructorArgs` - Get constructor arguments
//!
//! ## Expression Evaluation ([`expr`])
//! - `edb_evalOnSnapshot` - Evaluate expressions against snapshots
//!
//! ## Navigation ([`navigation`])
//! - `edb_getNextCall` - Navigate to next function call
//! - `edb_getPrevCall` - Navigate to previous function call
//!
//! ## Resolution ([`resolve`])
//! - `edb_getContractABI` - Resolve contract ABI information
//! - `edb_getCallableABI` - Get callable function ABI details
//!
//! ## Snapshot Management ([`snapshot`])
//! - `edb_getSnapshotCount` - Get total number of snapshots
//! - `edb_getSnapshotInfo` - Get detailed snapshot information
//!
//! ## Storage Inspection ([`storage`])
//! - `edb_getStorage` - Read contract storage at specific snapshot
//! - `edb_getStorageDiff` - Compare storage between snapshots
//!
//! ## Trace Analysis ([`trace`])
//! - `edb_getTrace` - Get complete execution trace
//!
//! # Architecture
//!
//! All methods are stateless and operate through the [`MethodHandler`] which
//! provides access to the immutable debugging context. Methods follow a consistent
//! pattern of parameter validation, operation execution, and result serialization.

mod artifact;
mod breakpoint;
mod expr;
mod navigation;
mod resolve;
mod snapshot;
mod storage;
mod trace;

use super::types::RpcError;
use crate::{EngineContext, error_codes};
use revm::{Database, DatabaseCommit, DatabaseRef, database::CacheDB};
use std::sync::Arc;
use tracing::debug;

/// Stateless RPC method dispatcher for EDB debugging API.
///
/// This handler provides a centralized entry point for all RPC methods.
/// It maintains a reference to the immutable debugging context and routes
/// method calls to their appropriate implementation modules.
///
/// The handler is designed to be thread-safe and stateless, allowing
/// concurrent processing of RPC requests without side effects.
pub struct MethodHandler<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    /// Immutable debugging context providing read-only access to debugging data
    context: Arc<EngineContext<DB>>,
}

impl<DB> MethodHandler<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    /// Create a new method handler
    pub fn new(context: Arc<EngineContext<DB>>) -> Self {
        Self { context }
    }

    /// Handle an RPC method call with client-provided state
    pub async fn handle_method(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, RpcError> {
        debug!("Handling RPC method: {}", method);

        match method {
            "edb_getTrace" => trace::get_trace(&self.context),
            "edb_getCode" => artifact::get_code(&self.context, params),
            "edb_getCodeByAddress" => artifact::get_code_by_address(&self.context, params),
            "edb_getConstructorArgs" => artifact::get_constructor_args(&self.context, params),
            "edb_getSnapshotCount" => snapshot::get_snapshot_count(&self.context),
            "edb_getSnapshotInfo" => snapshot::get_snapshot_info(&self.context, params),
            "edb_getContractABI" => resolve::get_contract_abi(&self.context, params),
            "edb_getCallableABI" => resolve::get_callable_abi(&self.context, params),
            "edb_getNextCall" => navigation::get_next_call(&self.context, params),
            "edb_getPrevCall" => navigation::get_prev_call(&self.context, params),
            "edb_getStorage" => storage::get_storage(&self.context, params),
            "edb_getStorageDiff" => storage::get_storage_diff(&self.context, params),
            "edb_evalOnSnapshot" => expr::eval_on_snapshot(&self.context, params),
            "edb_getBreakpointHits" => breakpoint::get_breakpoint_hits(&self.context, params),
            // Unimplemented methods
            _ => Err(RpcError {
                code: error_codes::METHOD_NOT_FOUND,
                message: format!("Method '{method}' not found"),
                data: None,
            }),
        }
    }
}
