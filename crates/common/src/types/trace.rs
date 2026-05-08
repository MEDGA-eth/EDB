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

use alloy_primitives::{Address, Bytes, LogData, U256, hex};
use auto_impl::auto_impl;
use revm::{
    context::CreateScheme,
    interpreter::{
        CallInputs, CallOutcome, CallScheme, CreateInputs, CreateOutcome, InstructionResult,
    },
};
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};
use tracing::error;

/// Type of call/creation operation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallType {
    /// Regular call to existing contract
    Call(CallScheme),
    /// Contract creation via CREATE opcode
    Create(CreateScheme),
}

/// Trait for converting inputs to call type representation for trace analysis
#[auto_impl(&, &mut, Box, Rc, Arc)]
trait IntoCallType {
    /// Convert this input type to its corresponding CallType variant
    fn convert_to_call_type(&self) -> CallType;
}

impl IntoCallType for CallInputs {
    fn convert_to_call_type(&self) -> CallType {
        CallType::Call(self.scheme)
    }
}

impl IntoCallType for CreateInputs {
    fn convert_to_call_type(&self) -> CallType {
        CallType::Create(self.scheme())
    }
}

impl<T> From<T> for CallType
where
    T: IntoCallType,
{
    fn from(value: T) -> Self {
        value.convert_to_call_type()
    }
}

/// Result of a call/creation operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CallResult {
    /// Call succeeded
    Success {
        /// Output data from the call
        output: Bytes,
        /// Result
        result: InstructionResult,
    },
    /// Call reverted
    Revert {
        /// Output data from the call
        output: Bytes,
        /// Result
        result: InstructionResult,
    },
    /// Self-destruct
    Error {
        /// Output data from the call
        output: Bytes,
        /// Result
        result: InstructionResult,
    },
}

impl PartialEq for CallResult {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Success { output: out1, result: res1 },
                Self::Success { output: out2, result: res2 },
            ) => {
                if out1.is_empty() && out2.is_empty() {
                    // When the return data is empty, we allow STOP equals to RETURN
                    (res1 == res2)
                        || (matches!(res1, InstructionResult::Stop)
                            && matches!(res2, InstructionResult::Return))
                        || (matches!(res2, InstructionResult::Stop)
                            && matches!(res1, InstructionResult::Return))
                } else {
                    out1 == out2 && res1 == res2
                }
            }
            (
                Self::Revert { output: out1, result: res1 },
                Self::Revert { output: out2, result: res2 },
            ) => out1 == out2 && res1 == res2,
            (
                Self::Error { output: out1, result: res1 },
                Self::Error { output: out2, result: res2 },
            ) => out1 == out2 && res1 == res2,
            _ => false,
        }
    }
}

impl Eq for CallResult {}

impl CallResult {
    /// Get the instruction result code from this call result
    pub fn result(&self) -> InstructionResult {
        match self {
            Self::Success { result, .. } => *result,
            Self::Revert { result, .. } => *result,
            Self::Error { result, .. } => *result,
        }
    }

    /// Get the output bytes from this call result (return data or revert reason)
    pub fn output(&self) -> &Bytes {
        match self {
            Self::Success { output, .. } => output,
            Self::Revert { output, .. } => output,
            Self::Error { output, .. } => output,
        }
    }
}

/// Trait for converting outcomes to call result representation for trace analysis
#[auto_impl(&, &mut, Box, Rc, Arc)]
trait IntoCallResult {
    /// Convert this outcome type to its corresponding CallResult variant
    fn convert_to_call_result(&self) -> CallResult;
}

impl IntoCallResult for CallOutcome {
    fn convert_to_call_result(&self) -> CallResult {
        if self.result.is_ok() {
            CallResult::Success { output: self.result.output.clone(), result: self.result.result }
        } else if self.result.is_revert() {
            CallResult::Revert { output: self.result.output.clone(), result: self.result.result }
        } else if self.result.is_error() {
            CallResult::Error { output: self.result.output.clone(), result: self.result.result }
        } else {
            error!("Unexpected call outcome, we use CallResult::Error");
            CallResult::Error { output: self.result.output.clone(), result: self.result.result }
        }
    }
}

