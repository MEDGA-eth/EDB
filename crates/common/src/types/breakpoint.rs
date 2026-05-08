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

use std::{fmt::Display, path::PathBuf, str::FromStr};

use alloy_primitives::Address;
use eyre::{Error, Result, bail, eyre};
use serde::{Deserialize, Serialize};

use crate::normalize_expression;

/// Represents a breakpoint in the debugger with optional location and condition.
/// A breakpoint can be set at specific code locations and optionally have conditions that must be
/// met to trigger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Breakpoint {
    /// The location where the breakpoint is set (source code or opcode).
    pub loc: Option<BreakpointLocation>,
    /// Optional condition expression that must evaluate to true for the breakpoint to trigger.
    pub condition: Option<String>,
}

impl Display for Breakpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(loc) = &self.loc {
            write!(f, "@{}", loc.display(None))?;
        }
        if let Some(cond) = &self.condition {
            if self.loc.is_some() { write!(f, " if {cond}") } else { write!(f, "if {cond}") }
        } else {
            Ok(())
        }
    }
}

impl FromStr for Breakpoint {
    type Err = Error;

    /// Parses a breakpoint from a string.
    /// Format: `[@<location>] [if <condition>]`
    /// Examples:
    /// - `@0x1234:42` - Breakpoint at opcode
    /// - `@0x1234:src/main.rs:100` - Breakpoint at source location
    /// - `if x > 10` - Data-watching breakpoint
    /// - `@0x1234:42 if balance == 0` - Breakpoint with condition
    fn from_str(s: &str) -> Result<Self> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Ok(Self { loc: None, condition: None });
        }

        let mut loc = None;
        let mut condition = None;

        // Check if string starts with @ (has location)
        if let Some(loc_str) = trimmed.strip_prefix('@') {
            // Find where location ends (either at "if" or end of string)
            if let Some(if_pos) = trimmed.find(" if ") {
                // Has both location and condition
                let loc_str = &loc_str[..if_pos].trim();
                loc = Some(BreakpointLocation::from_str(loc_str)?);
                let condition_str = trimmed[if_pos + 4..].trim();
                if !condition_str.starts_with("$") {
                    bail!("Condition expression does not start with $");
                }
                condition = Some(normalize_expression(condition_str[1..].trim()));
            } else {
                // Only has location
                let loc_str = loc_str.trim();
                loc = Some(BreakpointLocation::from_str(loc_str)?);
            }
        } else if let Some(condition_str) = trimmed.strip_prefix("if ") {
            // Only has condition
            let condition_str = condition_str.trim();
            if !condition_str.starts_with("$") {
                bail!("Condition expression does not start with $");
            }
            condition = Some(normalize_expression(condition_str[1..].trim()));
        } else {
            bail!("Invalid breakpoint format. Expected [@<location>] [if <condition>], got: {s}");
        }

        Ok(Self { loc, condition })
    }
}

/// Specifies the location of a breakpoint, either in source code or at a specific opcode.
/// Breakpoints can be placed at source code lines or at specific program counter positions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum BreakpointLocation {
    /// A breakpoint in source code at a specific file and character range.
    Source {
        /// The address of the bytecode contract.
        bytecode_address: Address,
        /// Path to the source file.
        file_path: PathBuf,
        /// Line number in the source file (1-based).
        line_number: usize,
    },
    /// A breakpoint at a specific opcode position.
    Opcode {
        /// The address of the bytecode contract.
        bytecode_address: Address,
        /// Program counter (PC) position in the bytecode.
        pc: usize,
    },
}

impl FromStr for BreakpointLocation {
    type Err = Error;

