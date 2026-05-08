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
    FunctionDefinition, FunctionTypeName, ModifierDefinition, StateMutability, Visibility,
};
use serde::{Deserialize, Serialize};

use crate::{
    analysis::{
        Analyzer, ContractRef, SourceRange, StepRef, VariableScopeRef,
        macros::{define_ref, universal_id},
    },
    utils::VisitorAction,
};

universal_id! {
    /// A Universal Function Identifier (UFID) is a unique identifier for a function in contract execution.
    UFID => 0
}

define_ref! {
    /// A reference-counted pointer to a Function for efficient sharing across multiple contexts.
    ///
    /// This type alias provides thread-safe reference counting for Function instances,
    /// allowing them to be shared between different parts of the analysis system
    /// without copying the entire function data.
    FunctionRef(Function) {
        clone_field: {
            ufid: UFID,
            contract: Option<ContractRef>,
            scope: VariableScopeRef,
        }
        delegate: {
            fn is_modifier(&self) -> bool;
        }
    }
}

impl FunctionRef {
    /// Returns the name of this function.
    pub fn name(&self) -> String {
        self.read().name.clone()
    }

    /// Returns the visibility of this function.
    pub fn visibility(&self) -> Visibility {
        self.read().visibility.clone()
    }

    /// Returns the state mutability of this function.
    pub fn state_mutability(&self) -> Option<StateMutability> {
        self.read().state_mutability.clone()
    }

    /// Returns the source location of this function.
    pub fn src(&self) -> SourceRange {
        self.read().src
    }
}

define_ref! {
    /// A reference-counted pointer to a FunctionTypeName for efficient sharing across multiple contexts.
    ///
    /// This type alias provides thread-safe reference counting for FunctionTypeName instances,
    /// allowing them to be shared between different parts of the analysis system
    /// without copying the entire function data.
    FunctionTypeNameRef(FunctionTypeName) {
        clone_field: {
            visibility: Visibility,
            state_mutability: StateMutability,
        }
    }
}

/// Represents a function or modifier in a smart contract with its metadata and type information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    /// The unique function identifier.
    pub ufid: UFID,
    /// The contract that this function belongs to.
    pub contract: Option<ContractRef>,
    /// The function name.
    pub name: String,
    /// The function visibility.
    pub visibility: Visibility,
    /// The source location of this function.
    pub src: SourceRange,
    /// The state mutability (None for modifiers).
    pub state_mutability: Option<StateMutability>,
    /// Whether this is a modifier (true) or a function (false).
    pub is_modifier: bool,
    /// The scope of this function.
    pub scope: VariableScopeRef,
    /// List of steps in this function.
    pub steps: Vec<StepRef>,
}

impl Function {
    /// Returns whether this function is a modifier.
    pub fn is_modifier(&self) -> bool {
        self.is_modifier
    }
}

impl Function {
    /// Creates a new Function with the given contract and definition.
    pub fn new_function(
        contract: Option<ContractRef>,
        definition: &FunctionDefinition,
        scope: VariableScopeRef,
    ) -> Self {
        Self {
            ufid: UFID::next(),
            contract,
            name: definition.name.clone(),
            visibility: definition.visibility.clone(),
            src: definition.src.into(),
            state_mutability: definition.state_mutability.clone(),
            is_modifier: false,
            scope,
            steps: vec![],
        }
    }

    /// Creates a new Function with the given contract and definition.
    pub fn new_modifier(
        contract: ContractRef,
        definition: &ModifierDefinition,
        scope: VariableScopeRef,
    ) -> Self {
        Self {
            ufid: UFID::next(),
            contract: Some(contract),
            name: definition.name.clone(),
            visibility: definition.visibility.clone(),
            src: definition.src.into(),
            state_mutability: None,
            is_modifier: true,
            scope,
            steps: vec![],
        }
    }
}

/* Function analysis utils */
impl Analyzer {
    pub(super) fn current_function(&self) -> FunctionRef {
        self.current_function.as_ref().expect("current function should be set").clone()
    }

