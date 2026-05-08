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

//! Variable analysis and representation for Ethereum smart contract analysis.
//!
//! This module provides the core data structures and utilities for representing
//! and tracking variables during smart contract analysis. It includes:
//!
//! - **UVID (Universal Variable Identifier)**: A unique identifier system for tracking variables
//!   across different scopes and contexts
//! - **Variable**: The main data structure representing a smart contract variable
//! - **VariableType**: Enumeration of supported Solidity variable types
//! - **VariableScope**: Structure for managing variable scope information
//!
//! The module is designed to work with the broader analysis framework to provide
//! comprehensive variable tracking and type information during contract analysis.

use foundry_compilers::artifacts::{
    Assignment, Expression, Mutability, StorageLocation, TypeName, VariableDeclaration, Visibility,
};
use serde::{Deserialize, Serialize};

use crate::{
    analysis::{
        Analyzer, ContractRef, FunctionRef,
        macros::{define_ref, universal_id},
    },
    utils::{VisitorAction, contains_user_defined_type},
};

// use crate::{
//     // Visitor, Walk
// };

/// The slot where the `edb_runtime_values` mapping is stored.
///
/// This constant represents the first 8 bytes of the keccak256 hash of the string
/// "EDB_RUNTIME_VALUE_OFFSET". It serves as the starting point for UVID generation
/// to ensure unique identifier spaces across different analysis contexts.
pub const EDB_RUNTIME_VALUE_OFFSET: u64 = 0x234c6dfc3bf8fed1;

universal_id! {
    /// A Universal Variable Identifier (UVID) is a unique identifier for a variable in a contract.
    ///
    /// UVIDs provide a way to uniquely identify variables across different scopes,
    /// contexts, and analysis passes. They are used internally by the analysis engine
    /// to track variable relationships and dependencies.
    ///
    /// UVID is also the storage slot that a variable should be stored in storage during debugging. UVID starts from `EDB_RUNTIME_VALUE_OFFSET`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use edb::analysis::variable::{UVID, UVID::next};
    ///
    /// let uvid1 = UVID::next();
    /// let uvid2 = UVID::next();
    /// assert_ne!(uvid1, uvid2);
    /// ```
    UVID => EDB_RUNTIME_VALUE_OFFSET
}

/// Represents the kind/category of a variable.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VariableKind {
    /// Local variable declared in function body
    Local,
    /// State variable
    State,
    /// Function parameter (input)
    Param,
    /// Named return variable
    Return,
}

define_ref! {
    /// A reference-counted pointer to a Variable.
    ///
    /// This type alias provides shared ownership of Variable instances, allowing
    /// multiple parts of the analysis system to reference the same variable
    /// without copying the data.
    #[allow(unused)]
    VariableRef(Variable) {
        cached_method: {
            name: String,
            declaration: VariableDeclaration,
            type_name: Option<TypeName>,
            initialized: bool,
        }
        delegate: {
            fn id(&self) -> UVID;
            fn kind(&self) -> VariableKind;
            fn contract(&self) -> Option<ContractRef>;
            fn function(&self) -> Option<FunctionRef>;
        }
    }
}

#[allow(unused)]
impl VariableRef {
    /// Returns the base variable of this variable.
    pub fn base(&self) -> Self {
        let inner = self.inner.read();
        if let Some(base) = inner.base() { base } else { self.clone() }
    }
}

/// Represents a variable in a smart contract with its metadata and type information.
///
/// Currently, only local variables are supported.
///
/// The Variable struct contains all the information needed to track and analyze
/// a variable during contract analysis, including its unique identifier, name,
/// declaration details, type, and scope information.
///
/// # Examples
///
/// ```rust
/// use edb::analysis::variable::{UVID, Variable, VariableScope, VariableType};
/// use foundry_compilers::artifacts::VariableDeclaration;
///
/// let variable = Variable {
///     uvid: UVID(1),
///     name: "balance".to_string(),
///     declare: VariableDeclaration::default(),
///     ty: VariableType::Uint(256),
///     scope: VariableScope {},
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
#[allow(clippy::large_enum_variant)]
pub enum Variable {
    /// A plain variable with a direct declaration.
    Plain {
        /// The unique variable identifier.
        uvid: UVID,
        /// The name of the variable.
        name: String,
        /// The variable declaration from the AST.
        declaration: VariableDeclaration,
        /// Whether the variable is initialized at declaration.
        initialized: bool,
        /// The kind of variable (local, state, etc.).
        kind: VariableKind,
        /// Function that this variable is declared in.
        function: Option<FunctionRef>,
        /// Contract that this variable is declared in.
        contract: Option<ContractRef>,
    },
    /// A member access variable (e.g., `obj.field`).
    Member {
        /// The base variable being accessed.
        base: VariableRef,
        /// The name of the member being accessed.
        member: String,
    },
    /// An array or mapping index access variable (e.g., `arr[index]`).
    Index {
        /// The base variable being indexed.
        base: VariableRef,
        /// The index expression.
        index: Expression,
    },
    /// An array slice access variable (e.g., `arr[start:end]`).
    IndexRange {
        /// The base variable being sliced.
        base: VariableRef,
        /// The start index expression.
        start: Option<Expression>,
        /// The end index expression.
        end: Option<Expression>,
    },
}

