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

//! Debug handler implementations for testing and simulation.
//!
//! This module provides handler implementations designed for testing, debugging,
//! and simulation scenarios. It includes two main types of handlers:
//!
//! # Handler Types
//!
//! ## [`DebugHandler`]
//! A simple error-only handler that returns detailed error messages for all operations.
//! Useful for testing error conditions and understanding the evaluation flow.
//!
//! ## [`SimulationDebugHandler`]
//! An advanced simulation handler that:
//! - Returns mock values for variables and function calls
//! - Logs all operations for debugging
//! - Allows pre-configuration of specific return values
//! - Generates plausible default values based on naming conventions
//!
//! # Usage
//!
//! ## Error-Only Debug Handler
//! ```rust,ignore
//! let handlers = create_debug_handlers();
//! let evaluator = ExpressionEvaluator::new(handlers);
//! // All operations will return detailed error messages
//! ```
//!
//! ## Simulation Handler
//! ```rust,ignore
//! let (handlers, debug_handler) = create_simulation_debug_handlers();
//! debug_handler.set_variable("balance", DynSolValue::Uint(U256::from(1000), 256));
//! debug_handler.set_function("totalSupply", DynSolValue::Uint(U256::from(1000000), 256));
//!
//! let evaluator = ExpressionEvaluator::new(handlers);
//! let result = evaluator.eval("balance + totalSupply()", 0)?; // Returns mock values
//!
//! // Check execution log
//! let log = debug_handler.get_log();
//! for entry in log {
//!     println!("Operation: {}", entry);
//! }
//! ```

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use alloy_dyn_abi::DynSolValue;
use alloy_primitives::{Address, U256};
use eyre::{bail, Result};

use super::*;

/// Debug handler that returns errors but includes input values for debugging
#[derive(Debug, Clone, Default)]
pub struct DebugHandler;

impl DebugHandler {
    /// Create a new debug handler.
    ///
    /// Returns a handler that will return detailed error messages for all operations,
    /// making it useful for testing error conditions and debugging evaluation flow.
    pub fn new() -> Self {
        Self
    }
}

/// Enhanced debug handler that can simulate values and log evaluation flow
#[derive(Debug, Clone)]
pub struct SimulationDebugHandler {
    /// Map of variable names to simulated values
    variables: Arc<Mutex<HashMap<String, DynSolValue>>>,
    /// Map of function names to simulated return values
    functions: Arc<Mutex<HashMap<String, DynSolValue>>>,
    /// Execution log for debugging
    log: Arc<Mutex<Vec<String>>>,
    /// Whether to log all operations
    verbose: bool,
}

impl Default for SimulationDebugHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationDebugHandler {
    /// Create a new simulation debug handler
    pub fn new() -> Self {
        Self {
            variables: Arc::new(Mutex::new(HashMap::new())),
            functions: Arc::new(Mutex::new(HashMap::new())),
            log: Arc::new(Mutex::new(Vec::new())),
            verbose: true,
        }
    }

    /// Add a simulated variable value
    pub fn set_variable(&self, name: &str, value: DynSolValue) {
        if let Ok(mut vars) = self.variables.lock() {
            vars.insert(name.to_string(), value);
        }
    }

    /// Add a simulated function return value
    pub fn set_function(&self, name: &str, return_value: DynSolValue) {
        if let Ok(mut funcs) = self.functions.lock() {
            funcs.insert(name.to_string(), return_value);
        }
    }

    /// Get the execution log
    pub fn get_log(&self) -> Vec<String> {
        if let Ok(log) = self.log.lock() {
            log.clone()
        } else {
            vec![]
        }
    }

    /// Clear the execution log
    pub fn clear_log(&self) {
        if let Ok(mut log) = self.log.lock() {
            log.clear();
        }
    }

    /// Log an operation
    fn log_operation(&self, message: String) {
        if self.verbose
            && let Ok(mut log) = self.log.lock() {
                log.push(message);
            }
    }

