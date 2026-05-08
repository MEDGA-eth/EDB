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

use revm::bytecode::OpCode;

/// Extended trait for EVM opcode analysis
///
/// Provides additional methods for determining what types of EVM state
/// an opcode will modify when executed. This is useful for:
/// - Debugging tools that need to track state changes
/// - Optimization of state snapshots and memory sharing
/// - Security analysis of contract execution
pub trait OpcodeTr {
    /// Check if this opcode modifies persistent EVM state
    ///
    /// Returns `true` if the opcode will modify any persistent state including:
    /// - Storage (via SSTORE)
    /// - Account state (via CREATE, CREATE2, SELFDESTRUCT)
    /// - Account balances (via CALL, CALLCODE with value transfer)
    /// - Event logs (via LOG0-LOG4)
    ///
    /// Note: This does NOT include temporary state like memory or stack,
    /// only state that persists beyond the current transaction execution.
    ///
    /// # Example
    /// ```rust
    /// use edb_common::OpcodeTr;
    /// use revm::bytecode::OpCode;
    ///
    /// assert!(OpCode::SSTORE.modifies_evm_state());
    /// assert!(OpCode::CREATE.modifies_evm_state());
    /// assert!(!OpCode::ADD.modifies_evm_state());
    /// assert!(!OpCode::MSTORE.modifies_evm_state()); // Only modifies memory
    /// ```
    fn modifies_evm_state(&self) -> bool;

    /// Check if this opcode modifies transient storage (EIP-1153)
    ///
    /// Transient storage is temporary storage that exists only for the
    /// duration of a transaction and is cleared when the transaction ends.
    ///
    /// Returns `true` only for:
    /// - `TSTORE` (0x5C): Write to transient storage
    ///
    /// Note: `TLOAD` (0x5D) only reads transient storage and doesn't modify it.
    ///
    /// # Example
    /// ```rust
    /// use edb_common::OpcodeTr;
    /// use revm::bytecode::OpCode;
    ///
    /// assert!(OpCode::TSTORE.modifies_transient_storage());
    /// assert!(!OpCode::TLOAD.modifies_transient_storage());
    /// assert!(!OpCode::SSTORE.modifies_transient_storage());
    /// ```
    fn modifies_transient_storage(&self) -> bool;

    /// Check if this opcode is a call instruction
    fn is_message_call(&self) -> bool;

    /// Check if this opcode is a contract creation call
    fn is_creation_call(&self) -> bool;
}

impl OpcodeTr for OpCode {
    fn modifies_evm_state(&self) -> bool {
        matches!(
            *self,
            // Storage modifications - writes to persistent contract storage
            Self::SSTORE |

            // Account state changes - create/destroy accounts
            Self::CREATE |     // Create new contract account
            Self::CREATE2 |    // Create new contract with deterministic address
            Self::SELFDESTRUCT | // Destroy current contract and transfer balance

            // Balance transfers - modify account balances
            Self::CALL |       // External call that can transfer ETH
            Self::CALLCODE |   // Call with current account context (deprecated)

            // Note: DELEGATECALL and STATICCALL do not transfer value.
            // At the same time, we do not care about gas modifications.
            // We hence do not consider them as state-modifying opcodes.
            // Self::DELEGATECALL | // Call with caller's context (no value transfer)
            // Self::STATICCALL |   // Static call (no state modifications)

            // Log emissions - add entries to transaction receipt logs
            Self::LOG0 | Self::LOG1 | Self::LOG2 | Self::LOG3 | Self::LOG4
        )
    }

    fn modifies_transient_storage(&self) -> bool {
        matches!(
            *self,
            Self::TSTORE // Write to transient storage (EIP-1153)
        )
    }

    fn is_message_call(&self) -> bool {
        matches!(
            *self,
            Self::CREATE
                | Self::CREATE2
                | Self::CALL
                | Self::CALLCODE
                | Self::DELEGATECALL
                | Self::STATICCALL
        )
    }

