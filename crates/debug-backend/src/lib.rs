//! # edb-debug-backend
//!
//! EDB's core debugging backend.

pub mod artifact;
mod core;
mod handler;
mod inspector;
mod utils;

pub use core::DebugBackend;
