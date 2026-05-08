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

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use alloy_primitives::{Address, Bytes, U256};
use derive_more::From;
use revm::state::TransientStorage;
use serde::{Deserialize, Serialize};

use crate::types::{EdbSolValue, ExecutionFrameId};

/// Complete snapshot information capturing EVM state at a specific execution point for debugging
/// navigation
#[derive(Debug, Clone, Serialize, Deserialize, From)]
pub struct SnapshotInfo {
    /// Unique snapshot identifier for debugging navigation
    pub id: usize,
    /// Execution frame this snapshot belongs to
    pub frame_id: ExecutionFrameId,
    /// Identifier of the next snapshot in execution order for forward navigation
    pub next_id: usize,
    /// Identifier of the previous snapshot in execution order for backward navigation
    pub prev_id: usize,
    /// Detailed snapshot information varying by debugging mode (opcode vs source)
    pub detail: SnapshotInfoDetail,
    /// Target contract address for this execution step
    pub target_address: Address,
    /// Address where the actual bytecode is stored (may differ from target in proxy patterns)
    pub bytecode_address: Address,
}

/// Snapshot detail information varying by debugging mode for different levels of analysis
#[derive(Debug, Clone, Serialize, Deserialize, From)]
pub enum SnapshotInfoDetail {
    /// Low-level opcode debugging with EVM state (program counter, stack, memory)
    Opcode(#[from] OpcodeSnapshotInfoDetail),
    /// High-level source debugging with variable state and source location mapping
    Hook(#[from] HookSnapshotInfoDetail),
}

impl SnapshotInfo {
    /// Get the execution frame identifier this snapshot belongs to
    pub fn frame_id(&self) -> ExecutionFrameId {
        self.frame_id
    }

    /// Get the unique snapshot identifier for navigation purposes
    pub fn id(&self) -> usize {
        self.id
    }

    /// Get the identifier of the next snapshot for forward debugging navigation
    pub fn next_id(&self) -> usize {
        self.next_id
    }

    /// Get the identifier of the previous snapshot for backward debugging navigation
    pub fn prev_id(&self) -> usize {
        self.prev_id
    }

    /// Get the detailed snapshot information based on debugging mode
    pub fn detail(&self) -> &SnapshotInfoDetail {
        &self.detail
    }

    /// Get the source file path for source-level debugging, None for opcode debugging
    pub fn path(&self) -> Option<&PathBuf> {
        match self.detail() {
            SnapshotInfoDetail::Opcode(_) => None,
            SnapshotInfoDetail::Hook(info) => Some(&info.path),
        }
    }

    /// Get the source file offset for source-level debugging, None for opcode debugging
    pub fn offset(&self) -> Option<usize> {
        match self.detail() {
            SnapshotInfoDetail::Opcode(_) => None,
            SnapshotInfoDetail::Hook(info) => Some(info.offset),
        }
    }

    /// Get the program counter for opcode-level debugging, None for source debugging
    pub fn pc(&self) -> Option<usize> {
        match self.detail() {
            SnapshotInfoDetail::Opcode(info) => Some(info.pc),
            SnapshotInfoDetail::Hook(_) => None,
        }
    }

    /// Get local variables for source-level debugging, None for opcode debugging
    pub fn locals(&self) -> Option<&HashMap<String, Option<Arc<EdbSolValue>>>> {
        match self.detail() {
            SnapshotInfoDetail::Opcode(_) => None,
            SnapshotInfoDetail::Hook(info) => Some(&info.locals),
        }
    }
}

/// Source-level debugging snapshot with variable states and source location mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookSnapshotInfoDetail {
    /// Unique snapshot identifier for debugging navigation
    pub id: usize,
    /// Execution frame identifier this snapshot belongs to
    pub frame_id: ExecutionFrameId,
    /// Source file path where execution is currently positioned
    pub path: PathBuf,
    /// Character offset within the source file for precise location tracking
    pub offset: usize,
    /// Length of the current source code segment being executed
    pub length: usize,
    /// Local variables and their current values in this execution context
    pub locals: HashMap<String, Option<Arc<EdbSolValue>>>,
    /// Contract state variables and their values at the current bytecode address
    pub state_variables: HashMap<String, Option<Arc<EdbSolValue>>>,
}

/// Low-level opcode debugging snapshot with complete EVM state for instruction-level analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpcodeSnapshotInfoDetail {
    /// Unique snapshot identifier for debugging navigation
    pub id: usize,
    /// Execution frame identifier this snapshot belongs to
    pub frame_id: ExecutionFrameId,
    /// Program counter indicating current bytecode instruction offset
    pub pc: usize,
    /// Current opcode byte value being executed
    pub opcode: u8,
    /// EVM memory state at this execution point (shared via Arc when unchanged for efficiency)
    pub memory: Vec<u8>,
    /// EVM stack state with all values (always cloned as most opcodes modify it)
    pub stack: Vec<U256>,
    /// Call data for this execution context (shared via Arc within same call frame)
    pub calldata: Bytes,
    /// Transient storage state for EIP-1153 temporary storage operations
    #[serde(with = "transient_string_map")]
    pub transient_storage: TransientStorage,
}

