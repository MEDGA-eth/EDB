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

use foundry_compilers::artifacts::{
    Block, BlockOrStatement, StateMutability, Statement, TypeName, Visibility, ast::SourceLocation,
};
use semver::VersionReq;

use crate::analysis::{find_next_semicolon_after_source_location, stmt_src};

/// Get the source string at the given location.
///
/// # Arguments
///
/// * `id` - The source index
/// * `source` - The source string
/// * `location` - The source location. The index in the `location` must be the same as the `id` if
///   it is present.
pub fn source_string_at_location<'a>(
    id: u32,
    source: &'a str,
    location: &SourceLocation,
) -> &'a str {
    if let Some(index) = location.index {
        assert_eq!(index as u32, id, "Source index mismatch");
    }

    source_string_at_location_unchecked(source, location)
}

/// Get the source string at the given location.
///
/// # Arguments
///
/// * `source` - The source string
/// * `location` - The source location. The index in the `location` is not checked whether it is the
///   same as the id of the source.
pub fn source_string_at_location_unchecked<'a>(
    source: &'a str,
    location: &SourceLocation,
) -> &'a str {
    let start = location.start.unwrap_or(0);
    let end = location.length.map(|l| start + l).unwrap_or(source.len() - 1);
    &source[start..end]
}

/// Slice the source location.
///
/// # Arguments
///
/// * `src` - The source location
/// * `start` - The start index
/// * `length` - The length of the sliced source
///
/// # Returns
///
/// The sliced source location.
///
/// # Example
///
/// ```rust
/// let src = SourceLocation { start: Some(1), length: Some(10), index: Some(0) };
/// let sliced = slice_source_location(&src, 1, 3);
/// assert_eq!(sliced.start, Some(2));
/// assert_eq!(sliced.length, Some(3));
/// assert_eq!(sliced.index, Some(0));
/// ```
#[deprecated]
pub fn slice_source_location(src: &SourceLocation, start: usize, length: usize) -> SourceLocation {
    assert!(
        src.length.map(|l| l >= start).unwrap_or(true),
        "Sliced start is greater than the original source length"
    );
    assert!(
        src.length.map(|l| l >= start + length).unwrap_or(true),
        "Sliced source length is greater than the original source length"
    );

    SourceLocation {
        start: src.start.map(|s| s + start).or(Some(start)),
        length: Some(length),
        index: src.index,
    }
}

/// Convert the visibility to a string.
///
/// # Arguments
///
/// * `visibility` - The visibility
///
/// # Returns
///
/// The string representation of the visibility.
///
/// # Example
///
/// ```rust
/// let visibility = Visibility::Public;
/// let str = visibility_to_str(visibility);
/// assert_eq!(str, "public");
/// ```
pub fn visibility_to_str(visibility: &Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "public",
        Visibility::Internal => "internal",
        Visibility::Private => "private",
        Visibility::External => "external",
    }
}

/// Convert the mutability to a string.
///
/// # Arguments
///
/// * `mutability` - The mutability
///
/// # Returns
///
/// The string representation of the mutability.
///
/// # Example
///
/// ```rust
/// let mutability = StateMutability::Pure;
/// let str = mutability_to_str(mutability);
/// assert_eq!(str, "pure");
/// ```
pub fn mutability_to_str(mutability: &StateMutability) -> &'static str {
    match mutability {
        StateMutability::Pure => "pure",
        StateMutability::View => "view",
        StateMutability::Payable => "payable",
        StateMutability::Nonpayable => "nonpayable",
    }
}

/// Find the index of the next character immediately after the source location.
///
/// # Arguments
///
/// * `src` - The source location
///
/// # Returns
///
/// The index of the next character immediately after the source location.
pub fn find_next_index_of_source_location(src: &SourceLocation) -> Option<usize> {
    if let Some(start) = src.start
        && let Some(length) = src.length
    {
        return Some(start + length);
    }
    None
}

