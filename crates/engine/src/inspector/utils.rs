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

//! Utility functions for inspectors.

use alloy_primitives::U256;
use edb_common::OpcodeTr;
use revm::{
    bytecode::OpCode,
    interpreter::{Interpreter, interpreter_types::Jumps},
};

#[inline]
pub fn relax_gas_limit_at_callsite(interp: &mut Interpreter) {
    let opcode = unsafe { OpCode::new_unchecked(interp.bytecode.opcode()) };

    // There are four call opcodes: CALL, CALLCODE, DELEGATECALL, STATICCALL.
    // Luckily, gas limit is always the top of the stack for all four opcodes:
    // 1. CALL: gas, address, value, argsOffset, argsSize, retOffset, retSize
    // 2. CALLCODE: gas, address, value, argsOffset, argsSize, retOffset, retSize
    // 3. DELEGATECALL: gas, address, argsOffset, argsSize, retOffset, retSize
    // 4. STATICCALL: gas, address, argsOffset, argsSize, retOffset, retSize
    if !opcode.is_message_call() || opcode.is_creation_call() {
        return;
    }

    // We relax the gas limit at callsites to avoid OOG issues during debugging.
    // Set a high gas limit (u64::MAX) for the call
    unsafe { *interp.stack.top_unsafe() = U256::from(u64::MAX) };
}
