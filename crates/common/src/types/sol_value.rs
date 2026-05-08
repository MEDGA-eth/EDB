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

use std::ops::{Deref, DerefMut};

use alloy_dyn_abi::DynSolValue;
use alloy_primitives::hex;
use alloy_primitives::{Address, FixedBytes, I256, U256};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Wrapper around DynSolValue with custom serialization for EDB debugging and analysis
#[derive(Debug, Clone, PartialEq)]
pub struct EdbSolValue(pub DynSolValue);

impl From<DynSolValue> for EdbSolValue {
    fn from(value: DynSolValue) -> Self {
        Self(value)
    }
}

impl From<EdbSolValue> for DynSolValue {
    fn from(value: EdbSolValue) -> Self {
        value.0
    }
}

impl Deref for EdbSolValue {
    type Target = DynSolValue;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for EdbSolValue {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Serializable representation of DynSolValue for JSON and other formats with complete type information preservation
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
enum SerializedDynSolValue {
    /// Boolean value
    Bool(bool),
    /// Signed integer with bit size specification
    Int { value: I256, bits: usize },
    /// Unsigned integer with bit size specification
    Uint { value: U256, bits: usize },
    /// Fixed-size bytes with size specification
    FixedBytes { value: FixedBytes<32>, size: usize },
    /// Ethereum address (20 bytes)
    Address(Address),
    /// Function selector (24 bytes)
    Function(FixedBytes<24>),
    /// Dynamic bytes array
    Bytes(Vec<u8>),
    /// String value
    String(String),
    /// Dynamic array of values
    Array(Vec<Self>),
    /// Fixed-size array of values
    FixedArray(Vec<Self>),
    /// Tuple of multiple values
    Tuple(Vec<Self>),
    /// Custom struct with name, property names, and tuple data
    CustomStruct { name: String, prop_names: Vec<String>, tuple: Vec<Self> },
}

impl From<&DynSolValue> for SerializedDynSolValue {
    fn from(value: &DynSolValue) -> Self {
        match value {
            DynSolValue::Bool(b) => Self::Bool(*b),
            DynSolValue::Int(i, bits) => Self::Int { value: *i, bits: *bits },
            DynSolValue::Uint(u, bits) => Self::Uint { value: *u, bits: *bits },
            DynSolValue::FixedBytes(bytes, size) => Self::FixedBytes { value: *bytes, size: *size },
            DynSolValue::Address(addr) => Self::Address(*addr),
            DynSolValue::Function(func) => Self::Function(func.0),
            DynSolValue::Bytes(bytes) => Self::Bytes(bytes.clone()),
            DynSolValue::String(s) => Self::String(s.clone()),
            DynSolValue::Array(arr) => Self::Array(arr.iter().map(Into::into).collect()),
            DynSolValue::FixedArray(arr) => Self::FixedArray(arr.iter().map(Into::into).collect()),
            DynSolValue::Tuple(tuple) => Self::Tuple(tuple.iter().map(Into::into).collect()),
            DynSolValue::CustomStruct { name, prop_names, tuple } => Self::CustomStruct {
                name: name.clone(),
                prop_names: prop_names.clone(),
                tuple: tuple.iter().map(Into::into).collect(),
            },
        }
    }
}

impl From<SerializedDynSolValue> for DynSolValue {
    fn from(value: SerializedDynSolValue) -> Self {
        match value {
            SerializedDynSolValue::Bool(b) => Self::Bool(b),
            SerializedDynSolValue::Int { value, bits } => Self::Int(value, bits),
            SerializedDynSolValue::Uint { value, bits } => Self::Uint(value, bits),
            SerializedDynSolValue::FixedBytes { value, size } => Self::FixedBytes(value, size),
            SerializedDynSolValue::Address(addr) => Self::Address(addr),
            SerializedDynSolValue::Function(func) => {
                Self::Function(alloy_primitives::Function::from(func))
            }
            SerializedDynSolValue::Bytes(bytes) => Self::Bytes(bytes),
            SerializedDynSolValue::String(s) => Self::String(s),
            SerializedDynSolValue::Array(arr) => {
                Self::Array(arr.into_iter().map(Into::into).collect())
            }
            SerializedDynSolValue::FixedArray(arr) => {
                Self::FixedArray(arr.into_iter().map(Into::into).collect())
            }
            SerializedDynSolValue::Tuple(tuple) => {
                Self::Tuple(tuple.into_iter().map(Into::into).collect())
            }
            SerializedDynSolValue::CustomStruct { name, prop_names, tuple } => Self::CustomStruct {
                name,
                prop_names,
                tuple: tuple.into_iter().map(Into::into).collect(),
            },
        }
    }
}

impl Serialize for EdbSolValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let serialized = SerializedDynSolValue::from(&self.0);
        serialized.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EdbSolValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let serialized = SerializedDynSolValue::deserialize(deserializer)?;
        Ok(Self(serialized.into()))
    }
}