/// Find the index of the next character immediately after the `BlockOrStatement`.
///
/// # Arguments
///
/// * `source` - The source string
/// * `block_or_statement` - The `BlockOrStatement`
///
/// # Returns
///
/// The index of the next character immediately after the `BlockOrStatement`.
pub fn find_next_index_of_block_or_statement(
    source: &str,
    block_or_statement: &BlockOrStatement,
) -> Option<usize> {
    match block_or_statement {
        BlockOrStatement::Statement(statement) => find_next_index_of_statement(source, statement),
        BlockOrStatement::Block(block) => find_next_index_of_source_location(&block.src),
    }
}

/// Find the index of the first statement in the `BlockOrStatement`.
///
/// # Arguments
///
/// * `block_or_statement` - The `BlockOrStatement`
///
/// # Returns
///
/// The index of the first statement in the `BlockOrStatement`.
pub fn find_index_of_first_statement_in_block_or_statement(
    block_or_statement: &BlockOrStatement,
) -> Option<usize> {
    match block_or_statement {
        BlockOrStatement::Statement(statement) => match statement {
            Statement::Block(block) => find_index_of_first_statement_in_block(block),
            _ => stmt_src(statement).start,
        },
        BlockOrStatement::Block(block) => find_index_of_first_statement_in_block(block),
    }
}

/// Find the index of the first statement in the `Block`.
///
/// # Arguments
///
/// * `block` - The `Block`
///
/// # Returns
///
/// The index of the first statement in the `Block`.
pub fn find_index_of_first_statement_in_block(block: &Block) -> Option<usize> {
    block.statements.first().map_or(
        // if the block has no statements, the index of the first statement is the start of the
        // block '{' plus 1
        block.src.start.map(|s| s + 1),
        |stmt| stmt_src(stmt).start,
    )
}

/// Find the index of the next character immediately after the last statement in the `Block`.
///
/// # Arguments
///
/// * `source` - The source string
/// * `block` - The `Block`
///
/// # Returns
///
/// The index of the next character immediately after the last statement in the `Block`.
pub fn find_next_index_of_last_statement_in_block(source: &str, block: &Block) -> Option<usize> {
    block.statements.last().map_or(
        // if the block has no statements, the index of the last statement is the end of the block
        // '}'
        find_next_index_of_source_location(&block.src).map(|s| s - 1),
        |stmt| find_next_index_of_statement(source, stmt),
    )
}

/// Find the index of the next character immediately after the `Statement`.
///
/// # Arguments
///
/// * `source` - The source string
/// * `stmt` - The `Statement`
///
/// # Returns
///
/// The index of the next character immediately after the `Statement`.
pub fn find_next_index_of_statement(source: &str, stmt: &Statement) -> Option<usize> {
    match stmt {
        Statement::Block(block) => find_next_index_of_source_location(&block.src),
        Statement::Break(break_stmt) => {
            find_next_semicolon_after_source_location(source, &break_stmt.src).map(|i| i + 1)
        }
        Statement::Continue(continue_stmt) => {
            find_next_semicolon_after_source_location(source, &continue_stmt.src).map(|i| i + 1)
        }
        Statement::DoWhileStatement(do_while_statement) => {
            find_next_index_of_source_location(&do_while_statement.src)
        }
        Statement::EmitStatement(emit_statement) => {
            find_next_semicolon_after_source_location(source, &emit_statement.src).map(|i| i + 1)
        }
        Statement::ExpressionStatement(expression_statement) => {
            find_next_semicolon_after_source_location(source, &expression_statement.src)
                .map(|i| i + 1)
        }
        Statement::ForStatement(for_statement) => {
            find_next_index_of_block_or_statement(source, &for_statement.body)
        }
        Statement::IfStatement(if_statement) => match &if_statement.false_body {
            Some(false_body) => find_next_index_of_block_or_statement(source, false_body),
            None => find_next_index_of_block_or_statement(source, &if_statement.true_body),
        },
        Statement::InlineAssembly(inline_assembly) => {
            find_next_index_of_source_location(&inline_assembly.src)
        }
        Statement::PlaceholderStatement(placeholder_statement) => {
            find_next_semicolon_after_source_location(source, &placeholder_statement.src)
                .map(|i| i + 1)
        }
        Statement::Return(return_stmt) => {
            let return_str = source_string_at_location_unchecked(source, &return_stmt.src);
            if return_str.trim_end().ends_with(";") {
                find_next_index_of_source_location(&return_stmt.src)
            } else {
                find_next_semicolon_after_source_location(source, &return_stmt.src).map(|i| i + 1)
            }
        }
        Statement::RevertStatement(revert_statement) => {
            find_next_semicolon_after_source_location(source, &revert_statement.src).map(|i| i + 1)
        }
        Statement::TryStatement(try_statement) => {
            find_next_index_of_source_location(&try_statement.src)
        }
        Statement::UncheckedBlock(unchecked_block) => {
            find_next_index_of_source_location(&unchecked_block.src)
        }
        Statement::VariableDeclarationStatement(variable_declaration_statement) => {
            find_next_semicolon_after_source_location(source, &variable_declaration_statement.src)
                .map(|i| i + 1)
        }
        Statement::WhileStatement(while_statement) => {
            find_next_index_of_block_or_statement(source, &while_statement.body)
        }
    }
}

