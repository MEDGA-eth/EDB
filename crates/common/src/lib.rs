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

// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0
//! EDB Utils - Shared functionality for EDB components
//!
//! This crate provides shared utilities used by both the edb binary
//! and the engine crate, including chain forking and transaction replay.

/// Common types used throughout the EDB ecosystem including execution traces, snapshots, and code
/// representations
pub mod types;

/// Caching utilities for storing and retrieving RPC responses to optimize performance
pub mod cache;
/// Execution context management for EDB, including environment setup and configuration
pub mod context;
/// Environment variables and configuration management for EDB components
pub mod env;
/// Expression normalization utilities for consistent handling of user-defined expressions
pub mod expression;
/// Chain forking utilities for creating and managing forked blockchain states
pub mod forking;
/// Logging setup and utilities for consistent logging across EDB components
pub mod logging;
/// Conditional assertion macros for strict testing mode
pub mod macros;
/// Extended opcode analysis utilities for EVM state modification detection and debugging
pub mod opcode;
/// Progress message types for tracking operation progress
pub mod progress;
pub mod provider_db;
/// Specification ID utilities for handling different Ethereum hardforks and protocol versions
pub mod spec_id;
/// Testing utilities and helpers for integration and unit tests
pub mod test_utils;

pub use cache::*;
pub use context::*;
pub use expression::*;
pub use forking::*;
pub use logging::*;
pub use opcode::*;
pub use progress::*;
pub use spec_id::*;
