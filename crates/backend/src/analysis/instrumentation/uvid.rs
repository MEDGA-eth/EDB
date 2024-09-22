use std::sync::Mutex;

use alloy_primitives::U256;
use lazy_static::lazy_static;

/// The slot where the `edb_runtime_values` mapping is stored.
/// The value is the first 8 bytes of the keccak256 hash of the string "EDB_RUNTIME_VALUE_OFFSET".
pub const EDB_RUNTIME_VALUE_OFFSET: u64 = 0x234c6dfc3bf8fed1;

/// A Universal Variable Identifier (UVID) is a unique identifier for a variable in contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct UVID(u64);

impl UVID {
    /// Increment the UVID and return the previous value.
    pub fn inc(&mut self) -> UVID {
        let v = *self;
        self.0 += 1;
        v
    }
}

impl From<UVID> for u64 {
    fn from(uvid: UVID) -> u64 {
        uvid.0
    }
}

impl From<UVID> for U256 {
    fn from(uvid: UVID) -> U256 {
        U256::from(uvid.0)
    }
}

lazy_static! {
    pub static ref NEXT_UVID: Mutex<UVID> = Mutex::new(UVID(EDB_RUNTIME_VALUE_OFFSET));
}

/// Generate a new UVID.
pub fn new_uvid() -> UVID {
    let mut uvid = NEXT_UVID.lock().unwrap();
    uvid.inc()
}
