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

//! Call tracer for collecting complete execution trace during transaction replay
//!
//! This inspector captures the complete call trace including call stack, creation events,
//! and execution flow. The trace can be replayed later to determine execution paths
//! without needing to re-examine transaction inputs/outputs.

use alloy_primitives::{Address, B256, Log, U256, keccak256};
use edb_common::types::{CallResult, Trace, TraceEntry};
use revm::{
    Inspector,
    context::ContextTr,
    interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter},
};
use std::{collections::HashMap, ops::Deref};
use tracing::{debug, error};

/// Result of transaction replay with call trace
#[derive(Debug)]
pub struct TraceReplayResult {
    /// All addresses visited during execution
    pub visited_addresses: HashMap<Address, bool>,
    /// For contracts deployed during this transaction, the keccak256 of their
    /// post-deployment runtime bytecode. Used to look up local artifacts by
    /// codehash for in-tx-deployed contracts (the pre-tx state doesn't yet
    /// contain these addresses, so a `db.basic(addr)` from the original
    /// context would return `KECCAK_EMPTY`).
    pub in_tx_codehashes: HashMap<Address, B256>,
    /// Complete execution trace with call/create details
    pub execution_trace: Trace,
}

/// Complete call tracer that captures execution flow
#[derive(Debug, Default)]
pub struct CallTracer {
    /// Sequential list of all calls/creates in execution order
    pub trace: Trace,
    /// Map of visited addresses to whether they were deployed in this transaction
    pub visited_addresses: HashMap<Address, bool>,
    /// Keccak256 of the post-deploy runtime bytecode for each address that was
    /// CREATEd during this transaction. Populated in `create_end`.
    pub in_tx_codehashes: HashMap<Address, B256>,
    /// Stack to track call indices for proper nesting
    call_stack: Vec<usize>,
}

impl CallTracer {
    /// Create a new call tracer
    pub fn new() -> Self {
        Self {
            trace: Trace::default(),
            visited_addresses: HashMap::new(),
            in_tx_codehashes: HashMap::new(),
            call_stack: Vec::new(),
        }
    }

    /// Get all visited addresses
    pub fn visited_addresses(&self) -> &HashMap<Address, bool> {
        &self.visited_addresses
    }

    /// Get the complete execution trace
    pub fn execution_trace(&self) -> &Trace {
        &self.trace
    }

    /// Convert the call tracer into a replay result
    pub fn into_replay_result(self) -> TraceReplayResult {
        TraceReplayResult {
            visited_addresses: self.visited_addresses,
            in_tx_codehashes: self.in_tx_codehashes,
            execution_trace: self.trace,
        }
    }

    /// Add an address to the visited set
    fn mark_address_visited(&mut self, address: Address, deployed: bool) {
        self.visited_addresses
            .entry(address)
            .and_modify(|existing| *existing |= deployed)
            .or_insert(deployed);
    }
}

impl<CTX: ContextTr> Inspector<CTX> for CallTracer {
    fn step(&mut self, interp: &mut Interpreter, _context: &mut CTX) {
        let Some(entry) = self.trace.last_mut() else {
            debug!("Trace is empty, cannot step");
            return;
        };

        if entry.bytecode.is_some() {
            // We already update the bytecode for the current entry
            return;
        }

        entry.bytecode = Some(interp.bytecode.bytes());
    }

    fn call(&mut self, context: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        let call_type = inputs.into();
        let target = inputs.target_address;
        let code_address = inputs.bytecode_address;
        let caller = inputs.caller;

        // Mark addresses as visited
        self.mark_address_visited(target, false);
        self.mark_address_visited(code_address, false);
        self.mark_address_visited(caller, false);

        // Determine the parent ID from the current call stack
        let parent_id = self.call_stack.last().copied();
        let trace_id = self.trace.len();

        // Create trace entry
        let trace_entry = TraceEntry {
            id: trace_id,
            parent_id,
            depth: self.call_stack.len(),
            call_type,
            caller,
            target,
            code_address,
            input: inputs.input.bytes(context),
            value: inputs.transfer_value().unwrap_or(U256::ZERO),
            result: None,        // Will be filled in call_end
            events: vec![],      // Will be filled in log
            self_destruct: None, // Will be filled in self_destruct
            created_contract: false,
            create_scheme: None,
            bytecode: None,          // Will be set in step
            target_label: None,      // Will be set in post-analysis
            first_snapshot_id: None, // Will be set in post-analysis
        };

        // Add to trace and update stack
        self.trace.push(trace_entry);
        self.call_stack.push(trace_id);

        None // Continue with normal execution
    }