    fn is_creation_call(&self) -> bool {
        matches!(*self, Self::CREATE | Self::CREATE2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modifies_evm_state_storage() {
        // Storage modification
        assert!(OpCode::SSTORE.modifies_evm_state());

        // Storage read doesn't modify state
        assert!(!OpCode::SLOAD.modifies_evm_state());
    }

    #[test]
    fn test_modifies_evm_state_account_creation() {
        assert!(OpCode::CREATE.modifies_evm_state());
        assert!(OpCode::CREATE2.modifies_evm_state());
        assert!(OpCode::SELFDESTRUCT.modifies_evm_state());
    }

    #[test]
    fn test_modifies_evm_state_calls() {
        // Calls that can transfer value
        assert!(OpCode::CALL.modifies_evm_state());
        assert!(OpCode::CALLCODE.modifies_evm_state());

        // Calls that cannot transfer value
        assert!(!OpCode::DELEGATECALL.modifies_evm_state());
        assert!(!OpCode::STATICCALL.modifies_evm_state());
    }

    #[test]
    fn test_modifies_evm_state_logs() {
        assert!(OpCode::LOG0.modifies_evm_state());
        assert!(OpCode::LOG1.modifies_evm_state());
        assert!(OpCode::LOG2.modifies_evm_state());
        assert!(OpCode::LOG3.modifies_evm_state());
        assert!(OpCode::LOG4.modifies_evm_state());
    }

    #[test]
    fn test_modifies_evm_state_arithmetic() {
        // Arithmetic operations don't modify state
        assert!(!OpCode::ADD.modifies_evm_state());
        assert!(!OpCode::SUB.modifies_evm_state());
        assert!(!OpCode::MUL.modifies_evm_state());
        assert!(!OpCode::DIV.modifies_evm_state());
        assert!(!OpCode::MOD.modifies_evm_state());
    }

    #[test]
    fn test_modifies_evm_state_memory() {
        // Memory operations don't modify persistent state
        assert!(!OpCode::MSTORE.modifies_evm_state());
        assert!(!OpCode::MSTORE8.modifies_evm_state());
        assert!(!OpCode::MLOAD.modifies_evm_state());
    }

    #[test]
    fn test_modifies_evm_state_stack() {
        // Stack operations don't modify persistent state
        assert!(!OpCode::PUSH0.modifies_evm_state());
        assert!(!OpCode::PUSH1.modifies_evm_state());
        assert!(!OpCode::POP.modifies_evm_state());
        assert!(!OpCode::DUP1.modifies_evm_state());
        assert!(!OpCode::SWAP1.modifies_evm_state());
    }

    #[test]
    fn test_modifies_transient_storage() {
        // Only TSTORE modifies transient storage
        assert!(OpCode::TSTORE.modifies_transient_storage());

        // TLOAD only reads
        assert!(!OpCode::TLOAD.modifies_transient_storage());

        // Regular storage operations don't affect transient storage
        assert!(!OpCode::SSTORE.modifies_transient_storage());
        assert!(!OpCode::SLOAD.modifies_transient_storage());

        // Other operations
        assert!(!OpCode::ADD.modifies_transient_storage());
        assert!(!OpCode::MSTORE.modifies_transient_storage());
    }

    #[test]
    fn test_is_call() {
        // Contract creation
        assert!(OpCode::CREATE.is_message_call());
        assert!(OpCode::CREATE2.is_message_call());

        // Various call types
        assert!(OpCode::CALL.is_message_call());
        assert!(OpCode::CALLCODE.is_message_call());
        assert!(OpCode::DELEGATECALL.is_message_call());
        assert!(OpCode::STATICCALL.is_message_call());

        // Non-call operations
        assert!(!OpCode::SSTORE.is_message_call());
        assert!(!OpCode::SLOAD.is_message_call());
        assert!(!OpCode::ADD.is_message_call());
        assert!(!OpCode::JUMP.is_message_call());
        assert!(!OpCode::RETURN.is_message_call());
        assert!(!OpCode::REVERT.is_message_call());
    }

    #[test]
    fn test_control_flow_opcodes() {
        // Control flow opcodes don't modify state
        assert!(!OpCode::JUMP.modifies_evm_state());
        assert!(!OpCode::JUMPI.modifies_evm_state());
        assert!(!OpCode::PC.modifies_evm_state());
        assert!(!OpCode::JUMPDEST.modifies_evm_state());

        // They also don't modify transient storage
        assert!(!OpCode::JUMP.modifies_transient_storage());
        assert!(!OpCode::JUMPI.modifies_transient_storage());

        // And they're not calls
        assert!(!OpCode::JUMP.is_message_call());
        assert!(!OpCode::JUMPI.is_message_call());
    }

    #[test]
    fn test_environment_opcodes() {
        // Environment info opcodes don't modify state
        assert!(!OpCode::ADDRESS.modifies_evm_state());
        assert!(!OpCode::BALANCE.modifies_evm_state());
        assert!(!OpCode::ORIGIN.modifies_evm_state());
        assert!(!OpCode::CALLER.modifies_evm_state());
        assert!(!OpCode::CALLVALUE.modifies_evm_state());
        assert!(!OpCode::CALLDATALOAD.modifies_evm_state());
        assert!(!OpCode::CALLDATASIZE.modifies_evm_state());
        assert!(!OpCode::CODESIZE.modifies_evm_state());
        assert!(!OpCode::GASPRICE.modifies_evm_state());
    }

    #[test]
    fn test_return_opcodes() {
        // Return and revert don't modify state (they may undo changes but don't create new state)
        assert!(!OpCode::RETURN.modifies_evm_state());
        assert!(!OpCode::REVERT.modifies_evm_state());
        assert!(!OpCode::STOP.modifies_evm_state());
        assert!(!OpCode::INVALID.modifies_evm_state());

        // SELFDESTRUCT does modify state
        assert!(OpCode::SELFDESTRUCT.modifies_evm_state());
    }
}