impl Variable {
    /// Returns the unique identifier of this variable.
    pub fn id(&self) -> UVID {
        match self {
            Self::Plain { uvid, .. } => *uvid,
            Self::Member { base, .. } => base.read().id(),
            Self::Index { base, .. } => base.read().id(),
            Self::IndexRange { base, .. } => base.read().id(),
        }
    }

    /// Returns whether the variable is initialized at declaration.
    pub fn initialized(&self) -> bool {
        match self {
            Self::Plain { initialized, .. } => *initialized,
            Self::Member { base, .. } => base.read().initialized(),
            Self::Index { base, .. } => base.read().initialized(),
            Self::IndexRange { base, .. } => base.read().initialized(),
        }
    }

    /// Returns the name of this variable.
    pub fn name(&self) -> String {
        match self {
            Self::Plain { name, .. } => name.clone(),
            Self::Member { base, .. } => base.read().name(),
            Self::Index { base, .. } => base.read().name(),
            Self::IndexRange { base, .. } => base.read().name(),
        }
    }

    /// Returns the kind of this variable.
    pub fn kind(&self) -> VariableKind {
        match self {
            Self::Plain { kind, .. } => *kind,
            Self::Member { base, .. } => base.read().kind(),
            Self::Index { base, .. } => base.read().kind(),
            Self::IndexRange { base, .. } => base.read().kind(),
        }
    }

    /// Returns the type name of this variable.
    pub fn type_name(&self) -> Option<TypeName> {
        self.declaration().type_name
    }

    /// Returns the declaration of this variable.
    pub fn declaration(&self) -> VariableDeclaration {
        match self {
            Self::Plain { declaration, .. } => declaration.clone(),
            Self::Member { base, .. } => base.read().declaration(),
            Self::Index { base, .. } => base.read().declaration(),
            Self::IndexRange { base, .. } => base.read().declaration(),
        }
    }

    /// Returns the function of this variable.
    pub fn function(&self) -> Option<FunctionRef> {
        match self {
            Self::Plain { function, .. } => function.clone(),
            Self::Member { base, .. } => base.read().function(),
            Self::Index { base, .. } => base.read().function(),
            Self::IndexRange { base, .. } => base.read().function(),
        }
    }

    /// Returns the contract of this variable.
    pub fn contract(&self) -> Option<ContractRef> {
        match self {
            Self::Plain { contract, .. } => contract.clone(),
            Self::Member { base, .. } => base.read().contract(),
            Self::Index { base, .. } => base.read().contract(),
            Self::IndexRange { base, .. } => base.read().contract(),
        }
    }

    /// Returns the base variable of this variable.
    pub fn base(&self) -> Option<VariableRef> {
        match self {
            Self::Plain { .. } => None,
            Self::Member { base, .. }
            | Self::Index { base, .. }
            | Self::IndexRange { base, .. } => {
                if let Some(base) = base.read().base() {
                    Some(base)
                } else {
                    Some(base.clone())
                }
            }
        }
    }
}

/* Variable analysis utils */
impl Analyzer {
    pub(super) fn declare_variable(
        &mut self,
        declaration: &VariableDeclaration,
    ) -> eyre::Result<()> {
        if declaration.name.is_empty() {
            // if a variable has no name, we skip the variable declaration
            return Ok(());
        }
        if declaration.mutability == Some(Mutability::Immutable)
            || declaration.mutability == Some(Mutability::Constant)
            || declaration.constant
        {
            // constant and immutable variables are excluded.
            return Ok(());
        }

        // add a new variable to the current scope
        let scope = self.current_scope();
        let function = self.current_function.clone();
        let contract = self.current_contract.clone();
        let uvid = UVID::next();
        let kind = if declaration.state_variable {
            VariableKind::State
        } else if self.is_declaring_param {
            VariableKind::Param
        } else if self.is_declaring_return {
            VariableKind::Return
        } else {
            VariableKind::Local
        };
        // Whether a variable is initialized so that we can observe the value at declaration.
        let initialized = match kind {
            VariableKind::Param | VariableKind::State => true,
            VariableKind::Local | VariableKind::Return => match declaration.storage_location {
                StorageLocation::Calldata
                | StorageLocation::Transient
                | StorageLocation::Storage => self.is_declaring_with_initial_expr,
                StorageLocation::Default | StorageLocation::Memory => true,
            },
        };
        let variable: VariableRef = Variable::Plain {
            uvid,
            name: declaration.name.clone(),
            declaration: declaration.clone(),
            initialized,
            kind,
            function,
            contract,
        }
        .into();
        self.check_state_variable_visibility(&variable)?;
        if kind == VariableKind::State {
            self.state_variables.push(variable.clone());
        }
        scope.write().declared_variables.push(variable.clone());

        // add the variable to the variable_declarations map
        self.variables.insert(declaration.id, variable.clone());

        if let Some(step) = self.current_step.as_mut() {
            // add the variable to the current step
            step.write().declared_variables.push(variable.clone());
        }
        Ok(())
    }