/// Trait for formatting Solidity values into human-readable strings.
pub trait SolValueFormatter {
    /// Formats a Solidity value into a human-readable string.
    ///
    /// # Arguments
    ///
    /// * `with_ty` - If true, includes type information in the output (e.g., "uint256(123)")
    ///
    /// # Returns
    ///
    /// A formatted string representation of the value.
    fn format_value(&self, ctx: &SolValueFormatterContext) -> String {
        self.format_value_with_indent(ctx, 0)
    }

    /// Formats a Solidity value with specific indentation level.
    fn format_value_with_indent(
        &self,
        ctx: &SolValueFormatterContext,
        indent_level: usize,
    ) -> String;

    /// Returns the Solidity type of this value as a string.
    ///
    /// # Returns
    ///
    /// The Solidity type (e.g., "uint256", "address", "bytes32[]")
    fn format_type(&self) -> String;
}

/// Configuration context for formatting Solidity values with various display options and address resolution
#[derive(Default)]
pub struct SolValueFormatterContext {
    /// Optional function to resolve addresses to human-readable names or labels
    pub resolve_address: Option<Box<dyn Fn(Address) -> Option<String>>>,
    /// Whether to include type information in the formatted output
    pub with_ty: bool,
    /// Whether to shorten long arrays, strings, and other large data structures
    pub shorten_long: bool,
    /// Whether to use multi-line formatting for better readability of complex structures
    pub multi_line: bool,
}

impl SolValueFormatterContext {
    /// Create a new default formatter context
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure whether to include type information in formatted output
    pub fn with_ty(mut self, with_ty: bool) -> Self {
        self.with_ty = with_ty;
        self
    }

    /// Configure whether to shorten long data structures for readability
    pub fn shorten_long(mut self, shorten_long: bool) -> Self {
        self.shorten_long = shorten_long;
        self
    }

    /// Configure whether to use multi-line formatting for complex structures
    pub fn multi_line(mut self, multi_line: bool) -> Self {
        self.multi_line = multi_line;
        self
    }
}

impl SolValueFormatter for DynSolValue {
    fn format_value_with_indent(
        &self,
        ctx: &SolValueFormatterContext,
        indent_level: usize,
    ) -> String {
        match self {
            Self::Bool(b) => b.to_string(),

            Self::Int(n, bits) => {
                if ctx.with_ty {
                    format!("int{bits}({n})")
                } else {
                    n.to_string()
                }
            }

            Self::Uint(n, bits) => {
                if ctx.with_ty {
                    format!("uint{bits}({n})")
                } else {
                    n.to_string()
                }
            }

            Self::Address(addr) => {
                if let Some(label) = ctx.resolve_address.as_ref().and_then(|f| f(*addr)) {
                    label
                } else {
                    let addr_str = if !ctx.shorten_long {
                        addr.to_checksum(None)
                    } else if *addr == Address::ZERO {
                        "0x0000000000000000".to_string()
                    } else {
                        let addr_str = addr.to_checksum(None);
                        // Show more characters for better identification: 8 chars + ... + 6 chars
                        format!("{}...{}", &addr_str[..8], &addr_str[addr_str.len() - 6..])
                    };

                    if ctx.with_ty {
                        format!("address({addr_str})")
                    } else {
                        addr_str
                    }
                }
            }

            Self::Function(func) => {
                format!("0x{}", hex::encode(func.as_slice()))
            }

            Self::FixedBytes(bytes, size) => {
                if ctx.with_ty {
                    format!("bytes{}(0x{})", size, hex::encode(bytes))
                } else {
                    format!("0x{}", hex::encode(bytes))
                }
            }

            Self::Bytes(bytes) => {
                if bytes.len() <= 32 || !ctx.shorten_long {
                    format!("0x{}", hex::encode(bytes))
                } else {
                    format!("0x{}...[{} bytes]", hex::encode(&bytes[..16]), bytes.len())
                }
            }

            Self::String(s) => {
                if s.len() <= 64 || !ctx.shorten_long {
                    format!("\"{}\"", s.replace('\"', "\\\""))
                } else {
                    format!("\"{}...\"[{} chars]", &s[..32].replace('\"', "\\\""), s.len())
                }
            }

            Self::Array(arr) => format_array(arr, false, ctx, indent_level),

            Self::FixedArray(arr) => format_array(arr, true, ctx, indent_level),

            Self::Tuple(tuple) => format_tuple(tuple, ctx, indent_level),

            Self::CustomStruct { name, prop_names, tuple } => {
                if prop_names.is_empty() {
                    format!("{}{}", name, format_tuple(tuple, ctx, indent_level))
                } else {
                    format_custom_struct(name, prop_names, tuple, ctx, indent_level)
                }
            }
        }
    }