    pub(super) fn enter_new_function(
        &mut self,
        function: &FunctionDefinition,
    ) -> eyre::Result<VisitorAction> {
        assert!(self.current_function.is_none(), "Function cannot be nested");
        let new_func: FunctionRef =
            Function::new_function(self.current_contract.clone(), function, self.current_scope())
                .into();
        self.check_function_visibility_and_mutability(&new_func)?;
        self.current_function = Some(new_func.clone());
        Ok(VisitorAction::Continue)
    }

    pub(super) fn exit_current_function(&mut self) -> eyre::Result<()> {
        assert!(self.current_function.is_some(), "current function should be set");
        let function = self.current_function.take().unwrap();
        self.functions.push(function);
        Ok(())
    }

    pub(super) fn enter_new_modifier(
        &mut self,
        modifier: &ModifierDefinition,
    ) -> eyre::Result<VisitorAction> {
        assert!(self.current_function.is_none(), "Function cannot be nested");
        let current_contract =
            self.current_contract.as_ref().expect("current contract should be set");
        let new_func: FunctionRef =
            Function::new_modifier(current_contract.clone(), modifier, self.current_scope()).into();
        self.current_function = Some(new_func);
        Ok(VisitorAction::Continue)
    }

    pub(super) fn exit_current_modifier(&mut self) -> eyre::Result<()> {
        assert!(self.current_function.is_some(), "current function should be set");
        let function = self.current_function.take().unwrap();
        self.functions.push(function);
        Ok(())
    }

