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

//! Information management for TUI panels
//!
//! This module implements a two-tier architecture for info management:
//!
//! - `InfoManager`: Per-thread instance for immediate data access during rendering
//! - `InfoManagerCore`: Shared core that handles RPC communication and data fetching
//!
//! This design ensures rendering threads never block on network I/O while
//! maintaining consistency across the application.

use crate::{
    data::manager::core::{
        FetchCache, ManagerCore, ManagerInner, ManagerRequestTr, ManagerStateTr, ManagerTr,
    },
    rpc::RpcClient,
};
use alloy_dyn_abi::{DynSolValue, EventExt, FunctionExt, JsonAbiExt};
use alloy_json_abi::JsonAbi;
use alloy_primitives::{Address, Bytes, LogData, Selector, U256, hex};
use edb_common::types::{
    CallableAbiInfo, EdbSolValue, SolValueFormatter, SolValueFormatterContext as FormatCtx,
};
use eyre::Result;
use std::{collections::HashSet, ops::Deref, sync::Arc};
use tokio::sync::RwLock;
use tracing::debug;

// Generate a unique expression identifier by removing whitespace
fn remove_whitespace(expr: &str) -> String {
    expr.replace(|c: char| c.is_whitespace(), "")
}

#[derive(Debug, Clone, Default)]
pub struct ResolverState {
    contract_abi: FetchCache<(Address, bool), JsonAbi>,
    callable_abi: FetchCache<Address, Vec<CallableAbiInfo>>,
    constructor_args: FetchCache<Address, Bytes>,
    expr_value: FetchCache<(usize, String), core::result::Result<EdbSolValue, String>>,
}

impl ManagerStateTr for ResolverState {
    async fn with_rpc_client(_rpc_client: Arc<RpcClient>) -> Result<Self> {
        Ok(Self::default())
    }

