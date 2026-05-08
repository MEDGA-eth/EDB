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

//! EVM bytecode disassembly utilities
//!
//! This module provides functionality to disassemble EVM bytecode into a structured
//! representation that includes opcodes and their associated data (particularly for
//! PUSHX instructions that include immediate values).
//!
//! The disassembly process handles:
//! - All standard EVM opcodes
//! - PUSH instructions with their immediate values (PUSH1 through PUSH32)
//! - Proper instruction boundary detection
//! - Invalid opcodes identification

use alloy_primitives::{Bytes, U256};
use revm::bytecode::opcode::OpCode;

/// A single disassembled instruction with its associated data
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisassemblyInstruction {
    /// Program counter offset where this instruction starts
    pub pc: usize,
    /// The opcode for this instruction
    pub opcode: OpCode,
    /// For PUSHX instructions, this contains the immediate value bytes
    /// For other instructions, this is empty
    pub push_data: Vec<u8>,
}

impl DisassemblyInstruction {
    /// Create a new instruction without push data
    pub fn new(pc: usize, opcode: OpCode) -> Self {
        Self { pc, opcode, push_data: Vec::new() }
    }

    /// Create a new instruction with push data
    pub fn with_push_data(pc: usize, opcode: OpCode, push_data: Vec<u8>) -> Self {
        Self { pc, opcode, push_data }
    }

    /// Check if this instruction is a PUSH instruction
    pub fn is_push(&self) -> bool {
        let opcode_byte = self.opcode.get();
        (0x60..=0x7F).contains(&opcode_byte)
    }

    /// Get the size of the immediate data for this instruction
    /// Returns 0 for non-PUSH instructions
    pub fn push_size(&self) -> usize {
        if self.is_push() { (self.opcode.get() - 0x60 + 1) as usize } else { 0 }
    }

    /// Get the total instruction size (opcode + immediate data)
    pub fn instruction_size(&self) -> usize {
        1 + self.push_size()
    }
}

/// Complete disassembly result for a piece of bytecode
#[derive(Debug, Clone)]
pub struct DisassemblyResult {
    /// Original bytecode that was disassembled
    pub bytecode: Bytes,
    /// List of disassembled instructions in order
    pub instructions: Vec<DisassemblyInstruction>,
}

impl DisassemblyResult {
    /// Create a new disassembly result
    pub fn new(bytecode: Bytes, instructions: Vec<DisassemblyInstruction>) -> Self {
        Self { bytecode, instructions }
    }

    /// Get the total number of instructions
    pub fn instruction_count(&self) -> usize {
        self.instructions.len()
    }

    /// Get instruction at a specific program counter offset
    pub fn get_instruction_at_pc(&self, pc: usize) -> Option<&DisassemblyInstruction> {
        self.instructions.iter().find(|inst| inst.pc == pc)
    }

    /// Get all PUSH instructions
    pub fn get_push_instructions(&self) -> Vec<&DisassemblyInstruction> {
        self.instructions.iter().filter(|inst| inst.is_push()).collect()
    }

    /// Find the instruction that contains a given PC offset
    /// This is useful when the PC might point into the middle of a PUSH instruction's data
    pub fn find_instruction_containing_pc(&self, pc: usize) -> Option<&DisassemblyInstruction> {
        self.instructions.iter().find(|inst| {
            let start = inst.pc;
            let end = start + inst.instruction_size();
            pc >= start && pc < end
        })
    }
}

