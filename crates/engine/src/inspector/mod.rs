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

//! EVM execution inspectors for debugging and analysis.
//!
//! This module provides various inspector implementations that hook into the EVM
//! execution process to collect debugging information, create snapshots, and
//! modify execution behavior for debugging purposes.
//!
//! # Inspector Types
//!
//! ## [`CallTracer`]
//! Traces all contract calls during execution, building a hierarchical call tree
//! that captures the complete execution flow including internal calls, delegate calls,
//! and create operations.
//!
//! ## [`HookSnapshotInspector`]
//! Creates detailed snapshots at specific hook points during execution, capturing
//! local variables, state variables, and execution context for source-level debugging.
//! This inspector works with source maps and debug information to provide high-level
//! debugging capabilities.
//!
//! ## [`OpcodeSnapshotInspector`]
//! Creates snapshots at the opcode level, capturing low-level EVM state including
//! stack, memory, storage, and transient storage. Useful for detailed execution
//! analysis and opcode-level debugging.
//!
//! ## [`TweakInspector`]
//! Allows runtime modification of contract bytecode and behavior for debugging
//! purposes. Can inject custom logic, modify return values, and alter execution
//! flow to test different scenarios.
//!
//! # Usage
//!
//! Inspectors are typically chained together to provide comprehensive debugging:
//!
//! ```rust,ignore
//! let call_tracer = CallTracer::new();
//! let hook_inspector = HookSnapshotInspector::new(context);
//! let opcode_inspector = OpcodeSnapshotInspector::new(context);
//! ```
//!
//! # Architecture
//!
//! All inspectors implement the REVM `Inspector` trait. Each inspector focuses
//! on a specific aspect of execution analysis while maintaining minimal overhead
//! when not actively collecting data.

mod call_tracer;
mod cheated_stack;
mod hook_snapshot_inspector;
mod opcode_snapshot_inspector;
mod tweak_inspector;
mod utils;

pub use call_tracer::*;
pub use cheated_stack::*;
pub use hook_snapshot_inspector::*;
pub use opcode_snapshot_inspector::*;
pub use tweak_inspector::*;

#[cfg(test)]
mod cheated_stack_tests;