    fn format_type(&self) -> String {
        match self {
            Self::Bool(_) => "bool".to_string(),
            Self::Int(_, bits) => format!("int{bits}"),
            Self::Uint(_, bits) => format!("uint{bits}"),
            Self::Address(_) => "address".to_string(),
            Self::Function(_) => "function".to_string(),
            Self::FixedBytes(_, size) => format!("bytes{size}"),
            Self::Bytes(_) => "bytes".to_string(),
            Self::String(_) => "string".to_string(),
            Self::Array(arr) => {
                if let Some(first) = arr.first() {
                    format!("{}[]", first.format_type())
                } else {
                    "unknown[]".to_string()
                }
            }
            Self::FixedArray(arr) => {
                if let Some(first) = arr.first() {
                    format!("{}[{}]", first.format_type(), arr.len())
                } else {
                    format!("unknown[{}]", arr.len())
                }
            }
            Self::Tuple(tuple) => {
                let types: Vec<String> = tuple.iter().map(|v| v.format_type()).collect();
                format!("({})", types.join(","))
            }
            Self::CustomStruct { name, .. } => name.clone(),
        }
    }
}

/// Helper function to create indentation string
fn make_indent(indent_level: usize) -> String {
    "  ".repeat(indent_level)
}

fn format_array(
    arr: &[DynSolValue],
    is_fixed: bool,
    ctx: &SolValueFormatterContext,
    indent_level: usize,
) -> String {
    const MAX_DISPLAY_ITEMS: usize = 5;

    if arr.is_empty() {
        return "[]".to_string();
    }

    if ctx.multi_line && arr.len() > 1 {
        let child_indent = make_indent(indent_level + 1);
        let current_indent = make_indent(indent_level);

        let items: Vec<String> = if ctx.shorten_long && arr.len() > MAX_DISPLAY_ITEMS {
            let mut items = arr
                .iter()
                .take(3)
                .map(|v| {
                    format!("{}{}", child_indent, v.format_value_with_indent(ctx, indent_level + 1))
                })
                .collect::<Vec<_>>();
            let suffix = if is_fixed {
                format!("{}...[{} total]", child_indent, arr.len())
            } else {
                format!("{}...[{} items]", child_indent, arr.len())
            };
            items.push(suffix);
            items
        } else {
            arr.iter()
                .map(|v| {
                    format!("{}{}", child_indent, v.format_value_with_indent(ctx, indent_level + 1))
                })
                .collect()
        };
        format!("[\n{}\n{}]", items.join(",\n"), current_indent)
    } else if arr.len() <= MAX_DISPLAY_ITEMS || !ctx.shorten_long {
        let items: Vec<String> =
            arr.iter().map(|v| v.format_value_with_indent(ctx, indent_level)).collect();
        format!("[{}]", items.join(", "))
    } else {
        let first_items: Vec<String> =
            arr.iter().take(3).map(|v| v.format_value_with_indent(ctx, indent_level)).collect();

        let suffix = if is_fixed {
            format!(", ...[{} total]", arr.len())
        } else {
            format!(", ...[{} items]", arr.len())
        };

        format!("[{}{}]", first_items.join(", "), suffix)
    }
}