/// Disassemble EVM bytecode into a structured representation
///
/// This function parses the bytecode and extracts all opcodes along with their
/// associated immediate data (for PUSH instructions). It properly handles
/// instruction boundaries and invalid opcodes.
///
/// # Arguments
/// * `bytecode` - The bytecode to disassemble
///
/// # Returns
/// A `DisassemblyResult` containing the list of instructions
///
/// # Examples
/// ```rust
/// use alloy_primitives::Bytes;
/// use edb_engine::utils::disasm::disassemble;
///
/// let bytecode = Bytes::from(vec![0x60, 0x42, 0x80]); // PUSH1 0x42, DUP1
/// let result = disassemble(&bytecode);
/// assert_eq!(result.instructions.len(), 2);
/// assert!(result.instructions[0].is_push());
/// assert_eq!(result.instructions[0].push_data, vec![0x42]);
/// ```
pub fn disassemble(bytecode: &Bytes) -> DisassemblyResult {
    let mut instructions = Vec::new();
    let mut pc = 0;

    while pc < bytecode.len() {
        let opcode_byte = bytecode[pc];

        // Create the opcode - use new_unchecked for performance since we handle invalid opcodes
        // below
        let opcode = unsafe { OpCode::new_unchecked(opcode_byte) };

        // Handle PUSH instructions (PUSH1 through PUSH32)
        if (0x60..=0x7F).contains(&opcode_byte) {
            // PUSHX instruction
            let push_size = (opcode_byte - 0x60 + 1) as usize;
            let data_start = pc + 1;
            let data_end = data_start + push_size;

            // Extract push data, padding with zeros if bytecode is truncated
            let mut push_data = Vec::new();
            for i in data_start..data_end {
                if i < bytecode.len() {
                    push_data.push(bytecode[i]);
                } else {
                    push_data.push(0); // Pad with zeros for truncated bytecode
                }
            }

            instructions.push(DisassemblyInstruction::with_push_data(pc, opcode, push_data));
            pc = data_end;
        } else {
            // Regular instruction without immediate data
            instructions.push(DisassemblyInstruction::new(pc, opcode));
            pc += 1;
        }
    }

    DisassemblyResult::new(bytecode.clone(), instructions)
}

/// Extract immediate value from a PUSH instruction as a big-endian integer
///
/// This helper function converts the bytes from a PUSH instruction into a big-endian
/// integer representation. Useful for analyzing PUSH values numerically.
///
/// # Arguments
/// * `instruction` - The PUSH instruction to extract value from
///
/// # Returns
/// The immediate value as a U256, or None if not a PUSH instruction
///
/// # Examples
/// ```rust
/// use edb_engine::utils::disasm::{DisassemblyInstruction, extract_push_value};
/// use revm::bytecode::opcode::OpCode;
///
/// let push_inst = DisassemblyInstruction::with_push_data(
///     0,
///     unsafe { OpCode::new_unchecked(0x60) }, // PUSH1
///     vec![0x42],
/// );
/// assert_eq!(extract_push_value(&push_inst), 0x42);
/// ```
pub fn extract_push_value(instruction: &DisassemblyInstruction) -> Option<U256> {
    if !instruction.is_push() || instruction.push_data.is_empty() {
        return None;
    }

    let mut value = U256::ZERO;
    for &byte in &instruction.push_data {
        value = value.wrapping_shl(8).wrapping_add(U256::from(byte));
    }
    Some(value)
}