impl IntoCallResult for CreateOutcome {
    fn convert_to_call_result(&self) -> CallResult {
        if self.result.is_ok() {
            CallResult::Success { output: self.result.output.clone(), result: self.result.result }
        } else if self.result.is_revert() {
            CallResult::Revert { output: self.result.output.clone(), result: self.result.result }
        } else if self.result.is_error() {
            CallResult::Error { output: self.result.output.clone(), result: self.result.result }
        } else {
            error!("Unexpected create outcome, we use CallResult::Error");
            CallResult::Error { output: self.result.output.clone(), result: self.result.result }
        }
    }
}

impl<T> From<T> for CallResult
where
    T: IntoCallResult,
{
    fn from(value: T) -> Self {
        value.convert_to_call_result()
    }
}

/// Complete execution trace containing all call/creation entries for transaction analysis and
/// debugging
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trace {
    /// Internal vector storing all trace entries in chronological order
    inner: Vec<TraceEntry>,
}

impl Deref for Trace {
    type Target = Vec<TraceEntry>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for Trace {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

// Convenient explicit iterator methods (optional but nice)
impl Trace {
    /// Convert trace to serde_json::Value for RPC serialization
    pub fn to_json_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(self)
    }

    /// Create a new empty trace
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a trace entry to this trace
    pub fn push(&mut self, entry: TraceEntry) {
        self.inner.push(entry);
    }

    /// Get the number of trace entries
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if the trace is empty
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

// IntoIterator for owned Trace (moves out its contents)
impl IntoIterator for Trace {
    type Item = TraceEntry;
    type IntoIter = std::vec::IntoIter<TraceEntry>;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

// IntoIterator for &Trace (shared iteration)
impl<'a> IntoIterator for &'a Trace {
    type Item = &'a TraceEntry;
    type IntoIter = std::slice::Iter<'a, TraceEntry>;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

// IntoIterator for &mut Trace (mutable iteration)
impl<'a> IntoIterator for &'a mut Trace {
    type Item = &'a mut TraceEntry;
    type IntoIter = std::slice::IterMut<'a, TraceEntry>;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter_mut()
    }
}

/// Single trace entry representing a call or creation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    /// Unique ID of this trace entry (its index in the trace vector)
    pub id: usize,
    /// ID of the parent trace entry (None for top-level calls)
    pub parent_id: Option<usize>,
    /// Depth in the call stack (0 = top level)
    pub depth: usize,
    /// Type of operation
    pub call_type: CallType,
    /// Address making the call
    pub caller: Address,
    /// Target address for calls, or computed address for creates
    pub target: Address,
    /// Address where the code actually lives (for delegate calls)
    pub code_address: Address,
    /// Input data / constructor args
    pub input: Bytes,
    /// Value transferred
    pub value: U256,
    /// Result of the call (populated on call_end)
    pub result: Option<CallResult>,
    /// Whether this created a new contract
    pub created_contract: bool,
    /// Create scheme for contract creation
    pub create_scheme: Option<CreateScheme>,
    /// The underlying running bytecode
    pub bytecode: Option<Bytes>,
    /// Label of the target contract
    pub target_label: Option<String>,
    /// Self-destruct information
    pub self_destruct: Option<(Address, U256)>,
    /// Events
    pub events: Vec<LogData>,
    /// The first snapshot id that belongs to this entry
    pub first_snapshot_id: Option<usize>,
}

// Pretty print for Trace
impl Trace {
    /// Print the trace tree structure showing parent-child relationships with fancy formatting
    pub fn print_trace_tree(&self) {
        println!();
        println!(
            "\x1b[36m╔══════════════════════════════════════════════════════════════════╗\x1b[0m"
        );
        println!(
            "\x1b[36m║                      EXECUTION TRACE TREE                        ║\x1b[0m"
        );
        println!(
            "\x1b[36m╚══════════════════════════════════════════════════════════════════╝\x1b[0m"
        );
        println!();

        // Find root entries (those without parents)
        let roots: Vec<&TraceEntry> =
            self.inner.iter().filter(|entry| entry.parent_id.is_none()).collect();

        if roots.is_empty() {
            println!("  \x1b[90mNo trace entries found\x1b[0m");
            return;
        }

        for (i, root) in roots.iter().enumerate() {
            let is_last = i == roots.len() - 1;
            self.print_trace_entry(root, 0, is_last, vec![]);
        }

        println!();
        println!(
            "\x1b[36m══════════════════════════════════════════════════════════════════\x1b[0m"
        );
        self.print_summary();
    }

    /// Helper function to recursively print trace entries with fancy indentation
    fn print_trace_entry(
        &self,
        entry: &TraceEntry,
        indent_level: usize,
        is_last: bool,
        mut prefix: Vec<bool>,
    ) {
        // Build the tree structure with proper connectors
        let mut tree_str = String::new();
        for (i, &is_empty) in prefix.iter().enumerate() {
            if i < prefix.len() {
                tree_str.push_str(if is_empty { "    " } else { "\x1b[90m│\x1b[0m   " });
            }
        }

        // Add the branch connector
        let connector = if indent_level > 0 {
            if is_last { "\x1b[90m└──\x1b[0m " } else { "\x1b[90m├──\x1b[0m " }
        } else {
            ""
        };
        tree_str.push_str(connector);

        // Format the call type with colors based on operation type
        let (_call_color, type_color, call_type_str) = match &entry.call_type {
            CallType::Call(CallScheme::Call) => ("\x1b[94m", "\x1b[34m", "CALL"),
            CallType::Call(CallScheme::CallCode) => ("\x1b[94m", "\x1b[34m", "CALLCODE"),
            CallType::Call(CallScheme::DelegateCall) => ("\x1b[96m", "\x1b[36m", "DELEGATECALL"),
            CallType::Call(CallScheme::StaticCall) => ("\x1b[95m", "\x1b[35m", "STATICCALL"),
            CallType::Create(CreateScheme::Create) => ("\x1b[93m", "\x1b[33m", "CREATE"),
            CallType::Create(CreateScheme::Create2 { .. }) => ("\x1b[93m", "\x1b[33m", "CREATE2"),
            CallType::Create(CreateScheme::Custom { .. }) => {
                ("\x1b[93m", "\x1b[33m", "CREATE_CUSTOM")
            }
        };

        // Format the result indicator
        let (result_indicator, result_color) = match &entry.result {
            Some(CallResult::Success { .. }) => ("✓", "\x1b[32m"),
            Some(CallResult::Revert { .. }) => ("✗", "\x1b[31m"),
            Some(CallResult::Error { .. }) => {
                ("☠", "\x1b[31m") // TODO (change icon)
            }
            None => ("", ""),
        };

        // Format value transfer with better visibility
        let value_str = if entry.value > U256::ZERO {
            format!(" \x1b[93m[{} ETH]\x1b[0m", format_ether(entry.value))
        } else {
            String::new()
        };

        // Format addresses with better distinction
        let caller_str = if entry.caller == Address::ZERO {
            "\x1b[90m0x0\x1b[0m".to_string()
        } else {
            format!("\x1b[37m{}\x1b[0m", format_address_short(entry.caller))
        };

        let target_str = if entry.target == Address::ZERO {
            "\x1b[90m0x0\x1b[0m".to_string()
        } else if entry.created_contract {
            format!("\x1b[92m{}\x1b[0m", format_address_short(entry.target))
        } else {
            format!("\x1b[37m{}\x1b[0m", format_address_short(entry.target))
        };

        // Different arrow based on call type
        let arrow = if matches!(entry.call_type, CallType::Create(_)) {
            "\x1b[93m→\x1b[0m"
        } else {
            "\x1b[90m→\x1b[0m"
        };

        // Print the formatted entry with cleaner layout
        print!("{tree_str}{type_color}{call_type_str:12}\x1b[0m {caller_str} {arrow} {target_str}");

        // Add result indicator at the end
        if !result_indicator.is_empty() {
            print!(" {result_color}{result_indicator} \x1b[0m");
        }

        // Add value if present
        if !value_str.is_empty() {
            print!("{value_str}");
        }

        // Add self-destruct indicator if present
        if let Some((beneficiary, value)) = &entry.self_destruct {
            print!(
                " \x1b[91m SELFDESTRUCT → {} ({} ETH)\x1b[0m",
                format_address_short(*beneficiary),
                format_ether(*value)
            );
        }

        println!();

        // Print input data with better formatting if significant
        if entry.input.len() > 4 {
            let data_preview = format_data_preview(&entry.input);
            let padding = "    ".repeat(indent_level + 1);
            println!("{padding}\x1b[90m└ Calldata: {data_preview}\x1b[0m");
        }

        // Print events if any
        if !entry.events.is_empty() {
            let padding = "    ".repeat(indent_level + 1);
            for (i, event) in entry.events.iter().enumerate() {
                let event_str = format_event(event);
                if i == 0 {
                    println!("{padding}\x1b[96m└ Events:\x1b[0m");
                }
                println!("{padding}    \x1b[96m• {event_str}\x1b[0m");
            }
        }

        // Print error details if result is Error
        if let Some(CallResult::Error { output, .. }) = &entry.result {
            let padding = "    ".repeat(indent_level + 1);
            let error_msg = if output.is_empty() {
                "Execution error (no output)".to_string()
            } else if output.len() >= 4 {
                // Try to decode as a revert message
                format!("Error: {}", format_data_preview(output))
            } else {
                format!("Error output: 0x{}", hex::encode(output))
            };
            println!("{padding}\x1b[91m└ ⚠️  {error_msg}\x1b[0m");
        }

        // Get children and recursively print them
        let children = self.get_children(entry.id);

        // Update prefix for children
        if indent_level > 0 {
            prefix.push(is_last);
        }

        for (i, child) in children.iter().enumerate() {
            let child_is_last = i == children.len() - 1;
            self.print_trace_entry(child, indent_level + 1, child_is_last, prefix.clone());
        }
    }

    /// Print summary statistics about the trace
    fn print_summary(&self) {
        let total = self.inner.len();
        let successful = self
            .inner
            .iter()
            .filter(|e| matches!(e.result, Some(CallResult::Success { .. })))
            .count();
        let reverted = self
            .inner
            .iter()
            .filter(|e| matches!(e.result, Some(CallResult::Revert { .. })))
            .count();
        let errors = self
            .inner
            .iter()
            .filter(|e| matches!(e.result, Some(CallResult::Error { .. })))
            .count();
        let self_destructs = self.inner.iter().filter(|e| e.self_destruct.is_some()).count();
        let with_events = self.inner.iter().filter(|e| !e.events.is_empty()).count();
        let total_events: usize = self.inner.iter().map(|e| e.events.len()).sum();
        let creates =
            self.inner.iter().filter(|e| matches!(e.call_type, CallType::Create(_))).count();
        let calls = self.inner.iter().filter(|e| matches!(e.call_type, CallType::Call(_))).count();
        let max_depth = self.inner.iter().map(|e| e.depth).max().unwrap_or(0);

        println!("\x1b[36mSummary:\x1b[0m");
        println!(
            "  Total: {total} | \x1b[32mSuccess: {successful}\x1b[0m | \x1b[31mReverts: {reverted}\x1b[0m | \x1b[91mErrors: {errors}\x1b[0m | \x1b[94mCalls: {calls}\x1b[0m | \x1b[93mCreates: {creates}\x1b[0m | Depth: {max_depth}"
        );

        if self_destructs > 0 {
            println!("  \x1b[91m💀 Self-destructs: {self_destructs}\x1b[0m");
        }

        if total_events > 0 {
            println!("  \x1b[96m📝 Events: {total_events} (in {with_events} calls)\x1b[0m");
        }
    }

    /// Get the parent trace entry for a given trace entry ID
    pub fn get_parent(&self, trace_id: usize) -> Option<&TraceEntry> {
        self.inner
            .get(trace_id)
            .and_then(|entry| entry.parent_id.and_then(|parent_id| self.inner.get(parent_id)))
    }

    /// Get all children trace entries for a given trace entry ID
    pub fn get_children(&self, trace_id: usize) -> Vec<&TraceEntry> {
        self.inner.iter().filter(|entry| entry.parent_id == Some(trace_id)).collect()
    }
}

// Helper functions for formatting

/// Format an address to a shortened display format
fn format_address_short(addr: Address) -> String {
    if addr == Address::ZERO { "0x0".to_string() } else { addr.to_checksum(None) }
}

/// Format data/input bytes to a preview format
fn format_data_preview(data: &Bytes) -> String {
    if data.is_empty() {
        "0x".to_string()
    } else if data.len() <= 4 {
        format!("0x{}", hex::encode(data))
    } else {
        // Show function selector and total length
        format!("0x{}… [{} bytes]", hex::encode(&data[..4]), data.len())
    }
}

/// Format Wei value to ETH
fn format_ether(value: U256) -> String {
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

/// Format event/log data for display
fn format_event(event: &LogData) -> String {
    if event.topics().is_empty() {
        // Anonymous event or no topics
        format!("Anonymous event with {} bytes data", event.data.len())
    } else {
        // First topic is usually the event signature hash
        let sig_hash = &event.topics()[0];
        let additional_topics = event.topics().len() - 1;
        let data_len = event.data.len();

        // Format the signature hash (first 8 chars)
        let sig_preview = format!("0x{}...", hex::encode(&sig_hash.as_slice()[..4]));

        if additional_topics > 0 && data_len > 0 {
            format!("{sig_preview} ({additional_topics} indexed, {data_len} bytes data)")
        } else if additional_topics > 0 {
            format!("{sig_preview} ({additional_topics} indexed params)")
        } else if data_len > 0 {
            format!("{sig_preview} ({data_len} bytes data)")
        } else {
            sig_preview
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, B256, Bytes, LogData, U256, address};
    use revm::{
        context::CreateScheme,
        interpreter::{CallScheme, InstructionResult},
    };
    use std::collections::HashSet;

    #[test]
    fn test_call_type_serialization() {
        let call_types = vec![
            CallType::Call(CallScheme::Call),
            CallType::Call(CallScheme::CallCode),
            CallType::Call(CallScheme::DelegateCall),
            CallType::Call(CallScheme::StaticCall),
            CallType::Create(CreateScheme::Create),
            CallType::Create(CreateScheme::Create2 { salt: U256::from(123) }),
        ];

        for call_type in call_types {
            let json = serde_json::to_string(&call_type).expect("Failed to serialize CallType");
            let deserialized: CallType =
                serde_json::from_str(&json).expect("Failed to deserialize CallType");
            assert_eq!(deserialized, call_type);
        }
    }

    #[test]
    fn test_call_result_serialization() {
        let results = vec![
            CallResult::Success {
                output: Bytes::from_static(b"success"),
                result: InstructionResult::Return,
            },
            CallResult::Revert {
                output: Bytes::from_static(b"revert reason"),
                result: InstructionResult::Revert,
            },
            CallResult::Error {
                output: Bytes::from_static(b"error"),
                result: InstructionResult::OutOfGas,
            },
        ];

        for result in results {
            let json = serde_json::to_string(&result).expect("Failed to serialize CallResult");
            let deserialized: CallResult =
                serde_json::from_str(&json).expect("Failed to deserialize CallResult");
            assert_eq!(deserialized, result);
        }
    }

    #[test]
    fn test_trace_entry_serialization() {
        let entry = TraceEntry {
            id: 1,
            parent_id: Some(0),
            depth: 1,
            call_type: CallType::Call(CallScheme::Call),
            caller: address!("0xabcdefabcdefabcdefabcdefabcdefabcdefabcd"),
            target: address!("0x1234567890123456789012345678901234567890"),
            code_address: address!("0x1234567890123456789012345678901234567890"),
            input: Bytes::from(hex::decode("deadbeef").unwrap()),
            value: U256::from(1000),
            result: Some(CallResult::Success {
                output: Bytes::from_static(b"return_data"),
                result: InstructionResult::Return,
            }),
            created_contract: false,
            create_scheme: None,
            bytecode: Some(Bytes::from(hex::decode("60806040").unwrap())),
            target_label: Some("TestContract".to_string()),
            self_destruct: None,
            events: vec![],
            first_snapshot_id: Some(42),
        };

        let json = serde_json::to_string(&entry).expect("Failed to serialize TraceEntry");
        let deserialized: TraceEntry =
            serde_json::from_str(&json).expect("Failed to deserialize TraceEntry");

        assert_eq!(deserialized.id, entry.id);
        assert_eq!(deserialized.parent_id, entry.parent_id);
        assert_eq!(deserialized.depth, entry.depth);
        assert_eq!(deserialized.call_type, entry.call_type);
        assert_eq!(deserialized.caller, entry.caller);
        assert_eq!(deserialized.target, entry.target);
        assert_eq!(deserialized.code_address, entry.code_address);
        assert_eq!(deserialized.input, entry.input);
        assert_eq!(deserialized.value, entry.value);
        assert_eq!(deserialized.result, entry.result);
        assert_eq!(deserialized.created_contract, entry.created_contract);
        assert_eq!(deserialized.create_scheme, entry.create_scheme);
        assert_eq!(deserialized.bytecode, entry.bytecode);
        assert_eq!(deserialized.target_label, entry.target_label);
        assert_eq!(deserialized.self_destruct, entry.self_destruct);
        assert_eq!(deserialized.events, entry.events);
        assert_eq!(deserialized.first_snapshot_id, entry.first_snapshot_id);
    }

    #[test]
    fn test_trace_serialization() {
        let mut trace = Trace::new();

        let input = Bytes::from(hex::decode("deadbeef").unwrap());
        let bytecode = Bytes::from(hex::decode("60806040").unwrap());

        trace.push(TraceEntry {
            id: 0,
            parent_id: None,
            depth: 0,
            call_type: CallType::Call(CallScheme::Call),
            caller: address!("0xabcdefabcdefabcdefabcdefabcdefabcdefabcd"),
            target: address!("0x1234567890123456789012345678901234567890"),
            code_address: address!("0x1234567890123456789012345678901234567890"),
            input,
            value: U256::from(1000),
            result: Some(CallResult::Success {
                output: Bytes::from_static(b"return_data"),
                result: InstructionResult::Return,
            }),
            created_contract: false,
            create_scheme: None,
            bytecode: Some(bytecode),
            target_label: None,
            self_destruct: None,
            events: vec![],
            first_snapshot_id: None,
        });

        let json = serde_json::to_string(&trace).expect("Failed to serialize Trace");
        let deserialized: Trace = serde_json::from_str(&json).expect("Failed to deserialize Trace");

        assert_eq!(deserialized.len(), 1);
        assert_eq!(deserialized[0].id, 0);
        assert_eq!(deserialized[0].call_type, CallType::Call(CallScheme::Call));
    }

    #[test]
    fn test_call_result_equality_special_cases() {
        // Test special case where empty output allows STOP == RETURN
        let stop_result =
            CallResult::Success { output: Bytes::new(), result: InstructionResult::Stop };
        let return_result =
            CallResult::Success { output: Bytes::new(), result: InstructionResult::Return };

        assert_eq!(stop_result, return_result);

        // Test that non-empty outputs must match exactly
        let stop_with_output = CallResult::Success {
            output: Bytes::from_static(b"data"),
            result: InstructionResult::Stop,
        };
        let return_with_output = CallResult::Success {
            output: Bytes::from_static(b"data"),
            result: InstructionResult::Return,
        };

        assert_ne!(stop_with_output, return_with_output);
    }

    #[test]
    fn test_call_result_methods() {
        let success = CallResult::Success {
            output: Bytes::from_static(b"success_output"),
            result: InstructionResult::Return,
        };

        assert_eq!(success.result(), InstructionResult::Return);
        assert_eq!(success.output(), &Bytes::from_static(b"success_output"));

        let revert = CallResult::Revert {
            output: Bytes::from_static(b"revert_reason"),
            result: InstructionResult::Revert,
        };

        assert_eq!(revert.result(), InstructionResult::Revert);
        assert_eq!(revert.output(), &Bytes::from_static(b"revert_reason"));

        let error = CallResult::Error {
            output: Bytes::from_static(b"error_data"),
            result: InstructionResult::OutOfGas,
        };

        assert_eq!(error.result(), InstructionResult::OutOfGas);
        assert_eq!(error.output(), &Bytes::from_static(b"error_data"));
    }

    #[test]
    fn test_trace_methods() {
        let mut trace = Trace::new();
        assert!(trace.is_empty());
        assert_eq!(trace.len(), 0);

        let entry = TraceEntry {
            id: 0,
            parent_id: None,
            depth: 0,
            call_type: CallType::Call(CallScheme::Call),
            caller: Address::ZERO,
            target: Address::ZERO,
            code_address: Address::ZERO,
            input: Bytes::new(),
            value: U256::ZERO,
            result: None,
            created_contract: false,
            create_scheme: None,
            bytecode: None,
            target_label: None,
            self_destruct: None,
            events: vec![],
            first_snapshot_id: None,
        };

        trace.push(entry);
        assert!(!trace.is_empty());
        assert_eq!(trace.len(), 1);
    }

    #[test]
    fn test_trace_to_json_value() {
        let trace = Trace::new();
        let json_value = trace.to_json_value().expect("Failed to convert to JSON");
        assert!(json_value.is_object());
    }

    #[test]
    fn test_trace_iteration() {
        let mut trace = Trace::new();
        let entry1 = TraceEntry {
            id: 0,
            parent_id: None,
            depth: 0,
            call_type: CallType::Call(CallScheme::Call),
            caller: Address::ZERO,
            target: Address::ZERO,
            code_address: Address::ZERO,
            input: Bytes::new(),
            value: U256::ZERO,
            result: None,
            created_contract: false,
            create_scheme: None,
            bytecode: None,
            target_label: None,
            self_destruct: None,
            events: vec![],
            first_snapshot_id: None,
        };
        let entry2 = TraceEntry {
            id: 1,
            parent_id: Some(0),
            depth: 1,
            call_type: CallType::Create(CreateScheme::Create),
            caller: Address::ZERO,
            target: Address::ZERO,
            code_address: Address::ZERO,
            input: Bytes::new(),
            value: U256::ZERO,
            result: None,
            created_contract: true,
            create_scheme: Some(CreateScheme::Create),
            bytecode: None,
            target_label: None,
            self_destruct: None,
            events: vec![],
            first_snapshot_id: None,
        };

        trace.push(entry1);
        trace.push(entry2);

        // Test shared reference iteration
        let collected: Vec<_> = (&trace).into_iter().collect();
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0].id, 0);
        assert_eq!(collected[1].id, 1);

        // Test mutable reference iteration
        for entry in &mut trace {
            entry.depth += 10;
        }
        assert_eq!(trace[0].depth, 10);
        assert_eq!(trace[1].depth, 11);

        // Test owned iteration (consumes trace)
        let collected: Vec<_> = trace.into_iter().collect();
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0].depth, 10);
        assert_eq!(collected[1].depth, 11);
    }

