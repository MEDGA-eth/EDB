//! # edb-debug-backend
//!
//! EDB's core debugging backend.

mod artifact;
mod handler;
mod inspector;
mod core;
mod utils;

pub use core::DebugBackend;
