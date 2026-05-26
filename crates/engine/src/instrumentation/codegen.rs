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

use edb_common::types::EDB_STATE_VAR_FLAG;
use foundry_compilers::artifacts::{ContractKind, Mutability, StorageLocation, TypeName};
use semver::Version;

use crate::{
    MAGIC_SNAPSHOT_NUMBER, MAGIC_VARIABLE_UPDATE_NUMBER, VersionRef,
    analysis::{USID, UVID, VariableRef},
    contains_function_type, contains_mapping_type, contains_user_defined_type,
};

pub fn generate_step_hook(version: &VersionRef, usid: USID) -> Option<String> {
    // Solidity 0.4 does not support abi.encode, so we use a "0.4-compatible" way to encode the
    // parameters.
    if **version < Version::parse("0.5.0").unwrap() {
        Some(format!(
            "require(keccak256(uint256({}), uint256({})) != bytes32(uint256(0x2333)));",
            MAGIC_SNAPSHOT_NUMBER,
            u64::from(usid)
        ))
    } else {
        Some(format!(
            "require(keccak256(abi.encode(uint256({}), uint256({}))) != bytes32(uint256(0x2333)));",
            MAGIC_SNAPSHOT_NUMBER,
            u64::from(usid)
        ))
    }
}

/// Generates a variable update hook.
pub fn generate_variable_update_hook(
    version: &VersionRef,
    uvid: UVID,
    variable: &VariableRef,
) -> Option<String> {
    if **version < Version::parse("0.4.24").unwrap() {
        // if the abi.encode function is not available, we skip the variable update hook
        // TODO: support solidity <0.4.24 in the future
        return None;
    }

    // We currently do not support recording variables involving user-defined types and arrays (<
    // 0.8.0), as well as state variables. In addition, source code with 0.4.x solidity version
    // is not supported due to the lack of the `abi.encode` function. TODO: support user-defined
    // types and arrays, as well as state variables, solidity <0.4.24, in the future
    let declaration = variable.declaration();
    let base_type = &declaration.type_name;
    let is_state_variable = declaration.state_variable;
    let is_storage_variable = declaration.storage_location == StorageLocation::Storage;
    if base_type.as_ref().is_some_and(|ty| {
        (contains_user_defined_type(ty) && **version < Version::parse("0.8.0").unwrap())
            || contains_function_type(ty)
            || contains_mapping_type(ty)
            || is_state_variable
            || is_storage_variable
    }) {
        return None;
    }

    let base_var = variable.base();
    let base_name = &base_var.declaration().name;
    Some(format!(
        "require(keccak256(abi.encode(uint256({}), uint256({}), abi.encode({}))) != bytes32(uint256(0x2333)));",
        MAGIC_VARIABLE_UPDATE_NUMBER,
        u64::from(uvid),
        base_name
    ))
}

/// Generates a view method for a state variable.
///
/// For primitive types, generates a simple view function.
/// For arrays and mappings, recursively adds index/key parameters.
/// Returns None if the variable contains user-defined types or is constant.
///
/// # Arguments
/// * `private_state_variable` - The VariableRef for the private state variable
///
/// # Returns
/// * `Option<String>` - The generated view function code, or None if user-defined types are present
pub fn generate_view_method(state_variable: &VariableRef) -> Option<String> {
    if state_variable.contract().map(|c| c.kind() != ContractKind::Contract).unwrap_or(true) {
        // We only generate view methods for state variables in contracts
        return None;
    }

    let declaration = state_variable.declaration();
    if declaration.mutability == Some(Mutability::Constant) {
        // We do not need to output constant state variables
        return None;
    }

    let var_name = &declaration.name;

    // Get the type information
    let type_name = declaration.type_name.as_ref()?;

    // Check if the type contains user-defined types and get parameter info
    let (params, return_type) = analyze_type_for_view_method(type_name)?;

    // Generate the function signature
    let params_str = if params.is_empty() { String::new() } else { params.join(", ") };

    // Generate the function body
    let body = generate_view_body(var_name, &params);

    // Get UVID
    let uvid = state_variable.id();

    // Construct the complete function with EDB prefix
    Some(format!(
        "    function {var_name}{EDB_STATE_VAR_FLAG}{uvid}({params_str}) public view returns ({return_type}) {{\n        return {body};\n    }}"
    ))
}

/// Analyzes a TypeName and returns parameter list and return type for the view function.
/// Returns None if user-defined types are found.
fn analyze_type_for_view_method(type_name: &TypeName) -> Option<(Vec<String>, String)> {
    analyze_type_recursive(type_name, 0)
}

