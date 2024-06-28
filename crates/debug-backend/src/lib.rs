//! # edb-debug-backend
//!
//! EDB's core debugging backend.

mod analysis;
pub mod artifact;
mod core;
mod handler;
mod inspector;
mod utils;

pub use core::DebugBackend;