    /// Parses a breakpoint location from a string in the format:
    /// - `<addr>:<pc>` for opcode breakpoints
    /// - `<addr>:<path>:<line>` for source breakpoints
    fn from_str(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split(':').collect();

        if parts.len() == 2 {
            // Opcode breakpoint: <addr>:<pc>
            let addr = parts[0].parse::<Address>().map_err(|e| eyre!("Invalid address: {e}"))?;
            let pc = parts[1].parse::<usize>().map_err(|e| eyre!("Invalid PC: {e}"))?;

            Ok(Self::Opcode { bytecode_address: addr, pc })
        } else if parts.len() == 3 {
            // Source breakpoint: <addr>:<path>:<line>
            let addr = parts[0].parse::<Address>().map_err(|e| eyre!("Invalid address: {e}"))?;
            let path = PathBuf::from(parts[1]);
            let line_number =
                parts[2].parse::<usize>().map_err(|e| eyre!("Invalid line number: {e}"))?;

            Ok(Self::Source { bytecode_address: addr, file_path: path, line_number })
        } else {
            bail!(
                "Invalid breakpoint location format. Expected: <addr>:<pc> or <addr>:<path>:<line>"
            )
        }
    }
}

impl BreakpointLocation {
    /// Creates a breakpoint location from an opcode line number.
    /// Returns None if the line number is invalid or out of bounds.
    pub fn from_opcode_line(
        bytecode_address: Address,
        code: &[(u64, String)],
        line_number: usize,
    ) -> Option<Self> {
        let pc = code.get(line_number - 1)?.0 as usize;
        Some(Self::Opcode { bytecode_address, pc })
    }

    /// Returns the bytecode address associated with this breakpoint location.
    /// Works for both Source and Opcode variants.
    pub fn bytecode_address(&self) -> Address {
        match self {
            Self::Source { bytecode_address, .. } => *bytecode_address,
            Self::Opcode { bytecode_address, .. } => *bytecode_address,
        }
    }

    /// Formats the breakpoint location as a string.
    pub fn display(&self, addr_label: Option<String>) -> String {
        let bytecode_address = self.bytecode_address();
        let addr_str = addr_label.unwrap_or_else(|| {
            let full_addr = format!("{bytecode_address}");
            if full_addr.len() > 14 {
                // Create human-readable short format: 0x1234...5678
                format!("{}...{}", &full_addr[..8], &full_addr[full_addr.len() - 6..])
            } else {
                full_addr
            }
        });

        match self {
            Self::Opcode { pc, .. } => {
                format!("{addr_str}:{pc}")
            }
            Self::Source { file_path, line_number, .. } => {
                format!("{addr_str}:{}:{line_number}", file_path.display())
            }
        }
    }
}

impl Breakpoint {
    /// Creates a new breakpoint with the given location and optional condition.
    pub fn new(loc: Option<BreakpointLocation>, condition: Option<String>) -> Self {
        Self { loc, condition }
    }