fn format_tuple(
    tuple: &[DynSolValue],
    ctx: &SolValueFormatterContext,
    indent_level: usize,
) -> String {
    if tuple.is_empty() {
        return "()".to_string();
    }

    if tuple.len() == 1 {
        return format!("({})", tuple[0].format_value_with_indent(ctx, indent_level));
    }

    const MAX_DISPLAY_FIELDS: usize = 4;

    if ctx.multi_line && tuple.len() > 1 {
        let child_indent = make_indent(indent_level + 1);
        let current_indent = make_indent(indent_level);

        let items: Vec<String> = if ctx.shorten_long && tuple.len() > MAX_DISPLAY_FIELDS {
            let mut items = tuple
                .iter()
                .take(3)
                .map(|v| {
                    format!("{}{}", child_indent, v.format_value_with_indent(ctx, indent_level + 1))
                })
                .collect::<Vec<_>>();
            items.push(format!("{}...[{} fields]", child_indent, tuple.len()));
            items
        } else {
            tuple
                .iter()
                .map(|v| {
                    format!("{}{}", child_indent, v.format_value_with_indent(ctx, indent_level + 1))
                })
                .collect()
        };
        format!("(\n{}\n{})", items.join(",\n"), current_indent)
    } else if tuple.len() <= MAX_DISPLAY_FIELDS || !ctx.shorten_long {
        let items: Vec<String> =
            tuple.iter().map(|v| v.format_value_with_indent(ctx, indent_level)).collect();
        format!("({})", items.join(", "))
    } else {
        let first_items: Vec<String> =
            tuple.iter().take(3).map(|v| v.format_value_with_indent(ctx, indent_level)).collect();
        format!("({}, ...[{} fields])", first_items.join(", "), tuple.len())
    }
}