    /// Generate a plausible default value for a type hint
    fn generate_default_value(&self, hint: &str) -> DynSolValue {
        match hint {
            name if name.contains("balance")
                || name.contains("amount")
                || name.contains("value") =>
            {
                DynSolValue::Uint(U256::from(1000000), 256) // 1M as default balance
            }
            name if name.contains("address")
                || name.contains("owner")
                || name.contains("sender") =>
            {
                DynSolValue::Address(Address::from([0x42; 20])) // Mock address
            }
            name if name.contains("count") || name.contains("length") || name.contains("index") => {
                DynSolValue::Uint(U256::from(5), 256) // Default count/length
            }
            name if name.contains("enabled")
                || name.contains("active")
                || name.contains("flag") =>
            {
                DynSolValue::Bool(true) // Default boolean
            }
            name if name.contains("name") || name.contains("symbol") || name.contains("uri") => {
                DynSolValue::String(format!("Mock_{name}")) // Mock string
            }
            _ => DynSolValue::Uint(U256::from(42), 256), // Default fallback
        }
    }
}

impl VariableHandler for DebugHandler {
    fn get_variable_value(&self, name: &str, snapshot_id: usize) -> Result<DynSolValue> {
        bail!(
            "DebugHandler::get_variable_value called with name='{}', snapshot_id={}",
            name,
            snapshot_id
        )
    }
}

impl MappingArrayHandler for DebugHandler {
    fn get_mapping_or_array_value(
        &self,
        root: DynSolValue,
        indices: Vec<DynSolValue>,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        let indices_str = indices.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(", ");
        bail!(
            "DebugHandler::get_mapping_or_array_value called with root={:?}, indices=[{}], snapshot_id={}",
            root,
            indices_str,
            snapshot_id
        )
    }
}

impl FunctionCallHandler for DebugHandler {
    fn call_function(
        &self,
        name: &str,
        args: &[DynSolValue],
        callee: Option<&DynSolValue>,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        let args_str = args.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(", ");
        let callee_str = callee.map(|c| format!("{c:?}")).unwrap_or_else(|| "None".to_string());
        bail!(
            "DebugHandler::call_function called with name='{}', args=[{}], callee={}, snapshot_id={}",
            name,
            args_str,
            callee_str,
            snapshot_id
        )
    }
}

impl MemberAccessHandler for DebugHandler {
    fn access_member(
        &self,
        value: DynSolValue,
        member: &str,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        bail!(
            "DebugHandler::access_member called with value={:?}, member='{}', snapshot_id={}",
            value,
            member,
            snapshot_id
        )
    }
}

impl MsgHandler for DebugHandler {
    fn get_msg_sender(&self, snapshot_id: usize) -> Result<DynSolValue> {
        bail!("DebugHandler::get_msg_sender called with snapshot_id={}", snapshot_id)
    }

    fn get_msg_value(&self, snapshot_id: usize) -> Result<DynSolValue> {
        bail!("DebugHandler::get_msg_value called with snapshot_id={}", snapshot_id)
    }
}

impl TxHandler for DebugHandler {
    fn get_tx_origin(&self, snapshot_id: usize) -> Result<DynSolValue> {
        bail!("DebugHandler::get_tx_origin called with snapshot_id={}", snapshot_id)
    }
}

impl BlockHandler for DebugHandler {
    fn get_block_number(&self, snapshot_id: usize) -> Result<DynSolValue> {
        bail!("DebugHandler::get_block_number called with snapshot_id={}", snapshot_id)
    }

    fn get_block_timestamp(&self, snapshot_id: usize) -> Result<DynSolValue> {
        bail!("DebugHandler::get_block_timestamp called with snapshot_id={}", snapshot_id)
    }
}

impl ValidationHandler for DebugHandler {
    fn validate_value(&self, value: DynSolValue) -> Result<DynSolValue> {
        Ok(value)
    }
}

// Implement handler traits for SimulationDebugHandler
impl VariableHandler for SimulationDebugHandler {
    fn get_variable_value(&self, name: &str, snapshot_id: usize) -> Result<DynSolValue> {
        self.log_operation(format!("get_variable_value: name='{name}', snapshot_id={snapshot_id}"));

        if let Ok(vars) = self.variables.lock()
            && let Some(value) = vars.get(name) {
                self.log_operation(format!("  -> returning stored value: {value:?}"));
                return Ok(value.clone());
            }

        // Generate a plausible default based on variable name
        let default_value = self.generate_default_value(name);
        self.log_operation(format!("  -> generating default value: {default_value:?}"));
        Ok(default_value)
    }
}