    fn check_state_variable_visibility(&mut self, variable: &VariableRef) -> eyre::Result<()> {
        let declaration = variable.declaration();
        if declaration.state_variable {
            // FIXME: this is a temporary workaround on user defined struct types.
            // Struct may not be able to be declared as a public state variable.
            // So here when we encounter a state variable with a user defined type, we skip the
            // visibility check. In the future, we may further consider to support user
            // defined struct types as public state variables under the condition that
            // it does not contain inner recursive types (array or mapping fields).
            if declaration.type_name.as_ref().map(contains_user_defined_type).unwrap_or(false) {
                return Ok(());
            }

            // we need to change the visibility of the state variable to public
            if declaration.visibility != Visibility::Public {
                self.private_state_variables.push(variable.clone());
            }
        }
        Ok(())
    }

    pub(super) fn record_assignment(
        &mut self,
        variable: &Assignment,
    ) -> eyre::Result<VisitorAction> {
        fn get_varaiable(this: &Analyzer, expr: &Expression) -> Option<VariableRef> {
            match expr {
                Expression::Identifier(identifier) => {
                    if let Some(declaration_id) = &identifier.referenced_declaration
                        && declaration_id >= &0
                        && let Some(variable) = this.variables.get(&(*declaration_id as usize))
                    {
                        return Some(variable.clone());
                    }
                    None
                }
                Expression::IndexAccess(index_access) => {
                    if let Some(base_variable) = get_varaiable(this, &index_access.base_expression)
                        && let Some(index) = &index_access.index_expression
                    {
                        let var = Variable::Index { base: base_variable, index: index.clone() };
                        return Some(var.into());
                    }
                    None
                }
                Expression::IndexRangeAccess(index_range_access) => {
                    if let Some(base_variable) =
                        get_varaiable(this, &index_range_access.base_expression)
                    {
                        let var = Variable::IndexRange {
                            base: base_variable,
                            start: index_range_access.start_expression.clone(),
                            end: index_range_access.end_expression.clone(),
                        };
                        return Some(var.into());
                    }
                    None
                }
                Expression::MemberAccess(member_access) => {
                    if let Some(base_variable) = get_varaiable(this, &member_access.expression) {
                        let var = Variable::Member {
                            base: base_variable,
                            member: member_access.member_name.clone(),
                        };
                        return Some(var.into());
                    }
                    None
                }
                Expression::TupleExpression(_) => unreachable!(),
                _ => None,
            }
        }

        let updated_variables: Vec<VariableRef> = match &variable.lhs {
            Expression::Identifier(_)
            | Expression::IndexAccess(_)
            | Expression::IndexRangeAccess(_)
            | Expression::MemberAccess(_) => {
                if let Some(var) = get_varaiable(self, &variable.lhs) {
                    vec![var]
                } else {
                    vec![]
                }
            }
            Expression::TupleExpression(tuple_expression) => {
                let mut vars = vec![];
                for comp in tuple_expression.components.iter().flatten() {
                    if let Some(var) = get_varaiable(self, comp) {
                        vars.push(var);
                    }
                }
                vars
            }
            _ => vec![],
        };

        if let Some(step) = self.current_step.as_mut() {
            step.write().updated_variables.extend(updated_variables);
        }
        Ok(VisitorAction::Continue)
    }