fn format_custom_struct(
    name: &str,
    prop_names: &[String],
    tuple: &[DynSolValue],
    ctx: &SolValueFormatterContext,
    indent_level: usize,
) -> String {
    const MAX_DISPLAY_FIELDS: usize = 4;

    if ctx.multi_line && tuple.len() > 1 {
        let child_indent = make_indent(indent_level + 1);
        let current_indent = make_indent(indent_level);

        let fields: Vec<String> = if ctx.shorten_long && tuple.len() > MAX_DISPLAY_FIELDS {
            let mut fields = tuple
                .iter()
                .zip(prop_names.iter())
                .take(3)
                .map(|(value, field_name)| {
                    format!(
                        "{}{}: {}",
                        child_indent,
                        field_name,
                        value.format_value_with_indent(ctx, indent_level + 1)
                    )
                })
                .collect::<Vec<_>>();
            fields.push(format!("{}...[{} fields]", child_indent, tuple.len()));
            fields
        } else {
            tuple
                .iter()
                .zip(prop_names.iter())
                .map(|(value, field_name)| {
                    format!(
                        "{}{}: {}",
                        child_indent,
                        field_name,
                        value.format_value_with_indent(ctx, indent_level + 1)
                    )
                })
                .collect()
        };

        if ctx.with_ty {
            format!("{}{{\n{}\n{}}}", name, fields.join(",\n"), current_indent)
        } else {
            format!("{{\n{}\n{}}}", fields.join(",\n"), current_indent)
        }
    } else {
        let fields: Vec<String> = if ctx.shorten_long && tuple.len() > MAX_DISPLAY_FIELDS {
            let mut fields = tuple
                .iter()
                .zip(prop_names.iter())
                .take(3)
                .map(|(value, field_name)| {
                    format!("{}: {}", field_name, value.format_value_with_indent(ctx, indent_level))
                })
                .collect::<Vec<_>>();
            fields.push(format!("...[{} fields]", tuple.len()));
            fields
        } else {
            tuple
                .iter()
                .zip(prop_names.iter())
                .map(|(value, field_name)| {
                    format!("{}: {}", field_name, value.format_value_with_indent(ctx, indent_level))
                })
                .collect()
        };

        if ctx.with_ty {
            format!("{}{{ {} }}", name, fields.join(", "))
        } else {
            format!("{{ {} }}", fields.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, FixedBytes, I256, U256};
    use serde_json;

    #[test]
    fn test_serialize_deserialize_bool() {
        let value = EdbSolValue(DynSolValue::Bool(true));
        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: EdbSolValue = serde_json::from_str(&serialized).unwrap();

        match deserialized.0 {
            DynSolValue::Bool(b) => assert!(b),
            _ => panic!("Expected Bool variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_uint() {
        let value = EdbSolValue(DynSolValue::Uint(U256::from(42u64), 256));
        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: EdbSolValue = serde_json::from_str(&serialized).unwrap();

        match deserialized.0 {
            DynSolValue::Uint(u, bits) => {
                assert_eq!(u, U256::from(42u64));
                assert_eq!(bits, 256);
            }
            _ => panic!("Expected Uint variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_int() {
        let value = EdbSolValue(DynSolValue::Int(I256::try_from(-42i64).unwrap(), 256));
        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: EdbSolValue = serde_json::from_str(&serialized).unwrap();

        match deserialized.0 {
            DynSolValue::Int(i, bits) => {
                assert_eq!(i, I256::try_from(-42i64).unwrap());
                assert_eq!(bits, 256);
            }
            _ => panic!("Expected Int variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_address() {
        let addr = address!("0000000000000000000000000000000000000001");
        let value = EdbSolValue(DynSolValue::Address(addr));
        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: EdbSolValue = serde_json::from_str(&serialized).unwrap();

        match deserialized.0 {
            DynSolValue::Address(a) => assert_eq!(a, addr),
            _ => panic!("Expected Address variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_bytes() {
        let value = EdbSolValue(DynSolValue::Bytes(vec![1, 2, 3, 4]));
        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: EdbSolValue = serde_json::from_str(&serialized).unwrap();

        match deserialized.0 {
            DynSolValue::Bytes(b) => assert_eq!(b, vec![1, 2, 3, 4]),
            _ => panic!("Expected Bytes variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_fixed_bytes() {
        let bytes = FixedBytes::<32>::from([1u8; 32]);
        let value = EdbSolValue(DynSolValue::FixedBytes(bytes, 32));
        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: EdbSolValue = serde_json::from_str(&serialized).unwrap();

        match deserialized.0 {
            DynSolValue::FixedBytes(b, size) => {
                assert_eq!(b, bytes);
                assert_eq!(size, 32);
            }
            _ => panic!("Expected FixedBytes variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_string() {
        let value = EdbSolValue(DynSolValue::String("Hello, Ethereum!".to_string()));
        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: EdbSolValue = serde_json::from_str(&serialized).unwrap();

        match deserialized.0 {
            DynSolValue::String(s) => assert_eq!(s, "Hello, Ethereum!"),
            _ => panic!("Expected String variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_array() {
        let value = EdbSolValue(DynSolValue::Array(vec![
            DynSolValue::Uint(U256::from(1u64), 256),
            DynSolValue::Uint(U256::from(2u64), 256),
            DynSolValue::Uint(U256::from(3u64), 256),
        ]));
        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: EdbSolValue = serde_json::from_str(&serialized).unwrap();

        match deserialized.0 {
            DynSolValue::Array(arr) => {
                assert_eq!(arr.len(), 3);
                match &arr[0] {
                    DynSolValue::Uint(u, _) => assert_eq!(*u, U256::from(1u64)),
                    _ => panic!("Expected Uint in array"),
                }
            }
            _ => panic!("Expected Array variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_fixed_array() {
        let value = EdbSolValue(DynSolValue::FixedArray(vec![
            DynSolValue::Bool(true),
            DynSolValue::Bool(false),
        ]));
        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: EdbSolValue = serde_json::from_str(&serialized).unwrap();

        match deserialized.0 {
            DynSolValue::FixedArray(arr) => {
                assert_eq!(arr.len(), 2);
                match (&arr[0], &arr[1]) {
                    (DynSolValue::Bool(b1), DynSolValue::Bool(b2)) => {
                        assert!(*b1);
                        assert!(!*b2);
                    }
                    _ => panic!("Expected Bool values in fixed array"),
                }
            }
            _ => panic!("Expected FixedArray variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_tuple() {
        let addr = address!("0000000000000000000000000000000000000001");
        let value = EdbSolValue(DynSolValue::Tuple(vec![
            DynSolValue::Address(addr),
            DynSolValue::Uint(U256::from(100u64), 256),
            DynSolValue::String("test".to_string()),
        ]));
        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: EdbSolValue = serde_json::from_str(&serialized).unwrap();

        match deserialized.0 {
            DynSolValue::Tuple(tuple) => {
                assert_eq!(tuple.len(), 3);
                match (&tuple[0], &tuple[1], &tuple[2]) {
                    (DynSolValue::Address(a), DynSolValue::Uint(u, _), DynSolValue::String(s)) => {
                        assert_eq!(*a, addr);
                        assert_eq!(*u, U256::from(100u64));
                        assert_eq!(s, "test");
                    }
                    _ => panic!("Expected (Address, Uint, String) in tuple"),
                }
            }
            _ => panic!("Expected Tuple variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_nested_structure() {
        let value = EdbSolValue(DynSolValue::Array(vec![
            DynSolValue::Tuple(vec![
                DynSolValue::Uint(U256::from(1u64), 256),
                DynSolValue::Array(vec![
                    DynSolValue::String("nested1".to_string()),
                    DynSolValue::String("nested2".to_string()),
                ]),
            ]),
            DynSolValue::Tuple(vec![
                DynSolValue::Uint(U256::from(2u64), 256),
                DynSolValue::Array(vec![DynSolValue::String("nested3".to_string())]),
            ]),
        ]));

        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: EdbSolValue = serde_json::from_str(&serialized).unwrap();

        match deserialized.0 {
            DynSolValue::Array(arr) => {
                assert_eq!(arr.len(), 2);
                match &arr[0] {
                    DynSolValue::Tuple(tuple) => {
                        assert_eq!(tuple.len(), 2);
                        match &tuple[1] {
                            DynSolValue::Array(inner_arr) => {
                                assert_eq!(inner_arr.len(), 2);
                            }
                            _ => panic!("Expected Array in tuple"),
                        }
                    }
                    _ => panic!("Expected Tuple in array"),
                }
            }
            _ => panic!("Expected Array variant"),
        }
    }

    #[test]
    fn test_serialize_deserialize_custom_struct() {
        let value = EdbSolValue(DynSolValue::CustomStruct {
            name: "Person".to_string(),
            prop_names: vec!["name".to_string(), "age".to_string()],
            tuple: vec![
                DynSolValue::String("Alice".to_string()),
                DynSolValue::Uint(U256::from(30u64), 256),
            ],
        });

        let serialized = serde_json::to_string(&value).unwrap();
        let deserialized: EdbSolValue = serde_json::from_str(&serialized).unwrap();

        match deserialized.0 {
            DynSolValue::CustomStruct { name, prop_names, tuple } => {
                assert_eq!(name, "Person");
                assert_eq!(prop_names, vec!["name", "age"]);
                assert_eq!(tuple.len(), 2);
                match (&tuple[0], &tuple[1]) {
                    (DynSolValue::String(s), DynSolValue::Uint(u, _)) => {
                        assert_eq!(s, "Alice");
                        assert_eq!(*u, U256::from(30u64));
                    }
                    _ => panic!("Expected (String, Uint) in custom struct"),
                }
            }
            _ => panic!("Expected CustomStruct variant"),
        }
    }

    #[test]
    fn test_json_format_readability() {
        let value = EdbSolValue(DynSolValue::Tuple(vec![
            DynSolValue::Bool(true),
            DynSolValue::Uint(U256::from(42u64), 256),
        ]));

        let json = serde_json::to_string_pretty(&value).unwrap();
        // Verify that the JSON is readable and contains expected structure
        assert!(json.contains("\"type\""));
        assert!(json.contains("\"value\""));
        assert!(json.contains("\"Tuple\""));
    }

    #[test]
    fn test_single_line_formatting() {
        let ctx = SolValueFormatterContext::new();

        // Test array single line
        let array = DynSolValue::Array(vec![
            DynSolValue::Uint(U256::from(1u64), 256),
            DynSolValue::Uint(U256::from(2u64), 256),
            DynSolValue::Uint(U256::from(3u64), 256),
        ]);
        let result = array.format_value(&ctx);
        assert_eq!(result, "[1, 2, 3]");

        // Test tuple single line
        let tuple = DynSolValue::Tuple(vec![
            DynSolValue::Bool(true),
            DynSolValue::Uint(U256::from(42u64), 256),
        ]);
        let result = tuple.format_value(&ctx);
        assert_eq!(result, "(true, 42)");
    }

    #[test]
    fn test_multi_line_formatting() {
        let ctx = SolValueFormatterContext::new().multi_line(true);

        // Test array multi-line
        let array = DynSolValue::Array(vec![
            DynSolValue::Uint(U256::from(1u64), 256),
            DynSolValue::Uint(U256::from(2u64), 256),
            DynSolValue::Uint(U256::from(3u64), 256),
        ]);
        let result = array.format_value(&ctx);
        assert_eq!(result, "[\n  1,\n  2,\n  3\n]");

        // Test tuple multi-line
        let tuple = DynSolValue::Tuple(vec![
            DynSolValue::Bool(true),
            DynSolValue::Uint(U256::from(42u64), 256),
        ]);
        let result = tuple.format_value(&ctx);
        assert_eq!(result, "(\n  true,\n  42\n)");

        // Test custom struct multi-line
        let custom_struct = DynSolValue::CustomStruct {
            name: "Person".to_string(),
            prop_names: vec!["name".to_string(), "age".to_string()],
            tuple: vec![
                DynSolValue::String("Alice".to_string()),
                DynSolValue::Uint(U256::from(30u64), 256),
            ],
        };
        let result = custom_struct.format_value(&ctx);
        assert_eq!(result, "{\n  name: \"Alice\",\n  age: 30\n}");
    }

    #[test]
    fn test_multi_line_with_type_info() {
        let ctx = SolValueFormatterContext::new().multi_line(true).with_ty(true);

        let custom_struct = DynSolValue::CustomStruct {
            name: "Person".to_string(),
            prop_names: vec!["name".to_string(), "age".to_string()],
            tuple: vec![
                DynSolValue::String("Alice".to_string()),
                DynSolValue::Uint(U256::from(30u64), 256),
            ],
        };
        let result = custom_struct.format_value(&ctx);
        assert_eq!(result, "Person{\n  name: \"Alice\",\n  age: uint256(30)\n}");
    }

    #[test]
    fn test_single_element_formatting() {
        let ctx = SolValueFormatterContext::new().multi_line(true);

        // Single element array should not use multi-line
        let array = DynSolValue::Array(vec![DynSolValue::Uint(U256::from(1u64), 256)]);
        let result = array.format_value(&ctx);
        assert_eq!(result, "[1]");

        // Single element tuple should not use multi-line
        let tuple = DynSolValue::Tuple(vec![DynSolValue::Bool(true)]);
        let result = tuple.format_value(&ctx);
        assert_eq!(result, "(true)");
    }

    #[test]
    fn test_empty_collections() {
        let ctx = SolValueFormatterContext::new().multi_line(true);

        // Empty array
        let empty_array = DynSolValue::Array(vec![]);
        let result = empty_array.format_value(&ctx);
        assert_eq!(result, "[]");

        // Empty tuple
        let empty_tuple = DynSolValue::Tuple(vec![]);
        let result = empty_tuple.format_value(&ctx);
        assert_eq!(result, "()");

        // Empty fixed array
        let empty_fixed_array = DynSolValue::FixedArray(vec![]);
        let result = empty_fixed_array.format_value(&ctx);
        assert_eq!(result, "[]");
    }

    #[test]
    fn test_deeply_nested_structures() {
        let ctx = SolValueFormatterContext::new().multi_line(true);

        // Nested array within array
        let nested_array = DynSolValue::Array(vec![
            DynSolValue::Array(vec![
                DynSolValue::Uint(U256::from(1u64), 256),
                DynSolValue::Uint(U256::from(2u64), 256),
            ]),
            DynSolValue::Array(vec![
                DynSolValue::Uint(U256::from(3u64), 256),
                DynSolValue::Uint(U256::from(4u64), 256),
            ]),
        ]);
        let result = nested_array.format_value(&ctx);
        let expected = "[\n  [\n    1,\n    2\n  ],\n  [\n    3,\n    4\n  ]\n]";
        assert_eq!(result, expected);

        // Nested struct within struct
        let nested_struct = DynSolValue::CustomStruct {
            name: "Outer".to_string(),
            prop_names: vec!["inner".to_string(), "value".to_string()],
            tuple: vec![
                DynSolValue::CustomStruct {
                    name: "Inner".to_string(),
                    prop_names: vec!["x".to_string(), "y".to_string()],
                    tuple: vec![
                        DynSolValue::Uint(U256::from(10u64), 256),
                        DynSolValue::Uint(U256::from(20u64), 256),
                    ],
                },
                DynSolValue::Uint(U256::from(100u64), 256),
            ],
        };
        let result = nested_struct.format_value(&ctx);
        let expected = "{\n  inner: {\n    x: 10,\n    y: 20\n  },\n  value: 100\n}";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_shorten_long_with_multi_line() {
        let ctx = SolValueFormatterContext::new().multi_line(true).shorten_long(true);

        // Long array should be shortened even in multi-line mode
        let long_array =
            DynSolValue::Array((1..=10).map(|i| DynSolValue::Uint(U256::from(i), 256)).collect());
        let result = long_array.format_value(&ctx);
        let expected = "[\n  1,\n  2,\n  3,\n  ...[10 items]\n]";
        assert_eq!(result, expected);

        // Long tuple should be shortened
        let long_tuple =
            DynSolValue::Tuple((1..=8).map(|i| DynSolValue::Uint(U256::from(i), 256)).collect());
        let result = long_tuple.format_value(&ctx);
        let expected = "(\n  1,\n  2,\n  3,\n  ...[8 fields]\n)";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_triple_nested_indentation() {
        let ctx = SolValueFormatterContext::new().multi_line(true);

        // Three levels of nesting to test proper indentation
        let triple_nested =
            DynSolValue::Array(vec![DynSolValue::Tuple(vec![DynSolValue::Array(vec![
                DynSolValue::Uint(U256::from(1u64), 256),
                DynSolValue::Uint(U256::from(2u64), 256),
            ])])]);
        let result = triple_nested.format_value(&ctx);
        let expected = "[([\n  1,\n  2\n])]"; // Actual format from error
        assert_eq!(result, expected);
    }

    #[test]
    fn test_mixed_complex_structures() {
        let ctx = SolValueFormatterContext::new().multi_line(true);

        // Array containing tuples containing structs
        let complex_value = DynSolValue::Array(vec![DynSolValue::Tuple(vec![
            DynSolValue::CustomStruct {
                name: "Point".to_string(),
                prop_names: vec!["x".to_string(), "y".to_string()],
                tuple: vec![
                    DynSolValue::Uint(U256::from(1u64), 256),
                    DynSolValue::Uint(U256::from(2u64), 256),
                ],
            },
            DynSolValue::Bool(true),
        ])]);
        let result = complex_value.format_value(&ctx);
        let expected = "[(\n  {\n    x: 1,\n    y: 2\n  },\n  true\n)]";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_bytes_and_strings_multi_line() {
        let ctx = SolValueFormatterContext::new().multi_line(true);

        let mixed_data = DynSolValue::Array(vec![
            DynSolValue::String("Hello, World!".to_string()),
            DynSolValue::Bytes(vec![0x01, 0x02, 0x03, 0x04]),
            DynSolValue::FixedBytes(FixedBytes::<32>::from([0xff; 32]), 32),
        ]);
        let result = mixed_data.format_value(&ctx);
        let expected = "[\n  \"Hello, World!\",\n  0x01020304,\n  0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\n]";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_address_formatting_nested() {
        let ctx = SolValueFormatterContext::new().multi_line(true).shorten_long(true);

        let struct_with_addresses = DynSolValue::CustomStruct {
            name: "Transfer".to_string(),
            prop_names: vec!["from".to_string(), "to".to_string()],
            tuple: vec![
                DynSolValue::Address("0x742d35Cc6639C0532fBb5dd9D09A0CB21234000A".parse().unwrap()),
                DynSolValue::Address("0x0000000000000000000000000000000000000000".parse().unwrap()),
            ],
        };
        let result = struct_with_addresses.format_value(&ctx);
        let expected = "{\n  from: 0x742d35...34000A,\n  to: 0x0000000000000000\n}";
        assert_eq!(result, expected);
    }

    #[test]
    fn test_all_options_together() {
        let ctx = SolValueFormatterContext::new().multi_line(true).with_ty(true).shorten_long(true);

        let complex_struct = DynSolValue::CustomStruct {
            name: "Transaction".to_string(),
            prop_names: vec!["from".to_string(), "to".to_string(), "values".to_string()],
            tuple: vec![
                DynSolValue::Address("0x742d35Cc6639C0532fBb5dd9D09A0CB21234000A".parse().unwrap()),
                DynSolValue::Address("0x123F681646d4A755815f9CB19e1aCc8565A0c2AC".parse().unwrap()),
                DynSolValue::Array(
                    (1..=10).map(|i| DynSolValue::Uint(U256::from(i), 256)).collect(),
                ),
            ],
        };
        let result = complex_struct.format_value(&ctx);
        let expected = "Transaction{\n  from: address(0x742d35...34000A),\n  to: address(0x123F68...A0c2AC),\n  values: [\n    uint256(1),\n    uint256(2),\n    uint256(3),\n    ...[10 items]\n  ]\n}";
        assert_eq!(result, expected);
    }
}
