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

//! Ethereum opcode syntax highlighting
//!
//! This module provides syntax highlighting for Ethereum Virtual Machine (EVM)
//! opcodes and assembly language with semantic token recognition.

use super::{SyntaxToken, TokenType};
use regex::Regex;
use std::sync::OnceLock;

/// Ethereum opcode syntax highlighter
#[derive(Debug)]
pub struct OpcodeHighlighter {
    patterns: &'static OpcodePatterns,
}

/// Opcode syntax patterns
struct OpcodePatterns {
    opcodes: Regex,
    push_opcodes: Regex,
    dup_swap_opcodes: Regex,
    log_opcodes: Regex,
    numbers: Regex,
    hex_addresses: Regex,
    hex_data: Regex,
    stack_positions: Regex,
    comments: Regex,
}

impl std::fmt::Debug for OpcodePatterns {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpcodePatterns")
            .field("opcodes", &"<regex>")
            .field("push_opcodes", &"<regex>")
            .field("dup_swap_opcodes", &"<regex>")
            .field("log_opcodes", &"<regex>")
            .field("numbers", &"<regex>")
            .field("hex_addresses", &"<regex>")
            .field("hex_data", &"<regex>")
            .field("stack_positions", &"<regex>")
            .field("comments", &"<regex>")
            .finish()
    }
}

impl OpcodeHighlighter {
    /// Create a new opcode syntax highlighter
    pub fn new() -> Self {
        Self { patterns: get_opcode_patterns() }
    }

    /// Tokenize EVM opcode assembly
    pub fn tokenize(&self, line: &str) -> Vec<SyntaxToken> {
        let mut tokens = Vec::new();

        // Process in order of precedence to avoid conflicts
        let patterns = [
            (&self.patterns.comments, TokenType::Comment), // Comments first
            (&self.patterns.push_opcodes, TokenType::Opcode), // PUSH opcodes
            (&self.patterns.dup_swap_opcodes, TokenType::Opcode), // DUP/SWAP opcodes
            (&self.patterns.log_opcodes, TokenType::Opcode), // LOG opcodes
            (&self.patterns.opcodes, TokenType::Opcode),   // All other opcodes
            (&self.patterns.hex_addresses, TokenType::OpcodeAddress), // Hex addresses (0x...)
            (&self.patterns.hex_data, TokenType::OpcodeData), // Hex data
            (&self.patterns.stack_positions, TokenType::OpcodeNumber), /* Stack positions [0],
                                                            * [1], etc. */
            (&self.patterns.numbers, TokenType::OpcodeNumber), // Decimal numbers
        ];

        let mut covered_ranges = Vec::new();

        // Apply patterns in priority order
        for (pattern, token_type) in patterns {
            for mat in pattern.find_iter(line) {
                let start = mat.start();
                let end = mat.end();

                // Check if this range overlaps with any existing range
                let overlaps = covered_ranges.iter().any(|(s, e)| start < *e && end > *s);

                if !overlaps {
                    tokens.push(SyntaxToken { start, end, token_type });
                    covered_ranges.push((start, end));
                }
            }
        }

        // Sort tokens by position
        tokens.sort_by_key(|t| t.start);
        tokens
    }
}

/// Get opcode syntax patterns (cached)
fn get_opcode_patterns() -> &'static OpcodePatterns {
    static PATTERNS: OnceLock<OpcodePatterns> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        OpcodePatterns {
            // Standard EVM opcodes - comprehensive list
            opcodes: Regex::new(r"\b(STOP|ADD|MUL|SUB|DIV|SDIV|MOD|SMOD|ADDMOD|MULMOD|EXP|SIGNEXTEND|LT|GT|SLT|SGT|EQ|ISZERO|AND|OR|XOR|NOT|BYTE|SHL|SHR|SAR|KECCAK256|SHA3|ADDRESS|BALANCE|ORIGIN|CALLER|CALLVALUE|CALLDATALOAD|CALLDATASIZE|CALLDATACOPY|CODESIZE|CODECOPY|GASPRICE|EXTCODESIZE|EXTCODECOPY|RETURNDATASIZE|RETURNDATACOPY|EXTCODEHASH|BLOCKHASH|COINBASE|TIMESTAMP|NUMBER|DIFFICULTY|GASLIMIT|CHAINID|SELFBALANCE|BASEFEE|POP|MLOAD|MSTORE|MSTORE8|SLOAD|SSTORE|JUMP|JUMPI|PC|MSIZE|GAS|JUMPDEST|CREATE|CALL|CALLCODE|RETURN|DELEGATECALL|CREATE2|STATICCALL|REVERT|INVALID|SELFDESTRUCT|SUICIDE)\b").unwrap(),

            // PUSH opcodes with numbers (PUSH0, PUSH1, PUSH2, ..., PUSH32)
            push_opcodes: Regex::new(r"\bPUSH(?:0|1[0-9]|2[0-9]|3[0-2]|[1-9])\b").unwrap(),

            // DUP and SWAP opcodes with numbers (DUP1-DUP16, SWAP1-SWAP16)
            dup_swap_opcodes: Regex::new(r"\b(?:DUP|SWAP)(?:1[0-6]|[1-9])\b").unwrap(),

            // LOG opcodes (LOG0, LOG1, LOG2, LOG3, LOG4)
            log_opcodes: Regex::new(r"\bLOG[0-4]\b").unwrap(),

            // Decimal numbers
            numbers: Regex::new(r"\b\d+\b").unwrap(),

            // Hexadecimal addresses and data (0x followed by hex digits)
            hex_addresses: Regex::new(r"\b0x[0-9a-fA-F]{20}\b").unwrap(),

            // Hex data (sequences of hex digits, at least 2 chars)
            hex_data: Regex::new(r"\b0x[0-9a-fA-F]{2,}\b").unwrap(),

            // Stack positions like [0], [1], etc.
            stack_positions: Regex::new(r"\[\d+\]").unwrap(),

            // Comments in assembly (typically ; or // style)
            comments: Regex::new(r";.*$|//.*$").unwrap(),
        }
    })
}