impl MappingArrayHandler for SimulationDebugHandler {
    fn get_mapping_or_array_value(
        &self,
        root: DynSolValue,
        indices: Vec<DynSolValue>,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        let indices_str = indices.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(", ");
        self.log_operation(format!(
            "get_mapping_or_array_value: root={root:?}, indices=[{indices_str}], snapshot_id={snapshot_id}"
        ));

        // For arrays, return a mock element
        // For mappings, return a value based on the key
        let result = match indices.first() {
            Some(DynSolValue::Uint(index, _)) => {
                // Array access - return mock data based on index
                DynSolValue::Uint(U256::from(1000 + index.to::<u64>()), 256)
            }
            Some(DynSolValue::Address(_)) => {
                // Address mapping - return mock balance
                DynSolValue::Uint(U256::from(1000000), 256)
            }
            Some(DynSolValue::String(key)) => {
                // String mapping - return based on key
                self.generate_default_value(key)
            }
            _ => DynSolValue::Uint(U256::from(42), 256),
        };

        self.log_operation(format!("  -> returning: {result:?}"));
        Ok(result)
    }
}

impl FunctionCallHandler for SimulationDebugHandler {
    fn call_function(
        &self,
        name: &str,
        args: &[DynSolValue],
        callee: Option<&DynSolValue>,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        let args_str = args.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(", ");
        let callee_str = callee.map(|c| format!("{c:?}")).unwrap_or_else(|| "None".to_string());
        self.log_operation(format!(
            "call_function: name='{name}', args=[{args_str}], callee={callee_str}, snapshot_id={snapshot_id}"
        ));

        if let Ok(funcs) = self.functions.lock()
            && let Some(value) = funcs.get(name) {
                self.log_operation(format!("  -> returning stored value: {value:?}"));
                return Ok(value.clone());
            }

        // Generate result based on function name
        let result = match name {
            "balanceOf" => DynSolValue::Uint(U256::from(1000000), 256),
            "totalSupply" => DynSolValue::Uint(U256::from(1000000000), 256),
            "approve" | "transfer" => DynSolValue::Bool(true),
            "name" => DynSolValue::String("MockToken".to_string()),
            "symbol" => DynSolValue::String("MTK".to_string()),
            "decimals" => DynSolValue::Uint(U256::from(18), 256),
            _ => self.generate_default_value(name),
        };

        self.log_operation(format!("  -> returning generated value: {result:?}"));
        Ok(result)
    }
}

impl MemberAccessHandler for SimulationDebugHandler {
    fn access_member(
        &self,
        value: DynSolValue,
        member: &str,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        self.log_operation(format!(
            "access_member: value={value:?}, member='{member}', snapshot_id={snapshot_id}"
        ));

        // Handle common member accesses
        let result = match member {
            "length" => DynSolValue::Uint(U256::from(10), 256), // Mock array length
            "balance" => DynSolValue::Uint(U256::from(1000000), 256), // Mock balance
            "code" => DynSolValue::Bytes(vec![0x60, 0x80, 0x60, 0x40]), // Mock bytecode
            _ => self.generate_default_value(member),
        };

        self.log_operation(format!("  -> returning: {result:?}"));
        Ok(result)
    }
}

impl MsgHandler for SimulationDebugHandler {
    fn get_msg_sender(&self, snapshot_id: usize) -> Result<DynSolValue> {
        self.log_operation(format!("get_msg_sender: snapshot_id={snapshot_id}"));
        let result = DynSolValue::Address(Address::from([0x42; 20])); // Mock sender
        self.log_operation(format!("  -> returning: {result:?}"));
        Ok(result)
    }

    fn get_msg_value(&self, snapshot_id: usize) -> Result<DynSolValue> {
        self.log_operation(format!("get_msg_value: snapshot_id={snapshot_id}"));
        let result = DynSolValue::Uint(U256::from(1000000000000000000u64), 256); // 1 ETH
        self.log_operation(format!("  -> returning: {result:?}"));
        Ok(result)
    }
}

impl TxHandler for SimulationDebugHandler {
    fn get_tx_origin(&self, snapshot_id: usize) -> Result<DynSolValue> {
        self.log_operation(format!("get_tx_origin: snapshot_id={snapshot_id}"));
        let result = DynSolValue::Address(Address::from([0x11; 20])); // Mock origin
        self.log_operation(format!("  -> returning: {result:?}"));
        Ok(result)
    }
}

impl BlockHandler for SimulationDebugHandler {
    fn get_block_number(&self, snapshot_id: usize) -> Result<DynSolValue> {
        self.log_operation(format!("get_block_number: snapshot_id={snapshot_id}"));
        let result = DynSolValue::Uint(U256::from(18500000), 256); // Mock block number
        self.log_operation(format!("  -> returning: {result:?}"));
        Ok(result)
    }