/// Recursively analyzes a TypeName to build parameter list and return type.
/// Returns None if user-defined types are found.
fn analyze_type_recursive(type_name: &TypeName, depth: usize) -> Option<(Vec<String>, String)> {
    match type_name {
        TypeName::ElementaryTypeName(elementary) => {
            // Elementary types are primitive types like uint, address, bool, etc.
            let return_type = format_return_type(&elementary.name);
            Some((Vec::new(), return_type))
        }
        TypeName::Mapping(mapping) => {
            // For mapping, we need to add a key parameter and recurse on value type
            let key_type = &mapping.key_type;
            let value_type = &mapping.value_type;

            // Get key type as string (must be elementary)
            let key_type_str = match key_type {
                TypeName::ElementaryTypeName(elem) => elem.name.clone(),
                _ => return None, // Mapping keys must be elementary types
            };

            // Recurse on value type
            let (mut sub_params, return_type) = analyze_type_recursive(value_type, depth + 1)?;

            // Add key parameter at the beginning
            let param_name = if depth == 0 { "key".to_string() } else { format!("key{depth}") };
            let key_param = format!("{key_type_str} {param_name}");

            let mut params = vec![key_param];
            params.append(&mut sub_params);

            Some((params, return_type))
        }
        TypeName::ArrayTypeName(array) => {
            // For arrays, we need to add an index parameter and recurse on base type
            let base_type = &array.base_type;

            // Recurse on base type
            let (mut sub_params, return_type) = analyze_type_recursive(base_type, depth + 1)?;

            // Add index parameter at the beginning
            let param_name = if depth == 0 { "index".to_string() } else { format!("index{depth}") };
            let index_param = format!("uint256 {param_name}");

            let mut params = vec![index_param];
            params.append(&mut sub_params);

            Some((params, return_type))
        }
        TypeName::UserDefinedTypeName(_) => {
            // User-defined types (structs, enums, contracts) - skip
            None
        }
        TypeName::FunctionTypeName(_) => {
            // Function types - skip
            None
        }
    }
}

/// Generates the body of the view function based on the variable name and parameters.
fn generate_view_body(var_name: &str, params: &[String]) -> String {
    if params.is_empty() {
        var_name.to_string()
    } else {
        // Extract parameter names from the parameter declarations
        let param_names: Vec<String> = params
            .iter()
            .map(|p| {
                // Extract the parameter name (last word in the parameter declaration)
                p.split_whitespace().last().unwrap_or("").to_string()
            })
            .collect();

        // Build the access expression (e.g., "myVar[key][index]")
        let mut body = var_name.to_string();
        for param_name in param_names {
            body = format!("{body}[{param_name}]");
        }
        body
    }
}

/// Formats the return type with appropriate data location for reference types.
/// For reference types (string, dynamic bytes, arrays), adds "memory" data location.
/// For value types (uint*, int*, bool, address, bytesN with N=1..32), returns as-is.
fn format_return_type(type_name: &str) -> String {
    // Dynamic arrays (ends with [])
    if type_name.ends_with("[]") {
        return format!("{type_name} memory");
    }
    // Fixed-size arrays (contains [n] where n is a number)
    if type_name.contains('[') && type_name.contains(']') {
        return format!("{type_name} memory");
    }
    // `string` (dynamic): needs memory. Elementary type names are bare (no
    // trailing " memory" / " calldata" suffix) so equality is the canonical
    // shape; the `starts_with` arm tolerates pre-suffixed forms defensively.
    if type_name == "string" || type_name.starts_with("string ") {
        return format!("{type_name} memory");
    }
    // Dynamic `bytes`: needs memory. `bytesN` (N = 1..32) is a value type
    // and must NOT be promoted — distinguish by checking that what follows
    // "bytes" is not a digit. Solc's elementary type names use the digit
    // form (e.g. `bytes32`), so a simple digit check is sufficient.
    if type_name == "bytes"
        || type_name.starts_with("bytes ")
        || (type_name.starts_with("bytes")
            && !type_name.chars().nth(5).map(|c| c.is_ascii_digit()).unwrap_or(false))
    {
        return format!("{type_name} memory");
    }
    type_name.to_string()
}

#[cfg(test)]
mod tests {
    use crate::analysis;

