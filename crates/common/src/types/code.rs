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

use std::{collections::HashMap, path::PathBuf};

use alloy_primitives::Address;
use derive_more::From;
use serde::{Deserialize, Serialize};

/// Represents code information in either opcode or source format for debugging analysis
#[derive(Debug, Clone, Serialize, Deserialize, From)]
pub enum Code {
    /// Opcode-level code representation with disassembled bytecode
    Opcode(#[from] OpcodeInfo),
    /// Source-level code representation with original Solidity source files
    Source(#[from] SourceInfo),
}

impl Code {
    /// Returns the bytecode address, which may differ from the contract address in proxy patterns
    pub fn bytecode_address(&self) -> Address {
        match self {
            Self::Opcode(info) => info.bytecode_address,
            Self::Source(info) => info.bytecode_address,
        }
    }
}

/// Information about disassembled bytecode with opcode mappings for debugging at the EVM level
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpcodeInfo {
    /// The address where the actual bytecode is stored (may differ from address in proxy patterns)
    pub bytecode_address: Address,
    /// Mapping from program counter to disassembled opcode strings for step-by-step debugging
    pub codes: HashMap<usize, String>, // pc -> opcode
}

/// Information about original Solidity source code for high-level debugging with source mappings
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceInfo {
    /// The address where the actual bytecode is stored (may differ from address in proxy patterns)
    pub bytecode_address: Address,
    /// Mapping from source file paths to their content for source-level debugging
    pub sources: HashMap<PathBuf, String>, // file -> source
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn test_opcode_info_default() {
        let info = OpcodeInfo::default();
        assert_eq!(info.bytecode_address, Address::ZERO);
        assert!(info.codes.is_empty());
    }

    #[test]
    fn test_opcode_info_with_codes() {
        let mut info = OpcodeInfo {
            bytecode_address: address!("1234567890123456789012345678901234567890"),
            codes: HashMap::new(),
        };
        info.codes.insert(0, "PUSH1 0x60".to_string());
        info.codes.insert(2, "PUSH1 0x40".to_string());
        info.codes.insert(4, "MSTORE".to_string());

        assert_eq!(info.bytecode_address, address!("1234567890123456789012345678901234567890"));
        assert_eq!(info.codes.len(), 3);
        assert_eq!(info.codes.get(&0), Some(&"PUSH1 0x60".to_string()));
        assert_eq!(info.codes.get(&2), Some(&"PUSH1 0x40".to_string()));
        assert_eq!(info.codes.get(&4), Some(&"MSTORE".to_string()));
    }

    #[test]
    fn test_source_info_default() {
        let info = SourceInfo::default();
        assert_eq!(info.bytecode_address, Address::ZERO);
        assert!(info.sources.is_empty());
    }

    #[test]
    fn test_source_info_with_sources() {
        let mut info = SourceInfo {
            bytecode_address: address!("abcdefabcdefabcdefabcdefabcdefabcdefabcd"),
            ..Default::default()
        };
        info.sources.insert(
            PathBuf::from("contracts/Token.sol"),
            "pragma solidity ^0.8.0;\n\ncontract Token {}".to_string(),
        );
        info.sources.insert(
            PathBuf::from("contracts/Proxy.sol"),
            "pragma solidity ^0.8.0;\n\ncontract Proxy {}".to_string(),
        );

        assert_eq!(info.bytecode_address, address!("abcdefabcdefabcdefabcdefabcdefabcdefabcd"));
        assert_eq!(info.sources.len(), 2);
        assert!(info.sources.contains_key(&PathBuf::from("contracts/Token.sol")));
        assert!(info.sources.contains_key(&PathBuf::from("contracts/Proxy.sol")));
    }

    #[test]
    fn test_code_from_opcode_info() {
        let mut opcode_info = OpcodeInfo {
            bytecode_address: address!("1111111111111111111111111111111111111111"),
            ..Default::default()
        };
        opcode_info.codes.insert(0, "PUSH1 0x01".to_string());

        let code = Code::from(opcode_info.clone());
        match code {
            Code::Opcode(info) => {
                assert_eq!(
                    info.bytecode_address,
                    address!("1111111111111111111111111111111111111111")
                );
                assert_eq!(info.codes.get(&0), Some(&"PUSH1 0x01".to_string()));
            }
            Code::Source(_) => panic!("Expected Code::Opcode"),
        }
    }

    #[test]
    fn test_code_from_source_info() {
        let mut source_info = SourceInfo {
            bytecode_address: address!("2222222222222222222222222222222222222222"),
            ..Default::default()
        };
        source_info.sources.insert(PathBuf::from("test.sol"), "contract Test {}".to_string());

        let code = Code::from(source_info.clone());
        match code {
            Code::Source(info) => {
                assert_eq!(
                    info.bytecode_address,
                    address!("2222222222222222222222222222222222222222")
                );
                assert!(info.sources.contains_key(&PathBuf::from("test.sol")));
            }
            Code::Opcode(_) => panic!("Expected Code::Source"),
        }
    }

    #[test]
    fn test_code_bytecode_address_opcode() {
        let opcode_info = OpcodeInfo {
            bytecode_address: address!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            ..Default::default()
        };

        let code = Code::Opcode(opcode_info);
        assert_eq!(code.bytecode_address(), address!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
    }

    #[test]
    fn test_code_bytecode_address_source() {
        let source_info = SourceInfo {
            bytecode_address: address!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            ..Default::default()
        };

        let code = Code::Source(source_info);
        assert_eq!(code.bytecode_address(), address!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
    }

    #[test]
    fn test_opcode_info_serialization() {
        let mut info = OpcodeInfo {
            bytecode_address: address!("1234567890123456789012345678901234567890"),
            ..Default::default()
        };
        info.codes.insert(0, "PUSH1 0x60".to_string());
        info.codes.insert(2, "PUSH1 0x40".to_string());

        let json = serde_json::to_string(&info).expect("Failed to serialize OpcodeInfo");
        let deserialized: OpcodeInfo =
            serde_json::from_str(&json).expect("Failed to deserialize OpcodeInfo");

        assert_eq!(deserialized.bytecode_address, info.bytecode_address);
        assert_eq!(deserialized.codes, info.codes);
    }

    #[test]
    fn test_source_info_serialization() {
        let mut info = SourceInfo {
            bytecode_address: address!("abcdefabcdefabcdefabcdefabcdefabcdefabcd"),
            ..Default::default()
        };
        info.sources
            .insert(PathBuf::from("contracts/Token.sol"), "pragma solidity ^0.8.0;".to_string());

        let json = serde_json::to_string(&info).expect("Failed to serialize SourceInfo");
        let deserialized: SourceInfo =
            serde_json::from_str(&json).expect("Failed to deserialize SourceInfo");

        assert_eq!(deserialized.bytecode_address, info.bytecode_address);
        assert_eq!(deserialized.sources, info.sources);
    }

    #[test]
    fn test_code_opcode_serialization() {
        let mut opcode_info = OpcodeInfo {
            bytecode_address: address!("1111111111111111111111111111111111111111"),
            ..Default::default()
        };
        opcode_info.codes.insert(0, "PUSH1 0x01".to_string());

        let code = Code::Opcode(opcode_info);
        let json = serde_json::to_string(&code).expect("Failed to serialize Code::Opcode");
        let deserialized: Code =
            serde_json::from_str(&json).expect("Failed to deserialize Code::Opcode");

        match deserialized {
            Code::Opcode(info) => {
                assert_eq!(
                    info.bytecode_address,
                    address!("1111111111111111111111111111111111111111")
                );
                assert_eq!(info.codes.get(&0), Some(&"PUSH1 0x01".to_string()));
            }
            Code::Source(_) => panic!("Expected Code::Opcode after deserialization"),
        }
    }

    #[test]
    fn test_code_source_serialization() {
        let mut source_info = SourceInfo {
            bytecode_address: address!("2222222222222222222222222222222222222222"),
            ..Default::default()
        };
        source_info.sources.insert(PathBuf::from("test.sol"), "contract Test {}".to_string());

        let code = Code::Source(source_info);
        let json = serde_json::to_string(&code).expect("Failed to serialize Code::Source");
        let deserialized: Code =
            serde_json::from_str(&json).expect("Failed to deserialize Code::Source");

        match deserialized {
            Code::Source(info) => {
                assert_eq!(
                    info.bytecode_address,
                    address!("2222222222222222222222222222222222222222")
                );
                assert_eq!(
                    info.sources.get(&PathBuf::from("test.sol")),
                    Some(&"contract Test {}".to_string())
                );
            }
            Code::Opcode(_) => panic!("Expected Code::Source after deserialization"),
        }
    }

    #[test]
    fn test_code_empty_serialization() {
        let opcode_code = Code::Opcode(OpcodeInfo::default());
        let source_code = Code::Source(SourceInfo::default());

        let opcode_json =
            serde_json::to_string(&opcode_code).expect("Failed to serialize empty Code::Opcode");
        let source_json =
            serde_json::to_string(&source_code).expect("Failed to serialize empty Code::Source");

        let opcode_deserialized: Code =
            serde_json::from_str(&opcode_json).expect("Failed to deserialize empty Code::Opcode");
        let source_deserialized: Code =
            serde_json::from_str(&source_json).expect("Failed to deserialize empty Code::Source");

        assert_eq!(opcode_deserialized.bytecode_address(), Address::ZERO);
        assert_eq!(source_deserialized.bytecode_address(), Address::ZERO);
    }

    #[test]
    fn test_pathbuf_serialization_with_special_chars() {
        let mut info = SourceInfo::default();
        info.sources.insert(
            PathBuf::from("contracts/Token with spaces.sol"),
            "pragma solidity ^0.8.0;".to_string(),
        );
        info.sources
            .insert(PathBuf::from("contracts/üñîçødé.sol"), "pragma solidity ^0.8.0;".to_string());
        info.sources
            .insert(PathBuf::from("contracts/../Token.sol"), "pragma solidity ^0.8.0;".to_string());

        let json = serde_json::to_string(&info)
            .expect("Failed to serialize SourceInfo with special paths");
        let deserialized: SourceInfo = serde_json::from_str(&json)
            .expect("Failed to deserialize SourceInfo with special paths");

        assert_eq!(deserialized.sources.len(), 3);
        assert!(
            deserialized.sources.contains_key(&PathBuf::from("contracts/Token with spaces.sol"))
        );
        assert!(deserialized.sources.contains_key(&PathBuf::from("contracts/üñîçødé.sol")));
        assert!(deserialized.sources.contains_key(&PathBuf::from("contracts/../Token.sol")));
    }

    #[test]
    fn test_large_data_serialization() {
        let mut info = OpcodeInfo {
            bytecode_address: address!("ffffffffffffffffffffffffffffffffffffffff"),
            ..Default::default()
        };

        // Add many opcode entries
        for i in 0..1000 {
            info.codes.insert(i * 2, format!("OPCODE_{i}"));
        }

        let json = serde_json::to_string(&info).expect("Failed to serialize large OpcodeInfo");
        let deserialized: OpcodeInfo =
            serde_json::from_str(&json).expect("Failed to deserialize large OpcodeInfo");

        assert_eq!(deserialized.bytecode_address, info.bytecode_address);
        assert_eq!(deserialized.codes.len(), 1000);
        assert_eq!(deserialized.codes.get(&0), Some(&"OPCODE_0".to_string()));
        assert_eq!(deserialized.codes.get(&1998), Some(&"OPCODE_999".to_string()));
    }

    #[test]
    fn test_source_info_large_content_serialization() {
        let mut info = SourceInfo {
            bytecode_address: address!("ffffffffffffffffffffffffffffffffffffffff"),
            ..Default::default()
        };

        let large_content = "// ".repeat(10000) + &"contract Test {}".repeat(100);
        info.sources.insert(PathBuf::from("large.sol"), large_content.clone());

        let json = serde_json::to_string(&info)
            .expect("Failed to serialize SourceInfo with large content");
        let deserialized: SourceInfo = serde_json::from_str(&json)
            .expect("Failed to deserialize SourceInfo with large content");

        assert_eq!(deserialized.bytecode_address, info.bytecode_address);
        assert_eq!(deserialized.sources.len(), 1);
        assert_eq!(deserialized.sources.get(&PathBuf::from("large.sol")), Some(&large_content));
    }

    #[test]
    fn test_serialization_roundtrip_preserves_order() {
        let mut info = OpcodeInfo::default();
        info.codes.insert(10, "JUMP".to_string());
        info.codes.insert(5, "PUSH1".to_string());
        info.codes.insert(15, "STOP".to_string());

        let json = serde_json::to_string(&info).expect("Failed to serialize OpcodeInfo");
        let deserialized: OpcodeInfo =
            serde_json::from_str(&json).expect("Failed to deserialize OpcodeInfo");

        // HashMap doesn't guarantee order but contents should match
        assert_eq!(deserialized.codes.len(), 3);
        assert_eq!(deserialized.codes.get(&5), Some(&"PUSH1".to_string()));
        assert_eq!(deserialized.codes.get(&10), Some(&"JUMP".to_string()));
        assert_eq!(deserialized.codes.get(&15), Some(&"STOP".to_string()));
    }

    #[test]
    fn test_address_serialization_formats() {
        let addresses = vec![
            Address::ZERO,
            address!("0x1111111111111111111111111111111111111111"),
            address!("0xffffffffffffffffffffffffffffffffffffffff"),
            address!("0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"),
        ];

        for addr in addresses {
            let info = OpcodeInfo { bytecode_address: addr, ..Default::default() };

            let json = serde_json::to_string(&info).expect("Failed to serialize address");
            let deserialized: OpcodeInfo =
                serde_json::from_str(&json).expect("Failed to deserialize address");

            assert_eq!(deserialized.bytecode_address, addr);
        }
    }

    #[test]
    fn test_code_clone() {
        let mut opcode_info = OpcodeInfo {
            bytecode_address: address!("1234567890123456789012345678901234567890"),
            ..Default::default()
        };
        opcode_info.codes.insert(0, "PUSH1 0x60".to_string());

        let code = Code::Opcode(opcode_info);
        let cloned_code = code.clone();

        assert_eq!(code.bytecode_address(), cloned_code.bytecode_address());
        match (&code, &cloned_code) {
            (Code::Opcode(info1), Code::Opcode(info2)) => {
                assert_eq!(info1.codes, info2.codes);
            }
            _ => panic!("Clone should preserve variant type"),
        }
    }

    #[test]
    fn test_empty_pathbuf_in_source_info() {
        let mut info = SourceInfo::default();
        info.sources.insert(PathBuf::new(), "empty path content".to_string());

        assert_eq!(info.sources.len(), 1);
        assert!(info.sources.contains_key(&PathBuf::new()));
    }

    #[test]
    fn test_large_pc_values_in_opcode_info() {
        let mut info = OpcodeInfo::default();
        info.codes.insert(0, "PUSH1".to_string());
        info.codes.insert(1000000, "JUMP".to_string());
        info.codes.insert(usize::MAX, "STOP".to_string());

        assert_eq!(info.codes.len(), 3);
        assert_eq!(info.codes.get(&0), Some(&"PUSH1".to_string()));
        assert_eq!(info.codes.get(&1000000), Some(&"JUMP".to_string()));
        assert_eq!(info.codes.get(&usize::MAX), Some(&"STOP".to_string()));
    }
}