    fn call_end(&mut self, _context: &mut CTX, inputs: &CallInputs, outcome: &mut CallOutcome) {
        // Pop from call stack and update result
        let Some(trace_index) = self.call_stack.pop() else {
            error!("Call stack underflow - no matching call entry found");
            return;
        };

        let Some(trace_entry) = self.trace.get_mut(trace_index) else {
            error!("Call stack entry not found");
            return;
        };

        trace_entry.result = Some(outcome.into());

        let target = inputs.target_address;
        let code_address = inputs.bytecode_address;
        let caller = inputs.caller;
        if trace_entry.target != target
            || trace_entry.code_address != code_address
            || trace_entry.caller != caller
        {
            error!("Call stack entry mismatch");
        }
    }

    fn create(&mut self, _context: &mut CTX, inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        let call_type = inputs.into();
        let caller = inputs.caller();

        // Mark addresses
        self.mark_address_visited(caller, false);

        // Determine the parent ID from the current call stack
        let parent_id = self.call_stack.last().copied();
        let trace_id = self.trace.len();

        // Create trace entry
        let trace_entry = TraceEntry {
            id: trace_id,
            parent_id,
            depth: self.call_stack.len(),
            call_type,
            caller,
            target: Address::ZERO,       // Target is not known yet
            code_address: Address::ZERO, // Code address is not known yet
            input: inputs.init_code().clone(),
            value: inputs.value(),
            result: None,            // Will be filled in create_end
            events: vec![],          // Will be filled in log
            self_destruct: None,     // Will be filled in self_destruct
            created_contract: false, // Will be updated in create_end
            create_scheme: Some(inputs.scheme()),
            bytecode: None,          // Will be set in step
            target_label: None,      // Will be set in post-analysis
            first_snapshot_id: None, // Will be set in post-analysis
        };

        // Add to trace and update stack
        self.trace.push(trace_entry);
        self.call_stack.push(trace_id);

        None // Continue with normal execution
    }

    fn create_end(
        &mut self,
        _context: &mut CTX,
        inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        // Pop from call stack and update result
        let Some(trace_index) = self.call_stack.pop() else {
            error!("Call stack underflow - no matching create entry found");
            return;
        };

        let Some(trace_entry) = self.trace.get_mut(trace_index) else {
            error!("Trace entry not found");
            return;
        };

        // Check caller consistence first, and no need to return
        let caller = inputs.caller();
        if trace_entry.caller != caller {
            error!("Create stack entry mismatch");
        }

        trace_entry.result = Some(outcome.into());

        if matches!(trace_entry.result, Some(CallResult::Revert { .. })) {
            debug!("Creation failed");
            return;
        }

        let Some(created_address) = outcome.address else {
            error!("Create outcome did not provide created address");
            return;
        };

        trace_entry.target = created_address;
        trace_entry.code_address = created_address;
        trace_entry.created_contract = true;

        let created_address_for_marking = trace_entry.target;
        // Mark address after trace_entry is no longer borrowed
        let _ = trace_entry;

        self.mark_address_visited(created_address_for_marking, true);

        // The CREATE output is the post-deploy runtime bytecode; its keccak256
        // is what the journaled state will store as `account.info.code_hash`.
        // Record it so the engine can look up local artifacts by codehash for
        // contracts deployed inside the current transaction (the pre-tx
        // database doesn't yet know these addresses).
        let runtime_code = &outcome.result.output;
        if !runtime_code.is_empty() {
            self.in_tx_codehashes
                .insert(created_address_for_marking, keccak256(runtime_code.as_ref()));
        }
    }

    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        // Mark both addresses as visited
        self.mark_address_visited(contract, false);
        self.mark_address_visited(target, false);

        let Some(entry) = self.trace.last_mut() else {
            error!("Trace is empty, cannot step");
            return;
        };

        if entry.target != contract {
            error!("Self-destruct entry mismatch");
            return;
        }
        entry.self_destruct = Some((target, value));
    }

    fn log(&mut self, _context: &mut CTX, log: Log) {
        let Some(entry) = self.trace.last_mut() else {
            error!("Trace is empty, cannot log");
            return;
        };

        entry.events.push(log.deref().clone());
    }
}
