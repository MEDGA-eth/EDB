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

//! Variable scope analysis and representation.
//!
//! This module provides data structures for managing variable scopes during
//! smart contract analysis, including scope hierarchies and variable visibility.

use foundry_compilers::artifacts::ast::SourceLocation;
use serde::{Deserialize, Serialize};

use crate::analysis::{Analyzer, SourceRange, VariableRef, macros::define_ref};

define_ref! {
    /// A reference-counted pointer to a VariableScope.
    VariableScopeRef(VariableScope) {
        cached_field: {
            children: Vec<VariableScopeRef>,
            declared_variables: Vec<VariableRef>,
        }
        delegate: {
            /// Returns the source location of this scope's AST node.
            fn src(&self) -> SourceRange;
        }
        additional_cache: {
            variables_recursive: Vec<VariableRef>,
        }
    }
}

/* Cached read methods */
impl VariableScopeRef {
    /// Returns all variables in this scope and its parent scopes recursively. The variables are
    /// cached.
    pub fn variables_recursive(&self) -> &Vec<VariableRef> {
        self.variables_recursive.get_or_init(|| {
            let mut variables = self.read().declared_variables.clone();
            variables.extend(
                self.inner
                    .read()
                    .parent
                    .as_ref()
                    .map_or(vec![], |parent| parent.variables_recursive().clone()),
            );
            variables
        })
    }
}

/// Represents the scope and visibility information for a variable.
///
/// This structure contains information about where a variable is defined
/// and how it can be accessed. Currently, this is a placeholder structure
/// that can be extended with additional scope-related information as needed.
///
/// # Future Extensions
///
/// This structure may be extended to include:
/// - Function scope information
/// - Contract scope information
/// - Visibility modifiers (public, private, internal, external)
/// - Storage location (storage, memory, calldata)
#[derive(Clone, Serialize, Deserialize, derive_more::Debug)]
#[non_exhaustive]
pub struct VariableScope {
    /// The source location of this scope
    pub src: SourceRange,
    /// Variables declared in this scope, mapped by their UVID
    pub declared_variables: Vec<VariableRef>,
    /// Parent scope
    pub parent: Option<VariableScopeRef>,
    /// Child scopes contained within this scope
    pub children: Vec<VariableScopeRef>,
}

impl VariableScope {
    /// Returns the source location of this scope.
    pub fn src(&self) -> SourceRange {
        self.src
    }

    /// Returns all variables in this scope and its parent scopes recursively. The variables are not
    /// cached.
    pub fn variables_recursive(&self) -> Vec<VariableRef> {
        let mut variables = self.declared_variables.clone();
        variables.extend(
            self.parent.clone().map_or(vec![], |parent| parent.read().variables_recursive()),
        );
        variables
    }
}

/* Scope management methods */
impl Analyzer {
    /// Returns the current scope from the scope stack.
    ///
    /// # Panics
    ///
    /// Panics if the scope stack is empty.
    pub(super) fn current_scope(&self) -> VariableScopeRef {
        self.scope_stack.last().expect("scope stack is empty").clone()
    }

    /// Enters a new scope by creating a new VariableScope and pushing it onto the scope stack.
    ///
    /// # Arguments
    ///
    /// * `src` - The source location that defines this scope
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    pub(super) fn enter_new_scope(&mut self, src: SourceLocation) -> eyre::Result<()> {
        let new_scope = VariableScope {
            src: src.into(),
            declared_variables: Vec::default(),
            children: vec![],
            parent: self.scope_stack.last().cloned(),
        }
        .into();
        self.scope_stack.push(new_scope);
        Ok(())
    }

