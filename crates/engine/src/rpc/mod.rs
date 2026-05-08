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

//! JSON-RPC server for EDB debugging control and inspection.
//!
//! This module provides a comprehensive JSON-RPC API that enables front-end applications
//! and debugging clients to interact with the EDB debugging engine. The RPC interface
//! exposes all debugging functionality including snapshot navigation, expression evaluation,
//! storage inspection, and trace analysis.
//!
//! # Architecture
//!
//! The RPC system consists of several key components:
//!
//! - **Server** ([`server`]) - HTTP/WebSocket server handling client connections
//! - **Methods** ([`methods`]) - RPC method implementations organized by functionality
//! - **Types** ([`types`]) - Request/response data structures and protocol types
//! - **Utils** ([`utils`]) - Common utilities for RPC operations
//!
//! # API Categories
//!
//! ## Snapshot Management
//! - Navigate through execution snapshots (forward/backward)
//! - Access snapshot details and execution context
//! - Jump to specific execution points
//!
//! ## Expression Evaluation
//! - Evaluate Solidity-like expressions against any snapshot
//! - Access variables, function calls, and blockchain context
//! - Real-time expression evaluation with full debugging context
//!
//! ## Storage and State Inspection
//! - Read contract storage and state variables
//! - Inspect memory, stack, and call data
//! - Access transient storage and EVM state
//!
//! ## Trace Analysis
//! - Navigate the complete execution trace
//! - Analyze call patterns and execution flow
//! - Access detailed opcode-level execution information
//!
//! ## Artifact Management
//! - Access compiled contract artifacts and metadata
//! - Retrieve source code mappings and debugging information
//! - Manage contract compilation and analysis results
//!
//! # Usage
//!
//! ```rust,ignore
//! use edb_engine::rpc::{DebugServer, DebugServerConfig};
//!
//! // Start the RPC server
//! let config = DebugServerConfig::default();
//! let server = DebugServer::new(engine_context, config).await?;
//! server.run().await?;
//! ```
//!
//! # Protocol
//!
//! The RPC server supports both HTTP POST requests and WebSocket connections
//! for real-time debugging. All methods follow the JSON-RPC 2.0 specification
//! with structured request/response formats defined in the [`types`] module.

pub mod methods;
pub mod server;
pub mod types;
pub mod utils;

pub use server::*;
pub use types::*;

/// Re-export of `axum::Router` for callers that want to attach extra routes
/// via [`server::DebugRpcServer::with_extra_router`].
pub use axum::Router;