impl Default for OpcodeHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_opcodes() {
        let highlighter = OpcodeHighlighter::new();
        let tokens = highlighter.tokenize("ADD MUL STOP");

        let opcode_tokens: Vec<_> =
            tokens.iter().filter(|t| t.token_type == TokenType::Opcode).collect();
        assert_eq!(opcode_tokens.len(), 3);
    }

    #[test]
    fn test_push_opcodes() {
        let highlighter = OpcodeHighlighter::new();
        let tokens = highlighter.tokenize("PUSH1 0x80");

        let push_token = tokens.iter().find(|t| t.token_type == TokenType::Opcode);
        assert!(push_token.is_some());

        let hex_token = tokens.iter().find(|t| t.token_type == TokenType::OpcodeData);
        assert!(hex_token.is_some());
    }

    #[test]
    fn test_dup_swap_opcodes() {
        let highlighter = OpcodeHighlighter::new();
        let tokens = highlighter.tokenize("DUP1 SWAP2");

        let opcode_tokens: Vec<_> =
            tokens.iter().filter(|t| t.token_type == TokenType::Opcode).collect();
        assert_eq!(opcode_tokens.len(), 2);
    }

    #[test]
    fn test_hex_data() {
        let highlighter = OpcodeHighlighter::new();
        let tokens = highlighter
            .tokenize("PUSH32 0x0000000000000000000000000000000000000000000000000000000000000080");

        let address_token = tokens.iter().find(|t| t.token_type == TokenType::OpcodeData);
        assert!(address_token.is_some());
    }

    #[test]
    fn test_numbers() {
        let highlighter = OpcodeHighlighter::new();
        let tokens = highlighter.tokenize("PUSH1 128");

        let number_token = tokens.iter().find(|t| t.token_type == TokenType::OpcodeNumber);
        assert!(number_token.is_some());
    }

    #[test]
    fn test_stack_positions() {
        let highlighter = OpcodeHighlighter::new();
        let tokens = highlighter.tokenize("[0] ADD [1]");

        let stack_tokens: Vec<_> =
            tokens.iter().filter(|t| t.token_type == TokenType::OpcodeNumber).collect();
        assert_eq!(stack_tokens.len(), 2);
    }

    #[test]
    fn test_comments() {
        let highlighter = OpcodeHighlighter::new();

        // Semicolon comment
        let tokens = highlighter.tokenize("ADD ; This adds two values");
        let comment_token = tokens.iter().find(|t| t.token_type == TokenType::Comment);
        assert!(comment_token.is_some());

        // Double slash comment
        let tokens = highlighter.tokenize("MUL // Multiply two values");
        let comment_token = tokens.iter().find(|t| t.token_type == TokenType::Comment);
        assert!(comment_token.is_some());
    }

    #[test]
    fn test_complex_opcode_line() {
        let highlighter = OpcodeHighlighter::new();
        let tokens = highlighter.tokenize("PUSH1 0x80 PUSH1 0x40 MSTORE ; Initialize memory");

        // Should have opcodes, addresses, and comments
        let has_opcode = tokens.iter().any(|t| t.token_type == TokenType::Opcode);
        let has_address = tokens.iter().any(|t| t.token_type == TokenType::OpcodeAddress);
        let has_comment = tokens.iter().any(|t| t.token_type == TokenType::Comment);

        assert!(has_opcode);
        assert!(!has_address);
        assert!(has_comment);
    }

    #[test]
    fn test_log_opcodes() {
        let highlighter = OpcodeHighlighter::new();
        let tokens = highlighter.tokenize("LOG0 LOG1 LOG2 LOG3 LOG4");

        let log_tokens: Vec<_> =
            tokens.iter().filter(|t| t.token_type == TokenType::Opcode).collect();
        assert_eq!(log_tokens.len(), 5);
    }
}