    /// Exits the current scope by popping it from the scope stack and adding it as a child
    /// to its parent scope.
    ///
    /// # Arguments
    ///
    /// * `src` - The source location of the scope being exited, used for validation
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Panics
    ///
    /// Panics if the source location doesn't match the current scope's location.
    pub(super) fn exit_current_scope(&mut self, src: SourceLocation) -> eyre::Result<()> {
        assert_eq!(
            self.current_scope().src(),
            src.into(),
            "scope mismatch: the post-visit block's source location does not match the current scope's location"
        );
        // close the scope
        let closed_scope = self.scope_stack.pop().expect("scope stack is empty");
        if let Some(parent) = self.scope_stack.last_mut() {
            parent.write().children.push(closed_scope);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::analysis::analyzer::tests::compile_and_analyze;

    #[test]
    fn test_scope_hierarchy_simple_function() {
        // Test basic scope hierarchy in a simple function
        let source = r#"
contract TestContract {
    function foo() public {
        uint256 x = 1;
        {
            uint256 y = 2;
        }
        uint256 z = 3;
    }
}
"#;

        let (_sources, analysis) = compile_and_analyze(source);

        // Should have: source unit scope, contract scope, function scope, and a block scope
        // Get the function
        let functions = &analysis.functions;
        assert_eq!(functions.len(), 1, "Should have one function");

        let function = &functions[0];
        let function_scope = &function.scope();

        // Function scope should have children (the inner block)
        assert!(!function_scope.children().is_empty(), "Function scope should have child scopes");

        // Variables should be in the correct scopes
        let func_variables = function_scope.declared_variables();
        assert_eq!(func_variables.len(), 2, "Function scope should have 2 variables (x and z)");

        // Inner block should have 1 variable
        let inner_block_scope = &function_scope.children()[0];
        let block_variables = inner_block_scope.declared_variables();
        assert_eq!(block_variables.len(), 1, "Inner block scope should have 1 variable (y)");
    }

    #[test]
    fn test_scope_variables_recursive() {
        // Test that variables_recursive correctly accumulates variables from parent scopes
        let source = r#"
contract TestContract {
    uint256 stateVar;

    function foo() public {
        uint256 x = 1;
        {
            uint256 y = 2;
            {
                uint256 z = 3;
            }
        }
    }
}
"#;

        let (_sources, analysis) = compile_and_analyze(source);

        let functions = &analysis.functions;
        let function = &functions[0];
        let function_scope = &function.scope();

        // Get the innermost block scope (z's scope)
        let outer_block = &function_scope.children()[0];
        let inner_block = &outer_block.children()[0];

        // The innermost scope should see all variables: stateVar (from contract), x (from
        // function), y (from outer block), z (from inner block)
        let all_vars = inner_block.variables_recursive();
        assert!(
            all_vars.len() >= 3,
            "Innermost scope should see at least 3 variables (x, y, z). Got: {}",
            all_vars.len()
        );

        // Verify the variables are accessible
        let var_names: Vec<_> = all_vars.iter().map(|v| v.name().to_string()).collect();
        assert!(var_names.contains(&"x".to_string()), "Should see variable x");
        assert!(var_names.contains(&"y".to_string()), "Should see variable y");
        assert!(var_names.contains(&"z".to_string()), "Should see variable z");
    }

    #[test]
    fn test_scope_for_statement() {
        // Test that for statements create their own scope
        let source = r#"
contract TestContract {
    function foo() public {
        for (uint256 i = 0; i < 10; i++) {
            uint256 x = i;
        }
    }
}
"#;

        let (_sources, analysis) = compile_and_analyze(source);

        let functions = &analysis.functions;
        let function = &functions[0];
        let function_scope = &function.scope();

        // For statement should create a child scope
        assert!(!function_scope.children().is_empty(), "Function scope should have for-loop scope");

        let for_scope = &function_scope.children()[0];

        // For scope should have the loop variable 'i'
        let for_vars = for_scope.declared_variables();
        assert_eq!(for_vars.len(), 1, "For-loop scope should have 1 variable (i)");
        assert_eq!(for_vars[0].read().name(), "i", "For-loop variable should be 'i'");

        // Inner block should have variable 'x'
        assert!(!for_scope.children().is_empty(), "For-loop scope should have block scope");
        let block_scope = &for_scope.children()[0];
        let block_vars = block_scope.declared_variables();
        assert_eq!(block_vars.len(), 1, "Block scope should have 1 variable (x)");
        assert_eq!(block_vars[0].read().name(), "x", "Block variable should be 'x'");
    }

    #[test]
    fn test_scope_nested_functions() {
        // Test scopes in multiple functions
        let source = r#"
contract TestContract {
    function foo() public {
        uint256 x = 1;
    }

    function bar() public {
        uint256 y = 2;
    }
}
"#;

        let (_sources, analysis) = compile_and_analyze(source);

        assert_eq!(analysis.functions.len(), 2, "Should have 2 functions");

        // Each function should have its own scope
        for function in &analysis.functions {
            let scope = function.scope();
            let scope_vars = scope.declared_variables();
            assert_eq!(scope_vars.len(), 1, "Each function should have 1 variable");
        }
    }

    #[test]
    fn test_scope_modifier() {
        // Test that modifiers create their own scope
        let source = r#"
contract TestContract {
    modifier onlyPositive(uint256 value) {
        uint256 x = value;
        _;
    }

    function foo() public onlyPositive(10) {
        uint256 y = 1;
    }
}
"#;

        let (_sources, analysis) = compile_and_analyze(source);

        // Should have both function and modifier
        assert!(analysis.functions.len() >= 2, "Should have function and modifier");

        // Find the modifier
        let modifier =
            analysis.functions.iter().find(|f| f.is_modifier()).expect("Should have modifier");

        // Modifier should have its own scope with parameter and local variable
        let modifier_scope = &modifier.scope();
        let modifier_vars = modifier_scope.declared_variables();

        // Should have at least the local variable 'x' and parameter 'value'
        assert!(modifier_vars.len() >= 2, "Modifier should have at least 2 declared variables");
    }

    #[test]
    fn test_scope_unchecked_block() {
        // Test that unchecked blocks create their own scope
        let source = r#"
contract TestContract {
    function foo() public {
        uint256 x = 1;
        unchecked {
            uint256 y = 2;
        }
    }
}
"#;

        let (_sources, analysis) = compile_and_analyze(source);

        let functions = &analysis.functions;
        let function = &functions[0];
        let function_scope = &function.scope();

        // Function should have variable x
        let func_vars = function_scope.declared_variables();
        assert_eq!(func_vars.len(), 1, "Function scope should have 1 variable (x)");

        // Unchecked block should create a child scope with variable y
        assert!(
            !function_scope.children().is_empty(),
            "Function scope should have unchecked block scope"
        );
        let unchecked_scope = &function_scope.children()[0];
        let unchecked_vars = unchecked_scope.declared_variables();
        assert_eq!(unchecked_vars.len(), 1, "Unchecked block should have 1 variable (y)");
    }

    #[test]
    fn test_scope_parent_relationship() {
        // Test parent-child relationships
        let source = r#"
contract TestContract {
    function foo() public {
        uint256 x = 1;
        {
            uint256 y = 2;
        }
    }
}
"#;

        let (_sources, analysis) = compile_and_analyze(source);

        let functions = &analysis.functions;
        let function = &functions[0];
        let function_scope = &function.scope();

        // Get the inner block scope
        let block_scope = &function_scope.children()[0];

        // Block scope should have a parent
        assert!(block_scope.read().parent.is_some(), "Block scope should have a parent");

        // Parent should be the function scope
        let parent = block_scope.read().parent.clone().unwrap();
        assert_eq!(
            parent.src(),
            function_scope.src(),
            "Block's parent should be the function scope"
        );
    }

    #[test]
    fn test_scope_multiple_nested_blocks() {
        // Test deeply nested scopes
        let source = r#"
contract TestContract {
    function foo() public {
        uint256 a = 1;
        {
            uint256 b = 2;
            {
                uint256 c = 3;
                {
                    uint256 d = 4;
                }
            }
        }
    }
}
"#;

        let (_sources, analysis) = compile_and_analyze(source);

        let functions = &analysis.functions;
        let function = &functions[0];
        let function_scope = &function.scope();

        // Navigate through nested scopes
        let level1 = &function_scope.children()[0];
        assert!(!level1.children().is_empty(), "Level 1 should have children");

        let level2 = &level1.children()[0];
        assert!(!level2.children().is_empty(), "Level 2 should have children");

        let level3 = &level2.children()[0];

        // Level 3 should have variable 'd'
        let level3_vars = level3.declared_variables();
        assert_eq!(level3_vars.len(), 1, "Level 3 should have 1 variable");
        assert_eq!(level3_vars[0].read().name(), "d", "Level 3 variable should be 'd'");

        // Level 3 should see all variables through recursion
        let all_vars = level3.variables_recursive();
        let var_names: Vec<_> = all_vars.iter().map(|v| v.read().name()).collect();

        assert!(var_names.contains(&"a".to_string()), "Should see variable a");
        assert!(var_names.contains(&"b".to_string()), "Should see variable b");
        assert!(var_names.contains(&"c".to_string()), "Should see variable c");
        assert!(var_names.contains(&"d".to_string()), "Should see variable d");
    }
}