/// Check recursively if the type name contains a user defined type or a function type.
///
/// # Arguments
///
/// * `type_name` - The type name
///
/// # Returns
///
/// True if the type name contains a user defined type or a function type.
///
/// # Example
///
/// ```rust
/// let type_name = TypeName::UserDefinedTypeName("MyType".to_string());
/// let contains = contains_user_defined_type_or_function_type(&type_name);
/// assert!(contains);
/// ```
pub fn contains_user_defined_type(type_name: &TypeName) -> bool {
    match type_name {
        TypeName::ArrayTypeName(array_type_name) => {
            contains_user_defined_type(&array_type_name.base_type)
        }
        TypeName::ElementaryTypeName(_) => false,
        TypeName::FunctionTypeName(_) => false,
        TypeName::Mapping(mapping) => {
            contains_user_defined_type(&mapping.key_type)
                || contains_user_defined_type(&mapping.value_type)
        }
        TypeName::UserDefinedTypeName(_) => true,
    }
}

/// Check recursively if the type name contains a function type.
///
/// # Arguments
///
/// * `type_name` - The type name
///
/// # Returns
///
/// True if the type name contains a function type.
///
/// # Example
///
/// ```rust
/// let type_name = TypeName::FunctionTypeName("MyFunction".to_string());
/// let contains = contains_function_type(&type_name);
/// assert!(contains);
/// ```
pub fn contains_function_type(type_name: &TypeName) -> bool {
    match type_name {
        TypeName::FunctionTypeName(_) => true,
        TypeName::ArrayTypeName(array_type_name) => {
            contains_function_type(&array_type_name.base_type)
        }
        TypeName::ElementaryTypeName(_) => false,
        TypeName::Mapping(mapping) => {
            contains_function_type(&mapping.key_type) || contains_function_type(&mapping.value_type)
        }
        TypeName::UserDefinedTypeName(_) => false,
    }
}

/// Check recursively if the type name contains a mapping type.
///
/// # Arguments
///
/// * `type_name` - The type name
///
/// # Returns
///
/// True if the type name contains a mapping type.
pub fn contains_mapping_type(type_name: &TypeName) -> bool {
    match type_name {
        TypeName::Mapping(_) => true,
        TypeName::ArrayTypeName(array_type_name) => {
            contains_mapping_type(&array_type_name.base_type)
        }
        TypeName::ElementaryTypeName(_) => false,
        TypeName::FunctionTypeName(_) => false,
        // TODO: the user defined type may have a mapping type, this need to be inspected in the
        // future
        TypeName::UserDefinedTypeName(_) => false,
    }
}