    #[test]
    fn test_trace_parent_child_relationships() {
        let mut trace = Trace::new();

        // Add parent entry
        trace.push(TraceEntry {
            id: 0,
            parent_id: None,
            depth: 0,
            call_type: CallType::Call(CallScheme::Call),
            caller: Address::ZERO,
            target: Address::ZERO,
            code_address: Address::ZERO,
            input: Bytes::new(),
            value: U256::ZERO,
            result: None,
            created_contract: false,
            create_scheme: None,
            bytecode: None,
            target_label: None,
            self_destruct: None,
            events: vec![],
            first_snapshot_id: None,
        });

        // Add child entries
        for i in 1..=3 {
            trace.push(TraceEntry {
                id: i,
                parent_id: Some(0),
                depth: 1,
                call_type: CallType::Call(CallScheme::Call),
                caller: Address::ZERO,
                target: Address::ZERO,
                code_address: Address::ZERO,
                input: Bytes::new(),
                value: U256::ZERO,
                result: None,
                created_contract: false,
                create_scheme: None,
                bytecode: None,
                target_label: None,
                self_destruct: None,
                events: vec![],
                first_snapshot_id: None,
            });
        }

        // Test get_parent
        assert!(trace.get_parent(0).is_none()); // Root has no parent
        assert_eq!(trace.get_parent(1).unwrap().id, 0);
        assert_eq!(trace.get_parent(2).unwrap().id, 0);
        assert_eq!(trace.get_parent(3).unwrap().id, 0);

        // Test get_children
        let children = trace.get_children(0);
        assert_eq!(children.len(), 3);
        let child_ids: HashSet<_> = children.iter().map(|c| c.id).collect();
        assert_eq!(child_ids, HashSet::from([1, 2, 3]));

        // Test getting children of leaf nodes
        assert!(trace.get_children(1).is_empty());
        assert!(trace.get_children(2).is_empty());
        assert!(trace.get_children(3).is_empty());
    }