    #[test]
    fn test_generate_view_method_primitive_types() {
        let source = r#"
        contract C {
            uint256 private myValue;
            address private owner;
            bool private isActive;
        }
        "#;
        let (_sources, analysis) = analysis::tests::compile_and_analyze(source);

        // Should find 3 private state variables
        assert_eq!(analysis.private_state_variables.len(), 3);

        // Test each private state variable
        for private_var in &analysis.private_state_variables {
            let result = super::generate_view_method(private_var);
            assert!(
                result.is_some(),
                "Should generate view method for primitive type: {}",
                private_var.declaration().name
            );

            let code = result.unwrap();
            let var_name = &private_var.declaration().name;

            // Check function signature contains the variable name with EDB suffix
            assert!(code.contains(&format!("function {var_name}_edb_state_var_")));
            assert!(code.contains("public view returns"));

            // Check function body returns the variable
            assert!(code.contains(&format!("return {var_name};")));
        }
    }

    #[test]
    fn test_generate_view_method_mapping_types() {
        let source = r#"
        contract C {
            mapping(address => uint256) private balances;
            mapping(uint256 => bool) private permissions;
        }
        "#;
        let (_sources, analysis) = analysis::tests::compile_and_analyze(source);

        // Should find 2 private state variables
        assert_eq!(analysis.private_state_variables.len(), 2);

        for private_var in &analysis.private_state_variables {
            let result = super::generate_view_method(private_var);
            assert!(
                result.is_some(),
                "Should generate view method for mapping: {}",
                private_var.declaration().name
            );

            let code = result.unwrap();
            let var_name = &private_var.declaration().name;

            // Check function signature has a key parameter with EDB suffix
            assert!(code.contains(&format!("function {var_name}_edb_state_var_")));
            assert!(code.contains("key) public view returns"));

            // Check function body accesses the mapping with key
            assert!(code.contains(&format!("return {var_name}[key];")));
        }
    }

    #[test]
    fn test_generate_view_method_array_types() {
        let source = r#"
        contract C {
            uint256[] private numbers;
            address[] private addresses;
        }
        "#;
        let (_sources, analysis) = analysis::tests::compile_and_analyze(source);

        // Should find 2 private state variables
        assert_eq!(analysis.private_state_variables.len(), 2);

        for private_var in &analysis.private_state_variables {
            let result = super::generate_view_method(private_var);
            assert!(
                result.is_some(),
                "Should generate view method for array: {}",
                private_var.declaration().name
            );

            let code = result.unwrap();
            let var_name = &private_var.declaration().name;

            // Check function signature has an index parameter with EDB suffix
            assert!(code.contains(&format!("function {var_name}_edb_state_var_")));
            assert!(code.contains("(uint256 index)"));
            assert!(code.contains("public view returns"));

            // Check function body accesses the array with index
            assert!(code.contains(&format!("return {var_name}[index];")));
        }
    }

    #[test]
    fn test_generate_view_method_nested_types() {
        let source = r#"
        contract C {
            mapping(address => uint256[]) private userTokens;
            mapping(uint256 => mapping(address => bool)) private permissions;
        }
        "#;
        let (_sources, analysis) = analysis::tests::compile_and_analyze(source);

        // Should find 2 private state variables
        assert_eq!(analysis.private_state_variables.len(), 2);

        let user_tokens =
            analysis.private_state_variables.iter().find(|v| v.declaration().name == "userTokens");
        let permissions =
            analysis.private_state_variables.iter().find(|v| v.declaration().name == "permissions");

        // Test userTokens mapping(address => uint256[])
        if let Some(user_tokens_var) = user_tokens {
            let result = super::generate_view_method(user_tokens_var);
            assert!(result.is_some(), "Should generate view method for nested mapping->array");

            let code = result.unwrap();
            // Should have address key parameter and uint256 index1 parameter (depth-based naming)
            // with EDB suffix
            assert!(code.contains("function userTokens_edb_state_var_"));
            assert!(code.contains("(address key, uint256 index1)"));
            assert!(code.contains("return userTokens[key][index1];"));
        }

        // Test permissions mapping(uint256 => mapping(address => bool))
        if let Some(permissions_var) = permissions {
            let result = super::generate_view_method(permissions_var);
            assert!(result.is_some(), "Should generate view method for nested mapping->mapping");

            let code = result.unwrap();
            // Should have uint256 key parameter and address key1 parameter (depth-based naming)
            // with EDB suffix
            assert!(code.contains("function permissions_edb_state_var_"));
            assert!(code.contains("(uint256 key, address key1)"));
            assert!(code.contains("return permissions[key][key1];"));
        }
    }