    fn get_block_timestamp(&self, snapshot_id: usize) -> Result<DynSolValue> {
        self.log_operation(format!("get_block_timestamp: snapshot_id={snapshot_id}"));
        let result = DynSolValue::Uint(U256::from(1700000000), 256); // Mock timestamp
        self.log_operation(format!("  -> returning: {result:?}"));
        Ok(result)
    }
}

// Implement traits for Arc<SimulationDebugHandler> to allow sharing
impl VariableHandler for Arc<SimulationDebugHandler> {
    fn get_variable_value(&self, name: &str, snapshot_id: usize) -> Result<DynSolValue> {
        self.as_ref().get_variable_value(name, snapshot_id)
    }
}

impl MappingArrayHandler for Arc<SimulationDebugHandler> {
    fn get_mapping_or_array_value(
        &self,
        root: DynSolValue,
        indices: Vec<DynSolValue>,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        self.as_ref().get_mapping_or_array_value(root, indices, snapshot_id)
    }
}

impl FunctionCallHandler for Arc<SimulationDebugHandler> {
    fn call_function(
        &self,
        name: &str,
        args: &[DynSolValue],
        callee: Option<&DynSolValue>,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        self.as_ref().call_function(name, args, callee, snapshot_id)
    }
}

impl MemberAccessHandler for Arc<SimulationDebugHandler> {
    fn access_member(
        &self,
        value: DynSolValue,
        member: &str,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        self.as_ref().access_member(value, member, snapshot_id)
    }
}

impl MsgHandler for Arc<SimulationDebugHandler> {
    fn get_msg_sender(&self, snapshot_id: usize) -> Result<DynSolValue> {
        self.as_ref().get_msg_sender(snapshot_id)
    }

    fn get_msg_value(&self, snapshot_id: usize) -> Result<DynSolValue> {
        self.as_ref().get_msg_value(snapshot_id)
    }
}

impl TxHandler for Arc<SimulationDebugHandler> {
    fn get_tx_origin(&self, snapshot_id: usize) -> Result<DynSolValue> {
        self.as_ref().get_tx_origin(snapshot_id)
    }
}

impl BlockHandler for Arc<SimulationDebugHandler> {
    fn get_block_number(&self, snapshot_id: usize) -> Result<DynSolValue> {
        self.as_ref().get_block_number(snapshot_id)
    }

    fn get_block_timestamp(&self, snapshot_id: usize) -> Result<DynSolValue> {
        self.as_ref().get_block_timestamp(snapshot_id)
    }
}

impl ValidationHandler for Arc<SimulationDebugHandler> {
    fn validate_value(&self, value: DynSolValue) -> Result<DynSolValue> {
        Ok(value)
    }
}

/// Create debug handlers for all traits (original error-only version)
pub fn create_debug_handlers() -> EvaluatorHandlers {
    EvaluatorHandlers::new()
        .with_variable_handler(Box::new(DebugHandler::new()))
        .with_mapping_array_handler(Box::new(DebugHandler::new()))
        .with_function_call_handler(Box::new(DebugHandler::new()))
        .with_member_access_handler(Box::new(DebugHandler::new()))
        .with_msg_handler(Box::new(DebugHandler::new()))
        .with_tx_handler(Box::new(DebugHandler::new()))
        .with_block_handler(Box::new(DebugHandler::new()))
        .with_validation_handler(Box::new(DebugHandler::new()))
}

/// Create simulation debug handlers that return mock values and log operations
pub fn create_simulation_debug_handlers() -> (EvaluatorHandlers, Arc<SimulationDebugHandler>) {
    let handler = Arc::new(SimulationDebugHandler::new());
    let handlers = EvaluatorHandlers::new()
        .with_variable_handler(Box::new(handler.clone()))
        .with_mapping_array_handler(Box::new(handler.clone()))
        .with_function_call_handler(Box::new(handler.clone()))
        .with_member_access_handler(Box::new(handler.clone()))
        .with_msg_handler(Box::new(handler.clone()))
        .with_tx_handler(Box::new(handler.clone()))
        .with_block_handler(Box::new(handler.clone()))
        .with_validation_handler(Box::new(handler.clone()));

    (handlers, handler)
}