    #[test]
    fn test_large_trace_serialization() {
        let mut trace = Trace::new();

        // Add many entries
        for i in 0..1000 {
            trace.push(TraceEntry {
                id: i,
                parent_id: if i > 0 { Some(i - 1) } else { None },
                depth: i,
                call_type: CallType::Call(CallScheme::Call),
                caller: Address::ZERO,
                target: Address::ZERO,
                code_address: Address::ZERO,
                input: Bytes::new(),
                value: U256::from(i),
                result: Some(CallResult::Success {
                    output: Bytes::from_static(b"success"),
                    result: InstructionResult::Return,
                }),
                created_contract: false,
                create_scheme: None,
                bytecode: None,
                target_label: Some(format!("Entry{i}")),
                self_destruct: None,
                events: vec![],
                first_snapshot_id: Some(i),
            });
        }

        let json = serde_json::to_string(&trace).expect("Failed to serialize large trace");
        let deserialized: Trace =
            serde_json::from_str(&json).expect("Failed to deserialize large trace");

        assert_eq!(deserialized.len(), 1000);
        assert_eq!(deserialized[0].id, 0);
        assert_eq!(deserialized[999].id, 999);
        assert_eq!(deserialized[999].value, U256::from(999));
    }

    #[test]
    fn test_trace_entry_with_events_serialization() {
        let event_data = LogData::new_unchecked(
            vec![B256::from([1u8; 32]), B256::from([2u8; 32])],
            Bytes::from_static(b"event_data"),
        );

        let entry = TraceEntry {
            id: 0,
            parent_id: None,
            depth: 0,
            call_type: CallType::Call(CallScheme::Call),
            caller: address!("0xabcdefabcdefabcdefabcdefabcdefabcdefabcd"),
            target: address!("0x1234567890123456789012345678901234567890"),
            code_address: address!("0x1234567890123456789012345678901234567890"),
            input: Bytes::new(),
            value: U256::ZERO,
            result: None,
            created_contract: false,
            create_scheme: None,
            bytecode: None,
            target_label: None,
            self_destruct: None,
            events: vec![event_data.clone()],
            first_snapshot_id: None,
        };

        let json = serde_json::to_string(&entry).expect("Failed to serialize entry with events");
        let deserialized: TraceEntry =
            serde_json::from_str(&json).expect("Failed to deserialize entry with events");

        assert_eq!(deserialized.events.len(), 1);
        assert_eq!(deserialized.events[0].topics(), event_data.topics());
        assert_eq!(deserialized.events[0].data, event_data.data);
    }