    /// Record a declared variable's initial value to the current step's updated variables.
    pub(super) fn record_declared_varaible(
        &mut self,
        declaration: &VariableDeclaration,
    ) -> eyre::Result<()> {
        let Some(step) = self.current_step.as_mut() else {
            return Ok(());
        };
        if declaration.name.is_empty() {
            // if the variable has no name, we skip the variable declaration
            return Ok(());
        }
        let variable: VariableRef = self.variables.get(&declaration.id).unwrap().clone();
        if *variable.initialized() {
            step.write().updated_variables.push(variable);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{EDB_RUNTIME_VALUE_OFFSET, UVID, Variable, VariableKind};
    use crate::{
        analysis::analyzer::tests::compile_and_analyze,
        test_utils::compile_contract_source_to_source_unit,
    };

    macro_rules! count_updated_variables {
        ($analysis:expr) => {
            $analysis.steps.iter().map(|s| s.read().updated_variables.len()).sum::<usize>()
        };
    }

    #[test]
    fn test_variable_assignment() {
        let source = r#"
        contract TestContract {
            struct S {
                uint256 a;
                uint[] b;
                mapping(address => uint256) c;
            }
            S[] internal s;
            function foo(bool b) public {
                uint256 x = 1;
                x = 2;

                s[x].c[msg.sender] = 3;
                s[x].b[0] = 4;
                s[x].a = x;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        // Count updated variables:
        // 1. Function parameter: bool b
        // 2. Local variable declaration: uint256 x = 1
        // 3. Assignment: x = 2
        // 4. Assignment: s[x].c[msg.sender] = 3
        // 5. Assignment: s[x].b[0] = 4
        // 6. Assignment: s[x].a = x
        assert_eq!(count_updated_variables!(analysis), 6);
    }

    #[test]
    fn test_variable_declaration_is_updated() {
        let source = r#"
        contract TestContract {
            function foo() public {
                uint256 x = 1;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        assert_eq!(count_updated_variables!(analysis), 1);
    }

    #[test]
    fn test_variable_accessible() {
        let source = r#"
        contract TestContract {
            uint256[] internal s;
            function foo(bool b) public {
                uint256 x = 1;
                x = 2;

                if (b) {
                    uint y = x;
                    x = 3;
                }

                uint z = s[x];
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // no step should have `z` in its accessible variables
        for step in &analysis.steps {
            assert!(
                !step
                    .read()
                    .accessible_variables
                    .iter()
                    .any(|v| v.read().declaration().name == "z")
            );
        }
    }

    #[test]
    fn test_dyn_abi_using_variable_declaration() {
        let source = r#"
        contract C {
            function f() public {
                address[] memory b = new address[](0);
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        let variables = analysis
            .variable_table()
            .values()
            .map(|v| v.base().declaration().clone())
            .collect::<Vec<_>>();
        let variable = variables.first().unwrap();
        assert_eq!(variable.type_descriptions.type_string.as_ref().unwrap(), "address[]");
    }

    #[test]
    fn test_variable_enum_id_and_name() {
        let source = r#"
        contract TestContract {
            struct Point {
                uint256 x;
                uint256 y;
            }
            Point[] points;

            function foo() public {
                uint256 a = 1;
                points[0].x = 2;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Get the variables
        let var_table = analysis.variable_table();

        // Find variable 'a' (Plain variant)
        let var_a =
            var_table.values().find(|v| v.read().name() == "a").expect("Should find variable 'a'");
        let uvid_a = var_a.read().id();

        // Verify Plain variant returns its own UVID and name
        assert_eq!(var_a.read().name(), "a");

        // Find the state variable 'points' (Plain variant)
        let var_points = var_table
            .values()
            .find(|v| v.read().name() == "points")
            .expect("Should find variable 'points'");
        let uvid_points = var_points.read().id();

        // Verify that different Plain variables have different UVIDs
        assert_ne!(uvid_a, uvid_points);

        // Check updated variables - should include Member and Index variants
        for step in &analysis.steps {
            let updated = step.read().updated_variables.clone();
            for var in updated {
                let var_read = var.read();
                // All variants should return a valid name and UVID
                let _name = var_read.name();
                let _id = var_read.id();

                // For Member/Index variants, they should delegate to their base
                match &*var_read {
                    Variable::Member { base, .. } => {
                        assert_eq!(
                            var_read.id(),
                            base.read().id(),
                            "Member should delegate id() to base"
                        );
                        assert_eq!(
                            var_read.name(),
                            base.read().name(),
                            "Member should delegate name() to base"
                        );
                    }
                    Variable::Index { base, .. } => {
                        assert_eq!(
                            var_read.id(),
                            base.read().id(),
                            "Index should delegate id() to base"
                        );
                        assert_eq!(
                            var_read.name(),
                            base.read().name(),
                            "Index should delegate name() to base"
                        );
                    }
                    Variable::Plain { .. } => {
                        // Plain variant has its own id and name
                    }
                    Variable::IndexRange { base, .. } => {
                        assert_eq!(
                            var_read.id(),
                            base.read().id(),
                            "IndexRange should delegate id() to base"
                        );
                        assert_eq!(
                            var_read.name(),
                            base.read().name(),
                            "IndexRange should delegate name() to base"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn test_variable_ref_base() {
        let source = r#"
        contract TestContract {
            struct Point {
                uint256 x;
                uint256 y;
            }
            Point[] points;

            function foo() public {
                // This creates: points[0].x
                // Index(base: points) -> Member(base: Index(base: points))
                points[0].x = 10;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Find the assignment step
        let assignment_step = analysis
            .steps
            .iter()
            .find(|s| !s.read().updated_variables.is_empty())
            .expect("Should have assignment step");

        let updated_vars = assignment_step.read().updated_variables.clone();
        assert!(!updated_vars.is_empty(), "Should have updated variables");

        // The updated variable should be points[0].x (Member variant)
        for var in updated_vars {
            // Use the base() method from VariableRef
            let base_var = var.base();

            // The base should be a Plain variant with name "points"
            let base_read = base_var.read();
            match &*base_read {
                Variable::Plain { name, .. } => {
                    assert_eq!(name, "points", "Base should be the 'points' variable");
                }
                _ => panic!("Base should be a Plain variant"),
            }

            // Verify that calling base() multiple times gives the same result
            let base_var2 = var.base();
            assert_eq!(base_var.read().id(), base_var2.read().id(), "base() should be idempotent");
        }
    }

    #[test]
    fn test_declare_variable_filtering() {
        let source = r#"
        contract TestContract {
            uint256 constant CONSTANT_VAR = 100;
            uint256 immutable immutableVar;

            constructor() {
                immutableVar = 200;
            }

            function foo() public {
                uint256 normalVar = 1;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let var_table = analysis.variable_table();

        // Check that constant and immutable variables are NOT in the variable table
        assert!(
            !var_table.values().any(|v| v.read().name() == "CONSTANT_VAR"),
            "Constant variables should be filtered out"
        );
        assert!(
            !var_table.values().any(|v| v.read().name() == "immutableVar"),
            "Immutable variables should be filtered out"
        );

        // Check that normal variable IS in the variable table
        assert!(
            var_table.values().any(|v| v.read().name() == "normalVar"),
            "Normal variables should be included"
        );

        // Verify state_variables list doesn't include constant/immutable
        assert!(
            !analysis.state_variables.iter().any(|v| v.read().name() == "CONSTANT_VAR"),
            "Constant variables should not be in state_variables"
        );
        assert!(
            !analysis.state_variables.iter().any(|v| v.read().name() == "immutableVar"),
            "Immutable variables should not be in state_variables"
        );
    }

    #[test]
    fn test_state_variable_visibility() {
        let source = r#"
        contract TestContract {
            uint256 public publicVar;
            uint256 private privateVar;
            uint256 internal internalVar;

            struct MyStruct {
                uint256 value;
            }
            MyStruct userDefinedVar;  // User-defined type (workaround case)
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Check private_state_variables list
        let private_vars: Vec<String> =
            analysis.private_state_variables.iter().map(|v| v.read().name()).collect();

        // Public variable should NOT be in private_state_variables
        assert!(
            !private_vars.contains(&"publicVar".to_string()),
            "Public state variable should not be in private_state_variables"
        );

        // Private variable SHOULD be in private_state_variables
        assert!(
            private_vars.contains(&"privateVar".to_string()),
            "Private state variable should be in private_state_variables"
        );

        // Internal variable SHOULD be in private_state_variables
        assert!(
            private_vars.contains(&"internalVar".to_string()),
            "Internal state variable should be in private_state_variables"
        );

        // User-defined type variable should NOT be in private_state_variables (workaround)
        assert!(
            !private_vars.contains(&"userDefinedVar".to_string()),
            "User-defined type state variable should be skipped (workaround)"
        );
    }

    #[test]
    fn test_index_range_assignment() {
        let source = r#"
        contract TestContract {
            function foo(bytes calldata data) public {
                bytes memory slice;
                // Array slicing is supported for calldata arrays
                slice = data[1:5];
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Since IndexRange is only used on left-hand side of assignments,
        // and Solidity only allows it for calldata arrays on the right side,
        // we just verify the test compiles successfully.
        // The record_assignment function handles IndexRangeAccess, but it may not
        // be commonly used in practice due to Solidity's restrictions.

        // At least verify we have some variables tracked
        let var_table = analysis.variable_table();
        assert!(!var_table.is_empty(), "Should have some variables");
    }

    #[test]
    fn test_tuple_destructuring_assignment() {
        let source = r#"
        contract TestContract {
            function foo() public returns (uint256, uint256) {
                uint256 x;
                uint256 y;
                (x, y) = (10, 20);
                return (x, y);
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Find the tuple assignment step
        let assignment_step = analysis
            .steps
            .iter()
            .find(|s| {
                let updated = s.read().updated_variables.clone();
                updated.len() == 2 // Should have 2 variables updated (x and y)
            })
            .expect("Should find tuple assignment step");

        let updated_vars = assignment_step.read().updated_variables.clone();
        assert_eq!(updated_vars.len(), 2, "Tuple assignment should update 2 variables");

        // Verify both variables are tracked
        let names: Vec<String> = updated_vars.iter().map(|v| v.read().name()).collect();
        assert!(names.contains(&"x".to_string()), "Should track variable 'x'");
        assert!(names.contains(&"y".to_string()), "Should track variable 'y'");
    }

    #[test]
    fn test_uvid_uniqueness() {
        let source = r#"
        contract TestContract {
            uint256 stateVar1;
            uint256 stateVar2;

            function foo() public {
                uint256 localVar1 = 1;
                uint256 localVar2 = 2;
                uint256 localVar3 = 3;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let var_table = analysis.variable_table();
        let uvids: Vec<UVID> = var_table.values().map(|v| v.read().id()).collect();

        // Check all UVIDs are unique
        let unique_uvids: std::collections::HashSet<_> = uvids.iter().collect();
        assert_eq!(uvids.len(), unique_uvids.len(), "All variables should have unique UVIDs");

        // Check UVIDs start from EDB_RUNTIME_VALUE_OFFSET
        for uvid in &uvids {
            assert!(
                uvid.0 >= EDB_RUNTIME_VALUE_OFFSET,
                "UVID should start from EDB_RUNTIME_VALUE_OFFSET"
            );
        }
    }

    #[test]
    fn test_variable_function_and_contract_context() {
        let source = r#"
        contract TestContract {
            uint256 stateVar;

            function foo() public {
                uint256 localVar = 1;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let var_table = analysis.variable_table();

        // Find state variable
        let state_var = var_table
            .values()
            .find(|v| v.read().name() == "stateVar")
            .expect("Should find stateVar");

        // State variable should have contract but NO function
        let state_var_read = state_var.read();
        assert!(state_var_read.contract().is_some(), "State variable should have contract context");
        assert!(
            state_var_read.function().is_none(),
            "State variable should NOT have function context"
        );

        // Verify contract name
        if let Some(contract) = state_var_read.contract() {
            assert_eq!(contract.name(), "TestContract", "Contract should be TestContract");
        }

        // Find local variable
        let local_var = var_table
            .values()
            .find(|v| v.read().name() == "localVar")
            .expect("Should find localVar");

        // Local variable should have BOTH contract and function
        let local_var_read = local_var.read();
        assert!(local_var_read.contract().is_some(), "Local variable should have contract context");
        assert!(local_var_read.function().is_some(), "Local variable should have function context");

        // Verify function name
        if let Some(function) = local_var_read.function() {
            assert_eq!(function.name(), "foo", "Function should be 'foo'");
        }

        // Verify contract name for local variable
        if let Some(contract) = local_var_read.contract() {
            assert_eq!(contract.name(), "TestContract", "Contract should be TestContract");
        }
    }

    #[test]
    fn test_function_arguments() {
        let source = r#"
        contract TestContract {
            function foo(uint256 arg1, address arg2, bool arg3) public returns (uint256) {
                uint256 result = arg1;
                return result;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let var_table = analysis.variable_table();

        // Function arguments should be tracked as variables
        let arg1 =
            var_table.values().find(|v| v.read().name() == "arg1").expect("Should find arg1");
        let arg2 =
            var_table.values().find(|v| v.read().name() == "arg2").expect("Should find arg2");
        let arg3 =
            var_table.values().find(|v| v.read().name() == "arg3").expect("Should find arg3");

        // All arguments should have function context
        assert!(arg1.read().function().is_some(), "arg1 should have function context");
        assert!(arg2.read().function().is_some(), "arg2 should have function context");
        assert!(arg3.read().function().is_some(), "arg3 should have function context");

        // All arguments should have contract context
        assert!(arg1.read().contract().is_some(), "arg1 should have contract context");
        assert!(arg2.read().contract().is_some(), "arg2 should have contract context");
        assert!(arg3.read().contract().is_some(), "arg3 should have contract context");

        // Arguments should be in the function's scope
        let function = analysis
            .functions
            .iter()
            .find(|f| f.name() == "foo")
            .expect("Should find function foo");

        let func_scope = function.scope();
        let func_vars = func_scope.declared_variables();
        let arg_names: Vec<String> = func_vars.iter().map(|v| v.read().name()).collect();

        assert!(arg_names.contains(&"arg1".to_string()), "Function scope should contain arg1");
        assert!(arg_names.contains(&"arg2".to_string()), "Function scope should contain arg2");
        assert!(arg_names.contains(&"arg3".to_string()), "Function scope should contain arg3");
    }

    #[test]
    fn test_function_arguments_in_entry_step() {
        let source = r#"
        contract TestContract {
            function foo(uint256 arg1, address arg2, bool arg3) public returns (uint256) {
                uint256 result = arg1;
                return result;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Find the function entry step
        let entry_step =
            analysis.steps.iter().find(|s| s.is_entry()).expect("Should find function entry step");

        // Check that function arguments appear in updated_variables for the entry step
        let updated_vars = entry_step.read().updated_variables.clone();
        let updated_names: Vec<String> = updated_vars.iter().map(|v| v.read().name()).collect();

        // All function arguments should be marked as updated in the entry step
        assert!(
            updated_names.contains(&"arg1".to_string()),
            "arg1 should be in updated_variables for function entry step"
        );
        assert!(
            updated_names.contains(&"arg2".to_string()),
            "arg2 should be in updated_variables for function entry step"
        );
        assert!(
            updated_names.contains(&"arg3".to_string()),
            "arg3 should be in updated_variables for function entry step"
        );

        // Verify we have exactly 3 updated variables (the 3 arguments)
        assert_eq!(
            updated_vars.len(),
            3,
            "Function entry step should have 3 updated variables (the 3 arguments)"
        );
    }

    #[test]
    fn test_named_return_variables() {
        let source = r#"
        contract TestContract {
            function foo(uint256 input) public returns (uint256 result, bool success) {
                result = input * 2;
                success = true;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let var_table = analysis.variable_table();

        // Named return variables should be tracked as variables
        let result_var = var_table
            .values()
            .find(|v| v.read().name() == "result")
            .expect("Should find named return variable 'result'");
        let success_var = var_table
            .values()
            .find(|v| v.read().name() == "success")
            .expect("Should find named return variable 'success'");

        // Return variables should have function context
        assert!(
            result_var.read().function().is_some(),
            "Return variable should have function context"
        );
        assert!(
            success_var.read().function().is_some(),
            "Return variable should have function context"
        );

        // Return variables should be in the function's scope
        let function = analysis
            .functions
            .iter()
            .find(|f| f.name() == "foo")
            .expect("Should find function foo");

        let func_scope = function.scope();
        let func_vars = func_scope.declared_variables();
        let var_names: Vec<String> = func_vars.iter().map(|v| v.read().name()).collect();

        assert!(
            var_names.contains(&"result".to_string()),
            "Function scope should contain 'result'"
        );
        assert!(
            var_names.contains(&"success".to_string()),
            "Function scope should contain 'success'"
        );

        // Named return variables should be tracked as updated when assigned
        let updated_count = count_updated_variables!(analysis);
        assert!(updated_count >= 2, "Should track assignments to named return variables");
    }

    #[test]
    fn test_unnamed_return_variables() {
        let source = r#"
        contract TestContract {
            function foo(uint256 input) public returns (uint256, bool) {
                return (input * 2, true);
            }

            function bar(uint256 x) public returns (uint256) {
                return x + 1;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let var_table = analysis.variable_table();

        // Unnamed return variables should NOT be in the variable table (filtered by empty name
        // check) Only the function parameters 'input' and 'x' should be tracked
        let var_names: Vec<String> = var_table.values().map(|v| v.read().name()).collect();

        // Should have the input parameters
        assert!(var_names.contains(&"input".to_string()), "Should track parameter 'input'");
        assert!(var_names.contains(&"x".to_string()), "Should track parameter 'x'");

        // Should NOT have any empty-named variables (unnamed returns are filtered)
        for name in &var_names {
            assert!(!name.is_empty(), "Should not have variables with empty names");
        }

        // The variable count should only include parameters (and any internal variables created by
        // the compiler) but definitely not unnamed return variables
        let function_foo = analysis
            .functions
            .iter()
            .find(|f| f.name() == "foo")
            .expect("Should find function foo");

        let foo_scope = function_foo.scope();
        let foo_vars = foo_scope.declared_variables();
        let foo_var_names: Vec<String> = foo_vars.iter().map(|v| v.read().name()).collect();

        // Should have 'input' parameter but no unnamed returns
        assert!(foo_var_names.contains(&"input".to_string()), "foo should have 'input' parameter");
        for name in &foo_var_names {
            assert!(!name.is_empty(), "foo should not have empty-named variables");
        }
    }

    #[test]
    fn test_variable_kind_distinction() {
        let source = r#"
        contract TestContract {
            uint256 stateVar;

            function foo(uint256 param1, address param2) public returns (uint256 ret1, bool ret2) {
                uint256 localVar = param1;
                ret1 = localVar;
                ret2 = true;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let var_table = analysis.variable_table();

        // Find state variable
        let state_var = var_table
            .values()
            .find(|v| v.read().name() == "stateVar")
            .expect("Should find stateVar");
        assert_eq!(state_var.read().kind(), VariableKind::State, "stateVar should be State kind");

        // Find parameters
        let param1 =
            var_table.values().find(|v| v.read().name() == "param1").expect("Should find param1");
        assert_eq!(param1.read().kind(), VariableKind::Param, "param1 should be Param kind");

        let param2 =
            var_table.values().find(|v| v.read().name() == "param2").expect("Should find param2");
        assert_eq!(param2.read().kind(), VariableKind::Param, "param2 should be Param kind");

        // Find return variables
        let ret1 =
            var_table.values().find(|v| v.read().name() == "ret1").expect("Should find ret1");
        assert_eq!(ret1.read().kind(), VariableKind::Return, "ret1 should be Return kind");

        let ret2 =
            var_table.values().find(|v| v.read().name() == "ret2").expect("Should find ret2");
        assert_eq!(ret2.read().kind(), VariableKind::Return, "ret2 should be Return kind");

        // Find local variable
        let local_var = var_table
            .values()
            .find(|v| v.read().name() == "localVar")
            .expect("Should find localVar");
        assert_eq!(local_var.read().kind(), VariableKind::Local, "localVar should be Local kind");
    }

    #[test]
    fn test_uvid_uniqueness_across_files() {
        use crate::analysis::Analyzer;
        use semver::Version;
        use std::path::PathBuf;

        // First source file
        let source1 = r#"
        contract Contract1 {
            uint256 var1;
            uint256 var2;

            function foo() public {
                uint256 local1 = 1;
                uint256 local2 = 2;
            }
        }
        "#;

        // Second source file
        let source2 = r#"
        contract Contract2 {
            uint256 var3;
            uint256 var4;

            function bar() public {
                uint256 local3 = 3;
                uint256 local4 = 4;
            }
        }
        "#;

        // Compile and analyze first file
        let version = Version::parse("0.8.20").unwrap();
        let source_unit1 =
            compile_contract_source_to_source_unit(version.clone(), source1, false).unwrap();
        let analyzer1 = Analyzer::new(0, PathBuf::from("file1.sol"), source1.to_string());
        let analysis1 = analyzer1.analyze(&source_unit1).unwrap();

        // Compile and analyze second file
        let source_unit2 = compile_contract_source_to_source_unit(version, source2, false).unwrap();
        let analyzer2 = Analyzer::new(1, PathBuf::from("file2.sol"), source2.to_string());
        let analysis2 = analyzer2.analyze(&source_unit2).unwrap();

        // Collect all UVIDs from both files
        let mut all_uvids: Vec<UVID> = Vec::new();

        // UVIDs from file 1
        for var in analysis1.variable_table().values() {
            all_uvids.push(var.read().id());
        }

        // UVIDs from file 2
        for var in analysis2.variable_table().values() {
            all_uvids.push(var.read().id());
        }

        // Check all UVIDs are unique across both files
        let unique_uvids: std::collections::HashSet<_> = all_uvids.iter().collect();
        assert_eq!(
            all_uvids.len(),
            unique_uvids.len(),
            "All UVIDs should be unique across multiple source files"
        );

        // Verify we have variables from both files
        assert!(!all_uvids.is_empty(), "Should have UVIDs from both files");
        assert!(all_uvids.len() >= 8, "Should have at least 8 variables (4 from each file)");

        // All UVIDs should still start from EDB_RUNTIME_VALUE_OFFSET
        for uvid in &all_uvids {
            assert!(
                uvid.0 >= EDB_RUNTIME_VALUE_OFFSET,
                "UVID should start from EDB_RUNTIME_VALUE_OFFSET"
            );
        }
    }

    #[test]
    fn test_variable_initialized() {
        let source = r#"
        contract TestContract {
            uint256[] stateVar;
            uint256 transient transientVar;
            function foo(uint param1, uint[] calldata param2) public returns (uint ret1, uint[] calldata ret2) {
                uint256 localVar;
                uint256 localVar2 = 1;
                uint256[] storage storageVar;
                uint256[] storage storageVar2 = stateVar;
                uint256[] memory memoryVar;
                uint256[] calldata calldataVar;
                uint256[] calldata calldataVar2 = param2;
                ret2 = param2;
            }
        }
        "#;

        let (_sources, analysis) = compile_and_analyze(source);

        let var_table = analysis.variable_table();
        let var_name_to_ref =
            var_table.values().map(|v| (v.read().name(), v)).collect::<HashMap<_, _>>();
        assert!(var_name_to_ref["stateVar"].initialized());
        assert!(var_name_to_ref["transientVar"].initialized());
        assert!(var_name_to_ref["param1"].initialized());
        assert!(var_name_to_ref["param2"].initialized());
        assert!(var_name_to_ref["ret1"].initialized());
        assert!(!var_name_to_ref["ret2"].initialized());
        assert!(var_name_to_ref["localVar"].initialized());
        assert!(var_name_to_ref["localVar2"].initialized());
        assert!(!var_name_to_ref["storageVar"].initialized());
        assert!(var_name_to_ref["storageVar2"].initialized());
        assert!(var_name_to_ref["memoryVar"].initialized());
        assert!(!var_name_to_ref["calldataVar"].initialized());
        assert!(var_name_to_ref["calldataVar2"].initialized());
    }

    #[test]
    fn test_struct_field_should_not_be_considered_as_variable() {
        let source = r#"
        contract TestContract {
            struct S {
                uint256 a;
            }
        }
        "#;

        let (_sources, analysis) = compile_and_analyze(source);

        let var_table = analysis.variable_table();
        let var_name_to_ref =
            var_table.values().map(|v| (v.read().name(), v)).collect::<HashMap<_, _>>();
        assert!(!var_name_to_ref.contains_key("a"));
    }

    #[test]
    fn test_event_definition_variables_should_not_be_considered_as_variables() {
        let source = r#"
        contract TestContract {
            event Transfer(address indexed from, address indexed to, uint256 value);
            event Approval(address indexed owner, address indexed spender, uint256 amount);
        }
        "#;

        let (_sources, analysis) = compile_and_analyze(source);

        let var_table = analysis.variable_table();
        let var_name_to_ref =
            var_table.values().map(|v| (v.read().name(), v)).collect::<HashMap<_, _>>();
        assert!(!var_name_to_ref.contains_key("from"));
        assert!(!var_name_to_ref.contains_key("to"));
        assert!(!var_name_to_ref.contains_key("value"));
        assert!(!var_name_to_ref.contains_key("owner"));
        assert!(!var_name_to_ref.contains_key("spender"));
        assert!(!var_name_to_ref.contains_key("amount"));
    }

    #[test]
    fn test_error_definition_variables_should_not_be_considered_as_variables() {
        let source = r#"
        contract TestContract {
            error InsufficientBalance(uint256 available, uint256 required);
            error Unauthorized(address caller);
        }
        "#;

        let (_sources, analysis) = compile_and_analyze(source);

        let var_table = analysis.variable_table();
        let var_name_to_ref =
            var_table.values().map(|v| (v.read().name(), v)).collect::<HashMap<_, _>>();
        assert!(!var_name_to_ref.contains_key("available"));
        assert!(!var_name_to_ref.contains_key("required"));
        assert!(!var_name_to_ref.contains_key("caller"));
    }
}
