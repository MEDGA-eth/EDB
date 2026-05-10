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

//! Utility functions and helpers for the EDB engine.
//!
//! This module provides a collection of utility functions and helper modules that support
//! various aspects of the EDB debugging engine. These utilities handle common tasks across
//! contract analysis, compilation, source code processing, and external service integration.
//!
//! # Core Utility Modules
//!
//! ## Contract and Artifact Management
//! - [`artifact`] - Contract artifact handling and metadata management
//! - [`compilation`] - Solidity compilation utilities and configuration
//! - [`abi`] - ABI processing and type conversion utilities
//!
//! ## Source Code Processing
//! - [`source`] - Source code analysis and manipulation utilities
//! - [`ast_prune`] - AST pruning and optimization for instrumentation
//!
//! ## Bytecode Analysis
//! - [`disasm`] - EVM bytecode disassembly and analysis utilities
//!
//! ## External Service Integration
//! - [`etherscan`] - Etherscan API integration and data fetching utilities
//!
//! # Design Philosophy
//!
//! The utilities in this module are designed to be:
//! - **Reusable**: Common functionality shared across multiple engine components
//! - **Efficient**: Optimized for performance in debugging scenarios
//! - **Reliable**: Robust error handling and edge case management
//! - **Modular**: Independent modules that can be used separately
//!
//! Each utility module focuses on a specific domain while providing clean interfaces
//! for integration with the broader EDB engine ecosystem.

mod artifact;
pub use artifact::*;

mod ast_prune;
pub use ast_prune::*;

mod errors;
pub use errors::*;

pub mod disasm;
pub use disasm::*;

mod etherscan;
pub use etherscan::*;

mod sourcify;
pub use sourcify::*;

mod compilation;
pub use compilation::*;

mod source;
pub use source::*;

mod abi;
pub use abi::*;

mod persistent_data;
pub use persistent_data::*;

mod visitor;
pub use visitor::*;
