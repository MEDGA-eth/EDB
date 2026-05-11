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

//! EDB library facade.
//!
//! This crate ships both a `bin` (`edb`) and a `lib` (`edb`). The library
//! exists primarily so integration tests can drive the same command pipeline
//! that `main.rs` does — without paying the cost of spawning a subprocess and
//! parsing its stdout.
//!
//! The full command surface (replay, server, proxy_status, test) lives under
//! [`cmd`]. The test-harness helpers used by integration tests are gated
//! behind the `test-harness` Cargo feature and live in
//! [`cmd::test::harness`](crate::cmd::test).
//!
//! Note: most internals were originally written for the `bin` target where
//! the `missing_docs` lint was a warn-by-default escape hatch. Promoting
//! them into a `lib` would turn the same warnings into errors under the
//! workspace's `-D warnings` policy. We blanket-allow `missing_docs` at the
//! library root to avoid an unrelated documentation churn within this task.

#![allow(missing_docs)]

pub mod cli;
pub mod cmd;
pub mod proxy;
pub mod utils;
pub mod ws_protocol;

pub use cli::{Cli, Commands, Ui};
