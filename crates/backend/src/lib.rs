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
use std::fmt::Display;

use alloy_primitives::Address;

/// Runtime Address. It can be either a constructor address or a deployed address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeAddress {
    pub address: Address,
    pub is_constructor: bool,
}

impl Display for RuntimeAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_constructor {
            write!(f, "constructor@{}", self.address)
        } else {
            write!(f, "runtime@{}", self.address)
        }
    }
}

impl RuntimeAddress {
    pub fn new(address: Address, is_constructor: bool) -> Self {
        Self { address, is_constructor }
    }

    pub fn constructor(address: Address) -> Self {
        Self::new(address, true)
    }

    pub fn deployed(address: Address) -> Self {
        Self::new(address, false)
    }
}