    #[test]
    fn test_trace_entry_with_self_destruct_serialization() {
        let entry = TraceEntry {
            id: 0,
            parent_id: None,
            depth: 0,
            call_type: CallType::Call(CallScheme::Call),
            caller: Address::ZERO,
            target: Address::ZERO,
            code_address: Address::ZERO,
            input: Bytes::new(),
            value: U256::ZERO,
            result: None,
            created_contract: false,
            create_scheme: None,
            bytecode: None,
            target_label: None,
            self_destruct: Some((
                address!("0x1234567890123456789012345678901234567890"),
                U256::from(1000),
            )),
            events: vec![],
            first_snapshot_id: None,
        };

        let json =
            serde_json::to_string(&entry).expect("Failed to serialize entry with self destruct");
        let deserialized: TraceEntry =
            serde_json::from_str(&json).expect("Failed to deserialize entry with self destruct");

        assert!(deserialized.self_destruct.is_some());
        let (addr, value) = deserialized.self_destruct.unwrap();
        assert_eq!(addr, address!("0x1234567890123456789012345678901234567890"));
        assert_eq!(value, U256::from(1000));
    }

    #[test]
    fn test_helper_functions() {
        // Test format_address_short
        assert_eq!(format_address_short(Address::ZERO), "0x0");
        let addr = address!("0x1234567890123456789012345678901234567890");
        assert!(format_address_short(addr).starts_with("0x"));

        // Test format_data_preview
        assert_eq!(format_data_preview(&Bytes::new()), "0x");
        assert_eq!(format_data_preview(&Bytes::from_static(b"1234")), "0x31323334");
        let long_data = Bytes::from(vec![0xde, 0xad, 0xbe, 0xef, 0x12, 0x34]);
        assert_eq!(format_data_preview(&long_data), "0xdeadbeef… [6 bytes]");

        // Test format_ether
        assert_eq!(format_ether(U256::ZERO), "0");
        assert_eq!(format_ether(U256::from(1000000000000u64)), "0.000001"); // Less than 1 ETH
    }

    #[test]
    fn test_create_scheme_serialization() {
        let schemes = vec![CreateScheme::Create, CreateScheme::Create2 { salt: U256::from(42) }];

        for scheme in schemes {
            let call_type = CallType::Create(scheme);
            let json = serde_json::to_string(&call_type).expect("Failed to serialize CreateScheme");
            let deserialized: CallType =
                serde_json::from_str(&json).expect("Failed to deserialize CreateScheme");
            assert_eq!(deserialized, call_type);
        }
    }
}