    fn update(&mut self, other: &Self) {
        if self.contract_abi.need_update(&other.contract_abi) {
            self.contract_abi.update(&other.contract_abi);
        }

        if self.callable_abi.need_update(&other.callable_abi) {
            self.callable_abi.update(&other.callable_abi);
        }

        if self.constructor_args.need_update(&other.constructor_args) {
            self.constructor_args.update(&other.constructor_args);
        }

        if self.expr_value.need_update(&other.expr_value) {
            self.expr_value.update(&other.expr_value);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResolverRequest {
    /// Request for contract ABI
    ContractAbi(Address, bool),

    /// Request for callable ABI list
    CallableAbi(Address),

    /// Request for contract constructor arguments
    ConstructorArgs(Address),

    /// Evaluate expression on snapshot
    ExprOnSnapshot(usize, String),
}

impl ManagerRequestTr<ResolverState> for ResolverRequest {
    async fn fetch_data(self, rpc_client: Arc<RpcClient>, state: &mut ResolverState) -> Result<()> {
        match self {
            Self::ContractAbi(address, recompiled) => {
                if state.contract_abi.has_cached(&(address, recompiled)) {
                    return Ok(());
                }
                let abi = rpc_client.get_contract_abi(address, recompiled).await?;
                state.contract_abi.insert((address, recompiled), abi);
            }
            Self::CallableAbi(address) => {
                if state.callable_abi.has_cached(&address) {
                    return Ok(());
                }
                let abi_list = rpc_client.get_callable_abi(address).await?;
                state.callable_abi.insert(address, Some(abi_list));
            }
            Self::ConstructorArgs(address) => {
                if state.constructor_args.has_cached(&address) {
                    return Ok(());
                }
                let args = rpc_client.get_constructor_args(address).await?;
                state.constructor_args.insert(address, args);
            }
            Self::ExprOnSnapshot(snapshot_id, expr) => {
                if state.expr_value.has_cached(&(snapshot_id, expr.clone())) {
                    return Ok(());
                }
                let value = rpc_client.eval_on_snapshot(snapshot_id, &expr).await?;
                state.expr_value.insert((snapshot_id, expr), Some(value));
            }
        }
        Ok(())
    }
}

/// Per-thread info manager providing cached data for rendering
///
/// # Design Philosophy
///
/// `Resolver` follows the same pattern as ThemeManager:
/// - All data reads are immediate and non-blocking
/// - Complex RPC operations are delegated to InfoManagerCore
/// - Data synchronization happens explicitly via `fetch_data()`
///
/// # Usage Pattern
///
/// ```ignore
/// // When data updates are needed
/// resolver.fetch_data().await?; // Sync with core
///
/// // During rendering - immediate access to cached data
/// // (future: add cached fields here for immediate reads)
/// ```
#[derive(Debug, Clone)]
pub struct Resolver {
    /// Pending requests
    pending_requests: HashSet<ResolverRequest>,
    state: ResolverState,

    core: Arc<RwLock<ManagerCore<ResolverState, ResolverRequest>>>,
}

impl Deref for Resolver {
    type Target = ResolverState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

// Data management functions
impl ManagerTr<ResolverState, ResolverRequest> for Resolver {
    fn get_core(&self) -> Arc<RwLock<ManagerCore<ResolverState, ResolverRequest>>> {
        self.core.clone()
    }

    fn get_inner<'a>(&'a mut self) -> ManagerInner<'a, ResolverState, ResolverRequest> {
        ManagerInner {
            core: &mut self.core,
            state: &mut self.state,
            pending_requests: &mut self.pending_requests,
        }
    }
}

// Resolving functions
impl Resolver {
    /// Create a new info manager with a shared core
    pub async fn new(core: Arc<RwLock<ManagerCore<ResolverState, ResolverRequest>>>) -> Self {
        Self {
            pending_requests: HashSet::new(),
            state: core.clone().read().await.state.clone(),
            core,
        }
    }

    /// Fetch the constructor arguments for a specific address
    pub fn get_constructor_args(&mut self, address: Address) -> Option<&Bytes> {
        let _ = self.pull_from_core(); // Try to update cache

        if !self.state.constructor_args.contains_key(&address) {
            debug!("Constructor arguments not found in cache, fetching...");
            self.new_fetching_request(ResolverRequest::ConstructorArgs(address));
            return None;
        }

        match self.state.constructor_args.get(&address) {
            Some(args) => args.as_ref(),
            _ => None,
        }
    }

    /// Fetch the callable ABI list for a specific address
    pub fn get_callable_abi_list(&mut self, address: Address) -> Option<&Vec<CallableAbiInfo>> {
        let _ = self.pull_from_core(); // Try to update cache

        if !self.state.callable_abi.contains_key(&address) {
            debug!("Callable ABI list not found in cache, fetching...");
            self.new_fetching_request(ResolverRequest::CallableAbi(address));
            return None;
        }

        match self.state.callable_abi.get(&address) {
            Some(abi) => abi.as_ref(),
            _ => None,
        }
    }

    /// Fetch the contract ABI for a specific address
    pub fn get_contract_abi(&mut self, address: Address, recompiled: bool) -> Option<&JsonAbi> {
        let _ = self.pull_from_core(); // Try to update cache

        if !self.state.contract_abi.contains_key(&(address, recompiled)) {
            debug!("Contract ABI not found in cache, fetching...");
            self.new_fetching_request(ResolverRequest::ContractAbi(address, recompiled));
            return None;
        }

        match self.state.contract_abi.get(&(address, recompiled)) {
            Some(abi) => abi.as_ref(),
            _ => None,
        }
    }

    /// Evaluate an expression on a specific snapshot
    pub fn eval_on_snapshot(
        &mut self,
        snapshot_id: usize,
        expr: &str,
    ) -> Option<&core::result::Result<EdbSolValue, String>> {
        let _ = self.pull_from_core(); // Try to update cache
        let expr_key = remove_whitespace(expr);

        if !self.state.expr_value.contains_key(&(snapshot_id, expr_key.clone())) {
            debug!("Expression value not found in cache, fetching...");
            self.new_fetching_request(ResolverRequest::ExprOnSnapshot(snapshot_id, expr_key));
            return None;
        }

        match self.state.expr_value.get(&(snapshot_id, expr_key)) {
            Some(value) => value.as_ref(),
            _ => None,
        }
    }

    /// Resolve function return
    pub fn resolve_function_return(
        &mut self,
        calldata: &Bytes,
        output: &Bytes,
        address: Option<Address>,
    ) -> Option<String> {
        if calldata.len() < 4 {
            return None;
        }

        let selector = Selector::from_slice(&calldata[..4]);
        if let Some(function_abi) = address
            .and_then(|addr| self.get_contract_abi(addr, false))
            .and_then(|abi| abi.function_by_selector(selector).cloned())
        {
            match function_abi.abi_decode_output(output) {
                Ok(decoded_values) => {
                    if decoded_values.is_empty() {
                        return Some("()".to_string());
                    }

                    // Format decoded return values with names if available
                    let mut return_parts = Vec::new();
                    for (i, value) in decoded_values.iter().enumerate() {
                        let param_name = function_abi
                            .outputs
                            .get(i)
                            .map(|param| param.name.as_str())
                            .filter(|name| !name.is_empty());

                        if let Some(name) = param_name {
                            return_parts.push(format!(
                                "{} {}: {}",
                                value.format_type(),
                                name,
                                self.resolve_sol_value(value, None),
                            ));
                        } else {
                            return_parts.push(
                                self.resolve_sol_value(value, Some(FormatCtx::new().with_ty(true))),
                            );
                        }
                    }

                    if return_parts.len() == 1 {
                        return Some(return_parts[0].clone());
                    } else {
                        return Some(format!("({})", return_parts.join(", ")));
                    }
                }
                Err(_) => {
                    return None;
                }
            }
        }
        None
    }

    /// Resolve function call
    pub fn resolve_function_call(
        &mut self,
        calldata: &Bytes,
        address: Option<Address>,
    ) -> Option<String> {
        if calldata.len() < 4 {
            return None;
        }

        let selector = Selector::from_slice(&calldata[..4]);
        if let Some(function_abi) = address
            .and_then(|addr| self.get_contract_abi(addr, false))
            .and_then(|abi| abi.function_by_selector(selector).cloned())
        {
            match function_abi.abi_decode_input(&calldata[4..]) {
                Ok(decoded) => {
                    let params: Vec<String> = decoded
                        .iter()
                        .map(|param| {
                            self.resolve_sol_value(param, Some(FormatCtx::new().with_ty(true)))
                        })
                        .collect();

                    return Some(format!("{}({})", function_abi.name, params.join(", ")));
                }
                Err(_) => {
                    return Some(format!(
                        "{}(0x{}...)",
                        function_abi.name,
                        hex::encode(&calldata[4..calldata.len().min(8)])
                    ));
                }
            }
        }

        None
    }

    /// Resolve constructor call
    pub fn resolve_constructor_call(&mut self, address: Address) -> Option<String> {
        let args = self.get_constructor_args(address).cloned()?;

        if let Some(abi) =
            self.get_contract_abi(address, false).and_then(|abi| abi.constructor().cloned())
        {
            match abi.abi_decode_input(&args) {
                Ok(decoded) => {
                    let params: Vec<String> = decoded
                        .iter()
                        .map(|param| {
                            self.resolve_sol_value(param, Some(FormatCtx::new().with_ty(true)))
                        })
                        .collect();

                    return Some(format!("constructor({})", params.join(", ")));
                }
                Err(_) => {
                    return None;
                }
            }
        }

        None
    }

    /// Resolve event
    pub fn resolve_event(&mut self, event: &LogData, address: Option<Address>) -> Option<String> {
        if event.topics().is_empty() {
            return None;
        }

        let event_signature = event.topics()[0];
        if let Some(event_abi) = address
            .and_then(|addr| self.get_contract_abi(addr, false))
            .and_then(|abi| abi.events().find(|e| e.selector() == event_signature).cloned())
        {
            // Try to decode the event
            match event_abi.decode_log(event) {
                Ok(decoded) => {
                    // Format decoded event with parameters
                    let mut params = Vec::new();
                    for (param, value) in event_abi.inputs.iter().zip(decoded.body.iter()) {
                        params.push(format!(
                            "{} {}: {}",
                            value.format_type(),
                            param.name,
                            self.resolve_sol_value(value, None),
                        ));
                    }

                    if params.is_empty() {
                        return Some(format!("{}()", event_abi.name));
                    } else {
                        return Some(format!("{}({})", event_abi.name, params.join(", ")));
                    }
                }
                Err(_) => {
                    // Fallback to event name with raw data
                    return Some(format!(
                        "{}(0x{}...) [decode failed]",
                        event_abi.name,
                        hex::encode(&event.data[..event.data.len().min(8)])
                    ));
                }
            }
        }

        None
    }

    /// Resolve solidity value
    pub fn resolve_sol_value(&mut self, value: &DynSolValue, ctx: Option<FormatCtx>) -> String {
        let mut ctx = ctx.unwrap_or_default();

        // Use unsafe to extend lifetime of self reference in closure
        // This should be safe since ctx is only used during formatting
        let self_ptr = self as *mut Self;
        ctx.resolve_address =
            Some(Box::new(move |addr| unsafe { (*self_ptr).resolve_address_label(addr) }));

        value.format_value(&ctx)
    }

    /// Resolve and format address with label if available
    pub fn resolve_address_label(&mut self, _address: Address) -> Option<String> {
        None
    }

    /// Resolve and format address
    pub fn resolve_address(&mut self, address: Address) -> String {
        if let Some(label) = self.resolve_address_label(address) {
            label
        } else {
            address.to_checksum(None)
        }
    }

    /// Resolve and format ether
    pub fn resolve_ether(&self, value: U256) -> String {
        // Convert Wei to ETH (1 ETH = 10^18 Wei)
        let eth_value = value.to_string();
        if eth_value.len() <= 18 {
            // Less than 1 ETH - show significant digits only
            let padded = format!("{eth_value:0>18}");
            let trimmed = padded.trim_end_matches('0');
            if trimmed.is_empty() {
                "0".to_string()
            } else {
                format!("0.{}", &trimmed[..trimmed.len().min(6)])
            }
        } else {
            // More than 1 ETH
            let (whole, decimal) = eth_value.split_at(eth_value.len() - 18);
            let decimal_trimmed = decimal[..4.min(decimal.len())].trim_end_matches('0');
            if decimal_trimmed.is_empty() {
                whole.to_string()
            } else {
                format!("{whole}.{decimal_trimmed}")
            }
        }
    }
}
