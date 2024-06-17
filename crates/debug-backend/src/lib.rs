//! # edb-debug-backend
//!
//! EDB's core debugging backend.

mod artifact;
mod backend;
mod handler;
mod inspector;
mod utils;

pub use backend::DebugBackend;