    #[test]
    fn test_generate_view_method_user_defined_types() {
        let source = r#"
        contract C {
            struct User {
                uint256 balance;
                address addr;
            }

            User private userData;
            User[] private users;
        }
        "#;
        let (_sources, analysis) = analysis::tests::compile_and_analyze(source);

        // Note: If no private state variables with user-defined types are detected,
        // this test verifies that the analysis correctly filters them out.
        // If they are detected, they should return None from generate_view_method

        for private_var in &analysis.private_state_variables {
            let result = super::generate_view_method(private_var);
            // Should return None for user-defined types
            assert!(
                result.is_none(),
                "Should not generate view method for user-defined type: {}",
                private_var.declaration().name
            );
        }
    }

    #[test]
    fn test_generate_view_method_reference_types() {
        let source = r#"
        contract C {
            string private message;
            string[] private messages;
        }
        "#;
        let (_sources, analysis) = analysis::tests::compile_and_analyze(source);

        // Should find 2 private state variables
        assert_eq!(analysis.private_state_variables.len(), 2);

        for private_var in &analysis.private_state_variables {
            let result = super::generate_view_method(private_var);
            assert!(
                result.is_some(),
                "Should generate view method for reference type: {}",
                private_var.declaration().name
            );

            let code = result.unwrap();
            let var_name = &private_var.declaration().name;

            // Check that memory data location is added for reference types
            match var_name.as_str() {
                "message" => {
                    assert!(code.contains("function message_edb_state_var_"));
                    assert!(code.contains("() public view returns (string memory)"));
                    assert!(code.contains("return message;"));
                }
                "messages" => {
                    assert!(code.contains("function messages_edb_state_var_"));
                    assert!(code.contains("(uint256 index) public view returns (string memory)"));
                    assert!(code.contains("return messages[index];"));
                }
                _ => panic!("Unexpected variable name: {var_name}"),
            }
        }
    }

    /// Regression for the case that broke `solady` ERC1155 tests: a `bytes`
    /// state variable (dynamic, reference type) needs `memory` on the
    /// return parameter, while a `bytesN` state variable (value type) does
    /// NOT — solc rejects `returns (bytes)` but accepts `returns (bytes32)`.
    #[test]
    #[allow(non_snake_case)]
    fn test_generate_view_method_bytes_and_bytesN() {
        let source = r#"
        contract C {
            bytes private blob;
            bytes32 private digest;
            bytes[] private blobs;
            bytes32[] private digests;
        }
        "#;
        let (_sources, analysis) = analysis::tests::compile_and_analyze(source);

        assert_eq!(analysis.private_state_variables.len(), 4);

        for private_var in &analysis.private_state_variables {
            let code =
                super::generate_view_method(private_var).expect("should generate view method");
            let var_name = &private_var.declaration().name;
            match var_name.as_str() {
                "blob" => {
                    assert!(
                        code.contains("returns (bytes memory)"),
                        "dynamic bytes must be `memory`: {code}"
                    );
                }
                "digest" => {
                    assert!(
                        code.contains("returns (bytes32)"),
                        "bytes32 is value type, no `memory`: {code}"
                    );
                    assert!(!code.contains("bytes32 memory"));
                }
                "blobs" => {
                    // `bytes[]` indexed access yields one `bytes` element, which
                    // still needs `memory` because dynamic bytes is a reference type.
                    assert!(
                        code.contains("(uint256 index) public view returns (bytes memory)"),
                        "bytes[] indexed access returns bytes memory: {code}"
                    );
                }
                "digests" => {
                    // `bytes32[]` indexed access yields one `bytes32` element —
                    // a value type — so no `memory`.
                    assert!(
                        code.contains("(uint256 index) public view returns (bytes32)"),
                        "bytes32[] indexed access returns bare bytes32: {code}"
                    );
                    assert!(!code.contains("bytes32 memory"));
                }
                _ => panic!("unexpected var: {var_name}"),
            }
        }
    }

    /// Pure-fn smoke for `format_return_type` so regressions on this small
    /// helper don't require a full analyze-and-generate round-trip.
    #[test]
    fn test_format_return_type_units() {
        use super::format_return_type;

        // Value types: no `memory`.
        assert_eq!(format_return_type("uint256"), "uint256");
        assert_eq!(format_return_type("int128"), "int128");
        assert_eq!(format_return_type("bool"), "bool");
        assert_eq!(format_return_type("address"), "address");
        assert_eq!(format_return_type("bytes1"), "bytes1");
        assert_eq!(format_return_type("bytes32"), "bytes32");

        // Reference types: `memory`.
        assert_eq!(format_return_type("string"), "string memory");
        assert_eq!(format_return_type("bytes"), "bytes memory");
        assert_eq!(format_return_type("uint256[]"), "uint256[] memory");
        assert_eq!(format_return_type("address[5]"), "address[5] memory");
        assert_eq!(format_return_type("bytes[]"), "bytes[] memory");
    }
}