/// Check if the abi.encode function (which is available since solidity 0.4.24) is available in the
/// given version requirement.
///
/// # Arguments
///
/// * `version_req` - The version requirement
///
/// # Returns
///
/// True if the abi.encode function is available in the given version requirement.
///
/// # Example
///
/// ```rust
/// let version_req = VersionReq::parse("^0.4.24").unwrap();
/// let available = abi_encode_available(&version_req);
/// assert!(available);
/// ```
pub fn abi_encode_available(version_req: &VersionReq) -> bool {
    // abi.encode function is available since solidity 0.4.24
    let min_version = semver::Version::parse("0.4.24").unwrap();

    // If any version < 0.4.24 satisfies the requirement, then abi.encode is not available
    !version_req.comparators.iter().all(|cmp| allows_any_version_lt(&min_version, cmp))
}

/// Check if a comparator allows any version less than the given minimum version.
fn allows_any_version_lt(min_version: &semver::Version, comparator: &semver::Comparator) -> bool {
    use semver::Op;

    match comparator.op {
        Op::Exact | Op::Greater | Op::GreaterEq | Op::Tilde | Op::Caret | Op::Wildcard => {
            // Exact match: check if the exact version is < min_version
            let exact_version = semver::Version {
                major: comparator.major,
                minor: comparator.minor.unwrap_or(0),
                patch: comparator.patch.unwrap_or(0),
                pre: semver::Prerelease::EMPTY,
                build: semver::BuildMetadata::EMPTY,
            };
            exact_version < *min_version
        }
        Op::Less | Op::LessEq => true,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::VersionReq;

    #[test]
    fn test_abi_encode_available_exact_versions() {
        // Test exact versions that should return true (>= 0.4.24, so no versions < 0.4.24 allowed)
        assert!(abi_encode_available(&VersionReq::parse("=0.4.24").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("=0.4.25").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("=0.5.0").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("=0.5.10").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("=0.6.0").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("=0.7.0").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("=0.8.0").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("=0.8.19").unwrap()));

        // Test exact versions that should return false (< 0.4.24, so versions < 0.4.24 are allowed)
        assert!(!abi_encode_available(&VersionReq::parse("=0.4.23").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("=0.4.20").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("=0.3.0").unwrap()));
    }

    #[test]
    fn test_abi_encode_available_greater_than() {
        // Test greater than versions - these should return true because they don't allow versions <
        // 0.4.24
        assert!(!abi_encode_available(&VersionReq::parse(">0.4.23").unwrap())); // allows 0.4.24+, no < 0.4.24
        assert!(!abi_encode_available(&VersionReq::parse(">0.4.20").unwrap())); // allows 0.4.21+, no < 0.4.24
        assert!(!abi_encode_available(&VersionReq::parse(">0.3.0").unwrap())); // allows 0.3.1+, no < 0.4.24
        assert!(!abi_encode_available(&VersionReq::parse(">0.1.0").unwrap())); // allows 0.1.1+, no < 0.4.24

        // These should return true because they don't allow any version < 0.4.24
        assert!(abi_encode_available(&VersionReq::parse(">0.4.24").unwrap())); // only allows 0.4.25+, no < 0.4.24
        assert!(abi_encode_available(&VersionReq::parse(">0.4.25").unwrap())); // only allows 0.4.26+, no < 0.4.24
        assert!(abi_encode_available(&VersionReq::parse(">0.5.0").unwrap())); // only allows 0.5.1+, no < 0.4.24
    }

    #[test]
    fn test_abi_encode_available_greater_equal() {
        // Test greater than or equal versions - these should return true because they don't allow
        // versions < 0.4.24
        assert!(abi_encode_available(&VersionReq::parse(">=0.4.24").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse(">=0.4.25").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse(">=0.5.0").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse(">=0.6.0").unwrap()));

        // These should return false because the lower bound is < 0.4.24, so versions < 0.4.24 are
        // allowed
        assert!(!abi_encode_available(&VersionReq::parse(">=0.4.23").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse(">=0.4.20").unwrap()));
    }

    #[test]
    fn test_abi_encode_available_less_than() {
        // Test less than versions - these should return false because they allow versions < 0.4.24
        assert!(!abi_encode_available(&VersionReq::parse("<0.4.24").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("<0.4.23").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("<0.3.0").unwrap()));

        // These should return false because they still allow versions < 0.4.24
        assert!(!abi_encode_available(&VersionReq::parse("<0.4.25").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("<0.5.0").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("<0.6.0").unwrap()));
    }

    #[test]
    fn test_abi_encode_available_less_equal() {
        // Test less than or equal versions - these should return false because they allow versions
        // < 0.4.24
        assert!(!abi_encode_available(&VersionReq::parse("<=0.4.24").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("<=0.4.25").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("<=0.5.0").unwrap()));

        // These should return false because the upper bound is < 0.4.24, so versions < 0.4.24 are
        // allowed
        assert!(!abi_encode_available(&VersionReq::parse("<=0.4.23").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("<=0.4.20").unwrap()));
    }

    #[test]
    fn test_abi_encode_available_tilde() {
        // Test tilde versions - these should return true because they don't allow versions < 0.4.24
        assert!(abi_encode_available(&VersionReq::parse("~0.4.24").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("~0.4.25").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("~0.5.0").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("~0.5.1").unwrap()));

        // These should return false because the lower bound is < 0.4.24, so versions < 0.4.24 are
        // allowed
        assert!(!abi_encode_available(&VersionReq::parse("~0.4.23").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("~0.4.20").unwrap()));

        // Test tilde with only major version - these should return false because they allow
        // versions < 0.4.24
        assert!(!abi_encode_available(&VersionReq::parse("~0").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("~0.3").unwrap()));
    }

    #[test]
    fn test_abi_encode_available_caret() {
        // Test caret versions - these should return true because they don't allow versions < 0.4.24
        assert!(abi_encode_available(&VersionReq::parse("^0.4.24").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("^0.4.25").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("^0.5.0").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("^0.6.0").unwrap()));

        // These should return false because the lower bound is < 0.4.24, so versions < 0.4.24 are
        // allowed
        assert!(!abi_encode_available(&VersionReq::parse("^0.4.23").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("^0.4.20").unwrap()));

        // Test caret with only major version - these should return false because they allow
        // versions < 0.4.24
        assert!(!abi_encode_available(&VersionReq::parse("^0").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("^0.3").unwrap()));
    }

    #[test]
    fn test_abi_encode_available_wildcard() {
        // Test wildcard versions - these should return true because they don't allow versions <
        // 0.4.24
        assert!(abi_encode_available(&VersionReq::parse("0.4.24").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("0.4.25").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse("0.5.0").unwrap()));

        // These should return false because they're < 0.4.24, so versions < 0.4.24 are allowed
        assert!(!abi_encode_available(&VersionReq::parse("0.4.23").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse("0.4.20").unwrap()));
    }

    #[test]
    fn test_abi_encode_available_complex_ranges() {
        // Test complex version ranges - these should return true because they don't allow versions
        // < 0.4.24
        assert!(abi_encode_available(&VersionReq::parse(">=0.4.24, <0.9.0").unwrap()));
        assert!(abi_encode_available(&VersionReq::parse(">=0.5.0, <0.8.0").unwrap()));

        // These should return false because they allow versions < 0.4.24
        assert!(!abi_encode_available(&VersionReq::parse(">=0.4.20, <0.4.24").unwrap()));
        assert!(!abi_encode_available(&VersionReq::parse(">=0.3.0, <0.4.24").unwrap()));
    }

    #[test]
    fn test_abi_encode_available_edge_cases() {
        // Test edge cases around 0.4.24
        assert!(abi_encode_available(&VersionReq::parse(">=0.4.24").unwrap())); // doesn't allow < 0.4.24
        assert!(abi_encode_available(&VersionReq::parse(">0.4.24").unwrap())); // doesn't allow < 0.4.24
        assert!(!abi_encode_available(&VersionReq::parse("<=0.4.24").unwrap())); // allows < 0.4.24
        assert!(!abi_encode_available(&VersionReq::parse("<0.4.24").unwrap())); // allows < 0.4.24

        // Test with pre-release versions (should be handled correctly)
        assert!(abi_encode_available(&VersionReq::parse(">=0.4.24-alpha").unwrap()));
    }
}
