//! We define `Step` as the smallest unit of execution during debugging. It can be a statement, or an
//! expression, or a block.

use alloy_primitives::{address, Address};
use foundry_compilers::artifacts::{Block, Statement};

use crate::analysis::source_map::debug_unit::UnitLocation;

use super::uvid::UVID;

/// The address of the step marker. The value is the first 20 bytes of `keccak256("EDB_STEP_MARKER")`.
pub const EDB_STEP_MARKER: Address = address!("550d659061947dc89537766a90b82749b2294cd5");

/// The context of a step.
#[derive(Debug)]
pub struct StepContext {
    /// The source code that this step contains.
    pub unit: UnitLocation,

    /// The variables that are updated in this step.
    pub updated_vars: Vec<UVID>,
}

/// A step in the execution.
pub enum Step {
    /// A statement.
    Statement(Statement, StepContext),

    /// A block.
    Block(Block, StepContext),
}