/// Custom serialization module for transient storage
/// Converts HashMap<(Address, U256), U256> to HashMap<String, U256> for JSON serialization
pub mod transient_string_map {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    /// Serialize the given transient storage
    pub fn serialize<S>(storage: &TransientStorage, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let string_map: HashMap<String, U256> = storage
            .iter()
            .map(|((addr, key), value)| {
                let key_string = format!("{addr}:{key}");
                (key_string, *value)
            })
            .collect();
        string_map.serialize(serializer)
    }

    /// Deserialize the given transient storage
    pub fn deserialize<'de, D>(deserializer: D) -> Result<TransientStorage, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string_map: HashMap<String, U256> = HashMap::deserialize(deserializer)?;
        let mut transient_storage = TransientStorage::default();

        for (key_string, value) in string_map {
            let parts: Vec<&str> = key_string.split(':').collect();
            if parts.len() != 2 {
                return Err(serde::de::Error::custom(format!(
                    "Invalid transient storage key format: {key_string}",
                )));
            }

            let addr: Address = parts[0].parse().map_err(|e| {
                serde::de::Error::custom(format!("Invalid address in key {key_string}: {e}"))
            })?;

            let key: U256 = parts[1].parse().map_err(|e| {
                serde::de::Error::custom(format!("Invalid U256 in key {key_string}: {e}"))
            })?;

            transient_storage.insert((addr, key), value);
        }

        Ok(transient_storage)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use alloy_primitives::{address, uint};

        #[test]
        fn test_transient_storage_serialization_empty() {
            let storage = TransientStorage::default();

            // Test serialization
            let mut writer = Vec::new();
            let mut serializer = serde_json::Serializer::new(&mut writer);
            serialize(&storage, &mut serializer).unwrap();
            let serialized_str = String::from_utf8(writer).unwrap();
            assert_eq!(serialized_str, "{}");

            // Test deserialization
            let mut deserializer = serde_json::Deserializer::from_str(&serialized_str);
            let deserialized: TransientStorage = deserialize(&mut deserializer).unwrap();
            assert_eq!(deserialized.len(), 0);
        }

        #[test]
        fn test_transient_storage_serialization_with_data() {
            let mut storage = TransientStorage::default();
            let addr1 = address!("0000000000000000000000000000000000000001");
            let addr2 = address!("0000000000000000000000000000000000000002");
            let key1 = uint!(100_U256);
            let key2 = uint!(200_U256);
            let value1 = uint!(1000_U256);
            let value2 = uint!(2000_U256);

            storage.insert((addr1, key1), value1);
            storage.insert((addr2, key2), value2);

            // Test serialization
            let mut writer = Vec::new();
            let mut serializer = serde_json::Serializer::new(&mut writer);
            serialize(&storage, &mut serializer).unwrap();
            let serialized_str = String::from_utf8(writer).unwrap();

            // Check that serialization produces expected format
            let json: serde_json::Value = serde_json::from_str(&serialized_str).unwrap();
            assert!(json.is_object());
            let obj = json.as_object().unwrap();
            assert_eq!(obj.len(), 2);

            // Check specific key-value pairs
            let key1_str = format!("{addr1}:{key1}");
            let key2_str = format!("{addr2}:{key2}");
            assert_eq!(obj.get(&key1_str).unwrap(), &serde_json::to_value(value1).unwrap());
            assert_eq!(obj.get(&key2_str).unwrap(), &serde_json::to_value(value2).unwrap());

            // Test deserialization
            let mut deserializer = serde_json::Deserializer::from_str(&serialized_str);
            let deserialized: TransientStorage = deserialize(&mut deserializer).unwrap();
            assert_eq!(deserialized.len(), 2);
            assert_eq!(deserialized.get(&(addr1, key1)), Some(&value1));
            assert_eq!(deserialized.get(&(addr2, key2)), Some(&value2));
        }

        #[test]
        fn test_transient_storage_roundtrip() {
            let mut original = TransientStorage::default();

            // Add multiple entries with various addresses and keys
            let entries = vec![
                (
                    address!("1111111111111111111111111111111111111111"),
                    uint!(0_U256),
                    uint!(42_U256),
                ),
                (
                    address!("2222222222222222222222222222222222222222"),
                    uint!(1_U256),
                    uint!(84_U256),
                ),
                (
                    address!("3333333333333333333333333333333333333333"),
                    uint!(999999_U256),
                    uint!(168_U256),
                ),
                (
                    address!("1111111111111111111111111111111111111111"),
                    uint!(1_U256),
                    uint!(336_U256),
                ), // Same address, different key
            ];

            for (addr, key, value) in &entries {
                original.insert((*addr, *key), *value);
            }

            // Test roundtrip serialization
            let mut writer = Vec::new();
            let mut serializer = serde_json::Serializer::new(&mut writer);
            serialize(&original, &mut serializer).unwrap();
            let serialized_str = String::from_utf8(writer).unwrap();

            let mut deserializer = serde_json::Deserializer::from_str(&serialized_str);
            let deserialized: TransientStorage = deserialize(&mut deserializer).unwrap();

            // Verify all entries are preserved
            assert_eq!(deserialized.len(), entries.len());
            for (addr, key, value) in &entries {
                assert_eq!(deserialized.get(&(*addr, *key)), Some(value));
            }
        }