    /// Update the condition of the breakpoint.
    pub fn set_condition(&mut self, condition: &str) {
        self.condition = Some(normalize_expression(condition));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn test_breakpoint_location_from_str_opcode() {
        // Valid opcode breakpoint
        let loc_str = "0x1234567890123456789012345678901234567890:42";
        let loc = BreakpointLocation::from_str(loc_str).unwrap();

        match loc {
            BreakpointLocation::Opcode { bytecode_address, pc } => {
                assert_eq!(bytecode_address, address!("1234567890123456789012345678901234567890"));
                assert_eq!(pc, 42);
            }
            _ => panic!("Expected Opcode variant"),
        }
    }

    #[test]
    fn test_breakpoint_location_from_str_source() {
        // Valid source breakpoint
        let loc_str = "0x1234567890123456789012345678901234567890:src/main.rs:100";
        let loc = BreakpointLocation::from_str(loc_str).unwrap();

        match loc {
            BreakpointLocation::Source { bytecode_address, file_path, line_number } => {
                assert_eq!(bytecode_address, address!("1234567890123456789012345678901234567890"));
                assert_eq!(file_path, PathBuf::from("src/main.rs"));
                assert_eq!(line_number, 100);
            }
            _ => panic!("Expected Source variant"),
        }
    }

    #[test]
    fn test_breakpoint_location_from_str_invalid() {
        // Invalid format - too few parts
        assert!(BreakpointLocation::from_str("0x1234").is_err());

        // Invalid format - too many parts
        assert!(BreakpointLocation::from_str("0x1234:foo:bar:baz").is_err());

        // Invalid address
        assert!(BreakpointLocation::from_str("invalid_address:42").is_err());

        // Invalid PC
        assert!(
            BreakpointLocation::from_str("0x1234567890123456789012345678901234567890:not_a_number")
                .is_err()
        );

        // Invalid line number
        assert!(
            BreakpointLocation::from_str(
                "0x1234567890123456789012345678901234567890:src/main.rs:not_a_number"
            )
            .is_err()
        );
    }

    #[test]
    fn test_breakpoint_location_to_string() {
        // Test opcode breakpoint formatting
        let loc = BreakpointLocation::Opcode {
            bytecode_address: address!("1234567890123456789012345678901234567890"),
            pc: 42,
        };
        assert_eq!(loc.display(None), "0x123456...567890:42");
        assert_eq!(loc.display(Some("<addr>".to_string())), "<addr>:42");

        // Test source breakpoint formatting
        let loc = BreakpointLocation::Source {
            bytecode_address: address!("1234567890123456789012345678901234567890"),
            file_path: PathBuf::from("src/main.rs"),
            line_number: 100,
        };
        assert_eq!(loc.display(None), "0x123456...567890:src/main.rs:100");
        assert_eq!(loc.display(Some("<addr>".to_string())), "<addr>:src/main.rs:100");
    }

    #[test]
    fn test_breakpoint_location_parsing() {
        // Test that parsing works correctly with full address format
        let original_opcode = "0x1234567890123456789012345678901234567890:42";
        let loc = BreakpointLocation::from_str(original_opcode).unwrap();
        match loc {
            BreakpointLocation::Opcode { bytecode_address, pc } => {
                assert_eq!(bytecode_address, address!("1234567890123456789012345678901234567890"));
                assert_eq!(pc, 42);
            }
            _ => panic!("Expected Opcode variant"),
        }

        let original_source = "0x1234567890123456789012345678901234567890:src/main.rs:100";
        let loc = BreakpointLocation::from_str(original_source).unwrap();
        match loc {
            BreakpointLocation::Source { bytecode_address, file_path, line_number } => {
                assert_eq!(bytecode_address, address!("1234567890123456789012345678901234567890"));
                assert_eq!(file_path, PathBuf::from("src/main.rs"));
                assert_eq!(line_number, 100);
            }
            _ => panic!("Expected Source variant"),
        }
    }

    #[test]
    fn test_breakpoint_from_str_empty() {
        // Empty string - should create breakpoint with no location or condition
        let bp = Breakpoint::from_str("").unwrap();
        assert!(bp.loc.is_none());
        assert!(bp.condition.is_none());

        // Whitespace only
        let bp = Breakpoint::from_str("   ").unwrap();
        assert!(bp.loc.is_none());
        assert!(bp.condition.is_none());
    }

    #[test]
    fn test_breakpoint_from_str_location_only() {
        // Just location, no condition
        let bp = Breakpoint::from_str("@0x1234567890123456789012345678901234567890:42").unwrap();

        assert!(bp.loc.is_some());
        assert!(bp.condition.is_none());

        match bp.loc.unwrap() {
            BreakpointLocation::Opcode { bytecode_address, pc } => {
                assert_eq!(bytecode_address, address!("1234567890123456789012345678901234567890"));
                assert_eq!(pc, 42);
            }
            _ => panic!("Expected Opcode variant"),
        }
    }

    #[test]
    fn test_breakpoint_from_str_condition_only() {
        // Just condition, no location
        let bp = Breakpoint::from_str("if $x > 10").unwrap();

        assert!(bp.loc.is_none());
        assert_eq!(bp.condition, Some("x > 10".to_string()));
    }

    #[test]
    fn test_breakpoint_from_str_location_and_condition() {
        // Both location and condition
        let bp = Breakpoint::from_str(
            "@0x1234567890123456789012345678901234567890:src/main.rs:100 if $ balance == 0",
        )
        .unwrap();

        assert!(bp.loc.is_some());
        assert_eq!(bp.condition, Some("balance == 0".to_string()));

        match bp.loc.unwrap() {
            BreakpointLocation::Source { bytecode_address, file_path, line_number } => {
                assert_eq!(bytecode_address, address!("1234567890123456789012345678901234567890"));
                assert_eq!(file_path, PathBuf::from("src/main.rs"));
                assert_eq!(line_number, 100);
            }
            _ => panic!("Expected Source variant"),
        }
    }

    #[test]
    fn test_breakpoint_from_str_invalid() {
        // Invalid format - no @ or if
        assert!(Breakpoint::from_str("invalid_format").is_err());

        // Invalid location format
        assert!(Breakpoint::from_str("@invalid_location").is_err());
    }

    #[test]
    fn test_breakpoint_from_str_with_spaces() {
        // Test with extra spaces
        let bp = Breakpoint::from_str(
            "  @0x1234567890123456789012345678901234567890:42  if $  x > 10  ",
        )
        .unwrap();

        assert!(bp.loc.is_some());
        assert_eq!(bp.condition, Some("x > 10".to_string()));
    }

    #[test]
    fn test_breakpoint_from_str_complex_conditions() {
        // Test with complex condition expressions
        let bp = Breakpoint::from_str("if $ msg.sender == 0x1234 && balance > 100").unwrap();
        assert!(bp.loc.is_none());
        assert_eq!(bp.condition, Some("msg.sender == 0x1234 && balance > 100".to_string()));

        // With location and complex condition
        let bp = Breakpoint::from_str(
            "@0x1234567890123456789012345678901234567890:100 if $ (x > 10 || y < 5) && z == true",
        )
        .unwrap();
        assert!(bp.loc.is_some());
        assert_eq!(bp.condition, Some("(x > 10 || y < 5) && z == true".to_string()));
    }

    #[test]
    fn test_breakpoint_new() {
        // Test the new constructor
        let loc = BreakpointLocation::Opcode {
            bytecode_address: address!("1234567890123456789012345678901234567890"),
            pc: 42,
        };
        let bp = Breakpoint::new(Some(loc.clone()), Some("test condition".to_string()));

        assert_eq!(bp.loc, Some(loc));
        assert_eq!(bp.condition, Some("test condition".to_string()));
    }

    #[test]
    fn test_breakpoint_location_bytecode_address() {
        let addr = address!("1234567890123456789012345678901234567890");

        // Test for Opcode variant
        let loc = BreakpointLocation::Opcode { bytecode_address: addr, pc: 42 };
        assert_eq!(loc.bytecode_address(), addr);

        // Test for Source variant
        let loc = BreakpointLocation::Source {
            bytecode_address: addr,
            file_path: PathBuf::from("test.rs"),
            line_number: 10,
        };
        assert_eq!(loc.bytecode_address(), addr);
    }

    #[test]
    fn test_breakpoint_equality() {
        // Test that derived PartialEq, Eq, and Hash work correctly
        let bp1 = Breakpoint::new(
            Some(BreakpointLocation::Opcode {
                bytecode_address: address!("1234567890123456789012345678901234567890"),
                pc: 42,
            }),
            Some("condition".to_string()),
        );

        let bp2 = Breakpoint::new(
            Some(BreakpointLocation::Opcode {
                bytecode_address: address!("1234567890123456789012345678901234567890"),
                pc: 42,
            }),
            Some("condition".to_string()),
        );

        let bp3 = Breakpoint::new(
            Some(BreakpointLocation::Opcode {
                bytecode_address: address!("1234567890123456789012345678901234567890"),
                pc: 43, // Different PC
            }),
            Some("condition".to_string()),
        );

        assert_eq!(bp1, bp2);
        assert_ne!(bp1, bp3);

        // Test with HashSet
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(bp1);
        assert!(!set.insert(bp2)); // Should return false as it's a duplicate
        assert!(set.insert(bp3)); // Should return true as it's different
    }
}