    fn check_function_visibility_and_mutability(&mut self, func: &FunctionRef) -> eyre::Result<()> {
        if func.visibility() != Visibility::Public && func.visibility() != Visibility::External {
            self.private_functions.push(func.clone());
        }

        if func
            .state_mutability()
            .as_ref()
            .is_some_and(|mu| *mu == StateMutability::View || *mu == StateMutability::Pure)
        {
            self.immutable_functions.push(func.clone());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::analyzer::tests::compile_and_analyze;

    #[test]
    fn test_function_and_modifier_collection() {
        let source = r#"
        contract TestContract {
            address public owner;

            modifier onlyOwner() {
                require(msg.sender == owner);
                _;
            }

            function publicFunc() public {
            }

            function internalFunc() internal {
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Should have 2 functions and 1 modifier
        assert_eq!(analysis.functions.len(), 3, "Should have 3 functions (including modifier)");

        // Find each by name
        let modifier = analysis
            .functions
            .iter()
            .find(|f| f.name() == "onlyOwner")
            .expect("Should find onlyOwner modifier");
        let public_func = analysis
            .functions
            .iter()
            .find(|f| f.name() == "publicFunc")
            .expect("Should find publicFunc");
        let internal_func = analysis
            .functions
            .iter()
            .find(|f| f.name() == "internalFunc")
            .expect("Should find internalFunc");

        // Verify the modifier is properly identified
        assert!(modifier.is_modifier(), "onlyOwner should be identified as a modifier");
        assert!(!public_func.is_modifier(), "publicFunc should not be a modifier");
        assert!(!internal_func.is_modifier(), "internalFunc should not be a modifier");
    }

    #[test]
    fn test_free_function_vs_contract_function() {
        let source = r#"
        function freeFunc() pure returns (uint256) {
            return 42;
        }

        contract TestContract {
            function contractFunc() public pure returns (uint256) {
                return 123;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let free_func = analysis
            .functions
            .iter()
            .find(|f| f.name() == "freeFunc")
            .expect("Should find freeFunc");
        let contract_func = analysis
            .functions
            .iter()
            .find(|f| f.name() == "contractFunc")
            .expect("Should find contractFunc");

        // Free function should have no contract
        assert!(free_func.contract().is_none(), "freeFunc should not have a contract");

        // Contract function should have a contract
        assert!(contract_func.contract().is_some(), "contractFunc should have a contract");
        assert_eq!(
            contract_func.contract().unwrap().name(),
            "TestContract",
            "contractFunc should belong to TestContract"
        );
    }

    #[test]
    fn test_function_visibility_categorization() {
        let source = r#"
        contract TestContract {
            function publicFunc() public {
            }

            function externalFunc() external {
            }

            function internalFunc() internal {
            }

            function privateFunc() private {
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Public and external functions should NOT be in private_functions
        let private_func_names: Vec<String> =
            analysis.private_functions.iter().map(|f| f.name()).collect();

        assert!(
            !private_func_names.contains(&"publicFunc".to_string()),
            "publicFunc should not be in private_functions"
        );
        assert!(
            !private_func_names.contains(&"externalFunc".to_string()),
            "externalFunc should not be in private_functions"
        );

        // Internal and private functions should be in private_functions
        assert!(
            private_func_names.contains(&"internalFunc".to_string()),
            "internalFunc should be in private_functions"
        );
        assert!(
            private_func_names.contains(&"privateFunc".to_string()),
            "privateFunc should be in private_functions"
        );
    }

    #[test]
    fn test_function_mutability_categorization() {
        let source = r#"
        contract TestContract {
            function pureFunc() public pure returns (uint256) {
                return 42;
            }

            function viewFunc() public view returns (uint256) {
                return block.timestamp;
            }

            function payableFunc() public payable {
            }

            function nonpayableFunc() public {
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Pure and view functions should be in immutable_functions
        let immutable_func_names: Vec<String> =
            analysis.immutable_functions.iter().map(|f| f.name()).collect();

        assert!(
            immutable_func_names.contains(&"pureFunc".to_string()),
            "pureFunc should be in immutable_functions"
        );
        assert!(
            immutable_func_names.contains(&"viewFunc".to_string()),
            "viewFunc should be in immutable_functions"
        );

        // Payable and nonpayable functions should NOT be in immutable_functions
        assert!(
            !immutable_func_names.contains(&"payableFunc".to_string()),
            "payableFunc should not be in immutable_functions"
        );
        assert!(
            !immutable_func_names.contains(&"nonpayableFunc".to_string()),
            "nonpayableFunc should not be in immutable_functions"
        );
    }

    #[test]
    fn test_function_metadata_methods() {
        let source = r#"
        contract TestContract {
            function testFunc() public pure returns (uint256) {
                return 42;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let func = analysis
            .functions
            .iter()
            .find(|f| f.name() == "testFunc")
            .expect("Should find testFunc");

        // Test name method
        assert_eq!(func.name(), "testFunc", "Function name should be testFunc");

        // Test visibility method
        assert_eq!(func.visibility(), Visibility::Public, "Function should be public");

        // Test state_mutability method
        assert_eq!(func.state_mutability(), Some(StateMutability::Pure), "Function should be pure");

        // Test src method (should return a valid SourceRange)
        let src = func.src();
        assert!(src.start < src.end(), "Source range should be valid");
    }

    #[test]
    fn test_modifier_detection() {
        let source = r#"
        contract TestContract {
            modifier onlyOwner() {
                _;
            }

            function normalFunc() public {
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let modifier = analysis
            .functions
            .iter()
            .find(|f| f.name() == "onlyOwner")
            .expect("Should find onlyOwner");
        let func = analysis
            .functions
            .iter()
            .find(|f| f.name() == "normalFunc")
            .expect("Should find normalFunc");

        // Test is_modifier method
        assert!(modifier.is_modifier(), "onlyOwner should be detected as a modifier");
        assert!(!func.is_modifier(), "normalFunc should not be detected as a modifier");

        // Modifiers should always have a contract
        assert!(modifier.contract().is_some(), "Modifier should have a contract");
    }

    #[test]
    fn test_ufid_uniqueness() {
        let source = r#"
        contract TestContract {
            function func1() public {
            }

            function func2() public {
            }

            function func3() public {
            }

            modifier mod1() {
                _;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Collect all UFIDs
        let ufids: Vec<UFID> = analysis.functions.iter().map(|f| f.ufid()).collect();

        // Check all UFIDs are unique
        let unique_ufids: std::collections::HashSet<_> = ufids.iter().collect();
        assert_eq!(ufids.len(), unique_ufids.len(), "All UFIDs should be unique");

        // Should have 4 unique UFIDs (3 functions + 1 modifier)
        assert_eq!(ufids.len(), 4, "Should have 4 functions/modifiers");
    }

    #[test]
    fn test_function_variant_methods() {
        let source = r#"
        contract TestContract {
            modifier testModifier() {
                _;
            }

            function testFunction() public view {
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let modifier = analysis
            .functions
            .iter()
            .find(|f| f.name() == "testModifier")
            .expect("Should find testModifier");
        let function = analysis
            .functions
            .iter()
            .find(|f| f.name() == "testFunction")
            .expect("Should find testFunction");

        // Test name() method works for both variants
        assert_eq!(modifier.name(), "testModifier", "Modifier name should be testModifier");
        assert_eq!(function.name(), "testFunction", "Function name should be testFunction");

        // Test visibility() method works for both variants
        assert!(
            matches!(modifier.visibility(), Visibility::Internal),
            "Modifier should have visibility"
        );
        assert_eq!(function.visibility(), Visibility::Public, "Function should be public");

        // Test state_mutability() - should return None for modifiers
        assert!(modifier.state_mutability().is_none(), "Modifier should not have state mutability");
        assert_eq!(
            function.state_mutability(),
            Some(StateMutability::View),
            "Function should be view"
        );

        // Test src() method works for both variants
        let modifier_src = modifier.src();
        let function_src = function.src();
        assert!(modifier_src.start < modifier_src.end(), "Modifier should have valid source range");
        assert!(function_src.start < function_src.end(), "Function should have valid source range");
    }

    #[test]
    fn test_multiple_functions_different_properties() {
        let source = r#"
        contract TestContract {
            function publicPure() public pure returns (uint256) {
                return 1;
            }

            function internalView() internal view returns (uint256) {
                return block.timestamp;
            }

            function privateNonpayable() private {
            }

            function externalPayable() external payable {
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        assert_eq!(analysis.functions.len(), 4, "Should have 4 functions");

        // Check private_functions list
        let private_func_names: Vec<String> =
            analysis.private_functions.iter().map(|f| f.name()).collect();
        assert_eq!(private_func_names.len(), 2, "Should have 2 private/internal functions");
        assert!(private_func_names.contains(&"internalView".to_string()));
        assert!(private_func_names.contains(&"privateNonpayable".to_string()));

        // Check immutable_functions list
        let immutable_func_names: Vec<String> =
            analysis.immutable_functions.iter().map(|f| f.name()).collect();
        assert_eq!(immutable_func_names.len(), 2, "Should have 2 pure/view functions");
        assert!(immutable_func_names.contains(&"publicPure".to_string()));
        assert!(immutable_func_names.contains(&"internalView".to_string()));

        // Verify each function has correct properties
        for func in &analysis.functions {
            let name = func.name();
            match name.as_str() {
                "publicPure" => {
                    assert_eq!(func.visibility(), Visibility::Public);
                    assert_eq!(func.state_mutability(), Some(StateMutability::Pure));
                }
                "internalView" => {
                    assert_eq!(func.visibility(), Visibility::Internal);
                    assert_eq!(func.state_mutability(), Some(StateMutability::View));
                }
                "privateNonpayable" => {
                    assert_eq!(func.visibility(), Visibility::Private);
                    assert_eq!(func.state_mutability(), Some(StateMutability::Nonpayable));
                }
                "externalPayable" => {
                    assert_eq!(func.visibility(), Visibility::External);
                    assert_eq!(func.state_mutability(), Some(StateMutability::Payable));
                }
                _ => panic!("Unexpected function: {name}"),
            }
        }
    }
}