        #[test]
        fn test_transient_storage_invalid_format() {
            // Test invalid key format (missing colon)
            let invalid_json = r#"{"invalid_key": "1000"}"#;
            let mut deserializer = serde_json::Deserializer::from_str(invalid_json);
            let result: Result<TransientStorage, _> = deserialize(&mut deserializer);
            assert!(result.is_err());
            assert!(
                result.unwrap_err().to_string().contains("Invalid transient storage key format")
            );

            // Test invalid address format
            let invalid_json = r#"{"not_an_address:100": "1000"}"#;
            let mut deserializer = serde_json::Deserializer::from_str(invalid_json);
            let result: Result<TransientStorage, _> = deserialize(&mut deserializer);
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("Invalid address"));

            // Test invalid U256 format
            let invalid_json =
                r#"{"0x0000000000000000000000000000000000000001:not_a_number": "1000"}"#;
            let mut deserializer = serde_json::Deserializer::from_str(invalid_json);
            let result: Result<TransientStorage, _> = deserialize(&mut deserializer);
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("Invalid U256"));
        }

        #[test]
        fn test_transient_storage_large_values() {
            let mut storage = TransientStorage::default();
            let addr = address!("ffffffffffffffffffffffffffffffffffffffff");
            let key = U256::MAX;
            let value = U256::MAX;

            storage.insert((addr, key), value);

            // Test roundtrip with large values
            let mut writer = Vec::new();
            let mut serializer = serde_json::Serializer::new(&mut writer);
            serialize(&storage, &mut serializer).unwrap();
            let serialized_str = String::from_utf8(writer).unwrap();

            let mut deserializer = serde_json::Deserializer::from_str(&serialized_str);
            let deserialized: TransientStorage = deserialize(&mut deserializer).unwrap();

            assert_eq!(deserialized.get(&(addr, key)), Some(&value));
        }
    }
}

/// Custom serialization module for Arc<TransientStorage>
///
/// This module provides specialized (de)serialization for `Arc<TransientStorage>`.
/// It reuses the underlying `transient_string_map` serialization logic but wraps
/// the deserialized result in an Arc.
pub mod arc_transient_string_map {
    use super::*;
    use serde::{Deserializer, Serializer};

    /// Serialize Arc<TransientStorage> by delegating to the base implementation
    pub fn serialize<S>(storage: &Arc<TransientStorage>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        transient_string_map::serialize(storage, serializer)
    }

    /// Deserialize into Arc<TransientStorage>
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<TransientStorage>, D::Error>
    where
        D: Deserializer<'de>,
    {
        transient_string_map::deserialize(deserializer).map(Arc::new)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use alloy_primitives::{address, uint};

        #[test]
        fn test_arc_transient_storage_serialization_empty() {
            let storage = Arc::new(TransientStorage::default());

            // Test serialization
            let mut writer = Vec::new();
            let mut serializer = serde_json::Serializer::new(&mut writer);
            transient_string_map::serialize(&storage, &mut serializer).unwrap();
            let serialized_str = String::from_utf8(writer).unwrap();
            assert_eq!(serialized_str, "{}");

            // Test deserialization
            let mut deserializer = serde_json::Deserializer::from_str(&serialized_str);
            let deserialized: Arc<TransientStorage> = deserialize(&mut deserializer).unwrap();
            assert_eq!(deserialized.len(), 0);
        }

        #[test]
        fn test_arc_transient_storage_roundtrip() {
            let mut original = TransientStorage::default();
            let addr1 = address!("1111111111111111111111111111111111111111");
            let addr2 = address!("2222222222222222222222222222222222222222");
            let key1 = uint!(100_U256);
            let key2 = uint!(200_U256);
            let value1 = uint!(1000_U256);
            let value2 = uint!(2000_U256);

            original.insert((addr1, key1), value1);
            original.insert((addr2, key2), value2);
            let storage = Arc::new(original);

            // Test roundtrip serialization
            let mut writer = Vec::new();
            let mut serializer = serde_json::Serializer::new(&mut writer);
            serialize(&storage, &mut serializer).unwrap();
            let serialized_str = String::from_utf8(writer).unwrap();

            let mut deserializer = serde_json::Deserializer::from_str(&serialized_str);
            let deserialized: Arc<TransientStorage> = deserialize(&mut deserializer).unwrap();

            // Verify all entries are preserved
            assert_eq!(deserialized.len(), 2);
            assert_eq!(deserialized.get(&(addr1, key1)), Some(&value1));
            assert_eq!(deserialized.get(&(addr2, key2)), Some(&value2));
        }
    }
}
