//! # edb-debug-backend
//!
//! EDB's core debugging backend.

#[macro_use]
extern crate tracing;

pub mod analysis;
pub mod artifact;
mod core;
mod handler;
mod inspector;
mod utils;

pub use core::DebugBackend;