/// Format a disassembly instruction as a human-readable string
///
/// This function creates a formatted string representation of an instruction,
/// similar to what you'd see in a standard disassembler.
///
/// # Arguments
/// * `instruction` - The instruction to format
/// * `show_pc` - Whether to include the program counter in the output
///
/// # Returns
/// A formatted string representation of the instruction
///
/// # Examples
/// ```rust
/// use edb_engine::utils::disasm::{DisassemblyInstruction, format_instruction};
/// use revm::bytecode::opcode::OpCode;
///
/// let push_inst = DisassemblyInstruction::with_push_data(
///     10,
///     unsafe { OpCode::new_unchecked(0x61) }, // PUSH2
///     vec![0x12, 0x34],
/// );
/// let formatted = format_instruction(&push_inst, true);
/// assert!(formatted.contains("PUSH2"));
/// assert!(formatted.contains("0x1234"));
/// ```
pub fn format_instruction(instruction: &DisassemblyInstruction, show_pc: bool) -> String {
    let pc_part = if show_pc { format!("{:04x}: ", instruction.pc) } else { String::new() };

    let opcode_name = if instruction.opcode.is_valid() {
        instruction.opcode.as_str().to_string()
    } else {
        format!("'{:x}'(Unknown Opcode)", instruction.opcode.get())
    };

    if instruction.is_push() && !instruction.push_data.is_empty() {
        let hex_data = instruction.push_data.iter().map(|b| format!("{b:02x}")).collect::<String>();
        format!("{pc_part}{opcode_name} 0x{hex_data}")
    } else {
        format!("{pc_part}{opcode_name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Bytes;

    #[test]
    fn test_disassemble_simple() {
        let bytecode = Bytes::from(vec![0x80, 0x81, 0x82]); // DUP1, DUP2, DUP3
        let result = disassemble(&bytecode);

        assert_eq!(result.instructions.len(), 3);
        assert_eq!(result.instructions[0].pc, 0);
        assert_eq!(result.instructions[1].pc, 1);
        assert_eq!(result.instructions[2].pc, 2);

        for inst in &result.instructions {
            assert!(!inst.is_push());
            assert!(inst.push_data.is_empty());
        }
    }

    #[test]
    fn test_disassemble_push_instructions() {
        let bytecode = Bytes::from(vec![
            0x60, 0x42, // PUSH1 0x42
            0x61, 0x12, 0x34, // PUSH2 0x1234
            0x80, // DUP1
        ]);
        let result = disassemble(&bytecode);

        assert_eq!(result.instructions.len(), 3);

        // PUSH1
        assert_eq!(result.instructions[0].pc, 0);
        assert!(result.instructions[0].is_push());
        assert_eq!(result.instructions[0].push_data, vec![0x42]);
        assert_eq!(result.instructions[0].instruction_size(), 2);

        // PUSH2
        assert_eq!(result.instructions[1].pc, 2);
        assert!(result.instructions[1].is_push());
        assert_eq!(result.instructions[1].push_data, vec![0x12, 0x34]);
        assert_eq!(result.instructions[1].instruction_size(), 3);

        // DUP1
        assert_eq!(result.instructions[2].pc, 5);
        assert!(!result.instructions[2].is_push());
        assert!(result.instructions[2].push_data.is_empty());
        assert_eq!(result.instructions[2].instruction_size(), 1);
    }

    #[test]
    fn test_extract_push_value() {
        let push1 = DisassemblyInstruction::with_push_data(
            0,
            unsafe { OpCode::new_unchecked(0x60) },
            vec![0x42],
        );
        assert_eq!(extract_push_value(&push1), Some(U256::from(0x42)));

        let push2 = DisassemblyInstruction::with_push_data(
            0,
            unsafe { OpCode::new_unchecked(0x61) },
            vec![0x12, 0x34],
        );
        assert_eq!(extract_push_value(&push2), Some(U256::from(0x1234)));

        let push4 = DisassemblyInstruction::with_push_data(
            0,
            unsafe { OpCode::new_unchecked(0x63) },
            vec![0x12, 0x34, 0x56, 0x78],
        );
        assert_eq!(extract_push_value(&push4), Some(U256::from(0x12345678)));
    }

    #[test]
    fn test_find_instruction_containing_pc() {
        let bytecode = Bytes::from(vec![
            0x60, 0x42, // PUSH1 0x42 (PC 0-1)
            0x61, 0x12, 0x34, // PUSH2 0x1234 (PC 2-4)
            0x80, // DUP1 (PC 5)
        ]);
        let result = disassemble(&bytecode);

        // PC 0 and 1 should find the PUSH1 instruction
        assert_eq!(result.find_instruction_containing_pc(0).unwrap().pc, 0);
        assert_eq!(result.find_instruction_containing_pc(1).unwrap().pc, 0);

        // PC 2, 3, and 4 should find the PUSH2 instruction
        assert_eq!(result.find_instruction_containing_pc(2).unwrap().pc, 2);
        assert_eq!(result.find_instruction_containing_pc(3).unwrap().pc, 2);
        assert_eq!(result.find_instruction_containing_pc(4).unwrap().pc, 2);

        // PC 5 should find the DUP1 instruction
        assert_eq!(result.find_instruction_containing_pc(5).unwrap().pc, 5);

        // PC beyond bytecode should return None
        assert!(result.find_instruction_containing_pc(6).is_none());
    }

    #[test]
    fn test_truncated_push_instruction() {
        let bytecode = Bytes::from(vec![0x61, 0x12]); // PUSH2 but only 1 byte of data
        let result = disassemble(&bytecode);

        assert_eq!(result.instructions.len(), 1);
        assert!(result.instructions[0].is_push());
        assert_eq!(result.instructions[0].push_data, vec![0x12, 0x00]); // Padded with zero
    }

    #[test]
    fn test_format_instruction() {
        let push_inst = DisassemblyInstruction::with_push_data(
            10,
            unsafe { OpCode::new_unchecked(0x61) },
            vec![0x12, 0x34],
        );

        let with_pc = format_instruction(&push_inst, true);
        assert_eq!(with_pc, "000a: PUSH2 0x1234");

        let without_pc = format_instruction(&push_inst, false);
        assert_eq!(without_pc, "PUSH2 0x1234");

        let regular_inst = DisassemblyInstruction::new(5, unsafe { OpCode::new_unchecked(0x80) });

        let regular_formatted = format_instruction(&regular_inst, true);
        assert_eq!(regular_formatted, "0005: DUP1");
    }
}
