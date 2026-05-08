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

use foundry_compilers::artifacts::{ContractDefinition, ContractKind};
use serde::{Deserialize, Serialize};

use crate::{
    analysis::{
        Analyzer,
        macros::{define_ref, universal_id},
    },
    utils::VisitorAction,
};

universal_id! {
    /// A Universal Contract Identifier (UCID) is a unique identifier for a contract.
    UCID => 0
}

define_ref! {
    /// A reference-counted pointer to a Contract for efficient sharing across multiple contexts.
    ContractRef(Contract) {
        clone_field: {
            ucid: UCID,
            name: String,
            kind: ContractKind,
        }
    }
}
/// Represents a contract in a smart contract with its metadata and type information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    /// The unique contract identifier.
    pub ucid: UCID,
    /// The contract name.
    pub name: String,
    /// The contract kind (Contract, Interface, Library).
    pub kind: ContractKind,
}

impl Contract {
    /// Creates a new Contract with the given definition.
    pub fn new(definition: &ContractDefinition) -> Self {
        Self { ucid: UCID::next(), name: definition.name.clone(), kind: definition.kind.clone() }
    }

    /// Returns the name of this contract.
    pub fn name(&self) -> String {
        self.name.clone()
    }
}

/* Contract analysis utils */
impl Analyzer {
    pub(super) fn enter_new_contract(
        &mut self,
        contract: &ContractDefinition,
    ) -> eyre::Result<VisitorAction> {
        assert!(self.current_contract.is_none(), "Contract cannot be nested");
        let new_contract: ContractRef = Contract::new(contract).into();
        self.current_contract = Some(new_contract);
        Ok(VisitorAction::Continue)
    }

    pub(super) fn exit_current_contract(&mut self) -> eyre::Result<()> {
        assert!(self.current_contract.is_some(), "current contract should be set");
        let contract = self.current_contract.take().unwrap();
        self.contracts.push(contract);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::analyzer::tests::compile_and_analyze;

    #[test]
    fn test_contract_collection() {
        let source = r#"
        function foo() {
        }
        contract TestContract {
            function bar() public {
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let foo_func = analysis
            .functions
            .iter()
            .find(|c| c.read().name == "foo")
            .expect("foo function should be found");
        let bar_func = analysis
            .functions
            .iter()
            .find(|c| c.read().name == "bar")
            .expect("bar function should be found");
        assert!(foo_func.read().contract.is_none());
        assert!(bar_func.read().contract.as_ref().is_some_and(|c| c.name() == "TestContract"));
    }

    #[test]
    fn test_multiple_contracts() {
        let source = r#"
        contract Contract1 {
            function func1() public {
            }
        }

        contract Contract2 {
            function func2() public {
            }
        }

        contract Contract3 {
            function func3() public {
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Should have exactly 3 contracts
        assert_eq!(analysis.contracts.len(), 3, "Should have 3 contracts");

        // Verify all contracts are collected
        let contract_names: Vec<String> = analysis.contracts.iter().map(|c| c.name()).collect();

        assert!(contract_names.contains(&"Contract1".to_string()), "Contract1 should be collected");
        assert!(contract_names.contains(&"Contract2".to_string()), "Contract2 should be collected");
        assert!(contract_names.contains(&"Contract3".to_string()), "Contract3 should be collected");

        // Verify each function is associated with the correct contract
        let func1 =
            analysis.functions.iter().find(|f| f.name() == "func1").expect("Should find func1");
        let func2 =
            analysis.functions.iter().find(|f| f.name() == "func2").expect("Should find func2");
        let func3 =
            analysis.functions.iter().find(|f| f.name() == "func3").expect("Should find func3");

        assert_eq!(func1.contract().unwrap().name(), "Contract1");
        assert_eq!(func2.contract().unwrap().name(), "Contract2");
        assert_eq!(func3.contract().unwrap().name(), "Contract3");
    }

    #[test]
    fn test_ucid_uniqueness() {
        let source = r#"
        contract Contract1 {
        }

        contract Contract2 {
        }

        contract Contract3 {
        }

        contract Contract4 {
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Collect all UCIDs
        let ucids: Vec<UCID> = analysis.contracts.iter().map(|c| c.read().ucid).collect();

        // Check all UCIDs are unique
        let unique_ucids: std::collections::HashSet<_> = ucids.iter().collect();
        assert_eq!(ucids.len(), unique_ucids.len(), "All UCIDs should be unique");

        // Should have 4 unique UCIDs
        assert_eq!(ucids.len(), 4, "Should have 4 contracts");
    }

    #[test]
    fn test_contract_name_method() {
        let source = r#"
        contract MyTestContract {
            function foo() public {
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Find the contract
        let contract = analysis
            .contracts
            .iter()
            .find(|c| c.name() == "MyTestContract")
            .expect("Should find MyTestContract");

        // Test name() method
        assert_eq!(contract.name(), "MyTestContract", "Contract name should be MyTestContract");
    }

    #[test]
    fn test_different_contract_kinds() {
        let source = r#"
        contract RegularContract {
            function foo() public {
            }
        }

        interface IMyInterface {
            function bar() external;
        }

        library MyLibrary {
            function baz() internal pure returns (uint256) {
                return 42;
            }
        }

        abstract contract AbstractContract {
            function abstractFunc() public virtual;
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Should have 4 contracts (interface, library, and abstract are all ContractDefinitions)
        assert_eq!(analysis.contracts.len(), 4, "Should have 4 contracts");

        // Verify all contract kinds are collected
        let contract_names: Vec<String> = analysis.contracts.iter().map(|c| c.name()).collect();

        assert!(
            contract_names.contains(&"RegularContract".to_string()),
            "RegularContract should be collected"
        );
        assert!(
            contract_names.contains(&"IMyInterface".to_string()),
            "IMyInterface should be collected"
        );
        assert!(contract_names.contains(&"MyLibrary".to_string()), "MyLibrary should be collected");
        assert!(
            contract_names.contains(&"AbstractContract".to_string()),
            "AbstractContract should be collected"
        );
    }

    #[test]
    fn test_contract_inheritance() {
        let source = r#"
        contract BaseContract {
            function baseFunc() public {
            }
        }

        contract DerivedContract is BaseContract {
            function derivedFunc() public {
            }
        }

        interface IBase {
            function interfaceFunc() external;
        }

        contract ImplementingContract is IBase {
            function interfaceFunc() external {
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Should have 4 contracts
        assert_eq!(analysis.contracts.len(), 4, "Should have 4 contracts");

        // Find the derived contract
        let _derived = analysis
            .contracts
            .iter()
            .find(|c| c.name() == "DerivedContract")
            .expect("Should find DerivedContract");

        // Find the implementing contract
        let _implementing = analysis
            .contracts
            .iter()
            .find(|c| c.name() == "ImplementingContract")
            .expect("Should find ImplementingContract");
    }

    #[test]
    fn test_empty_contract() {
        let source = r#"
        contract EmptyContract {
        }

        contract NonEmptyContract {
            function foo() public {
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Should have 2 contracts
        assert_eq!(analysis.contracts.len(), 2, "Should have 2 contracts");

        // Find the empty contract
        let empty = analysis
            .contracts
            .iter()
            .find(|c| c.name() == "EmptyContract")
            .expect("Should find EmptyContract");

        // Verify it's properly collected
        assert_eq!(empty.name(), "EmptyContract");

        // Find the non-empty contract
        let non_empty = analysis
            .contracts
            .iter()
            .find(|c| c.name() == "NonEmptyContract")
            .expect("Should find NonEmptyContract");

        assert_eq!(non_empty.name(), "NonEmptyContract");
    }

    #[test]
    fn test_contract_with_various_members() {
        let source = r#"
        contract ComplexContract {
            uint256 public stateVar;
            address private owner;

            event Transfer(address indexed from, address indexed to, uint256 value);
            error InvalidAmount(uint256 amount);

            struct Data {
                uint256 id;
                string name;
            }

            enum Status {
                Pending,
                Active,
                Completed
            }

            modifier onlyOwner() {
                require(msg.sender == owner);
                _;
            }

            function publicFunc() public {
            }

            function privateFunc() private {
            }

            constructor() {
                owner = msg.sender;
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Should have exactly 1 contract
        assert_eq!(analysis.contracts.len(), 1, "Should have 1 contract");

        // Find the contract
        let contract = analysis
            .contracts
            .iter()
            .find(|c| c.name() == "ComplexContract")
            .expect("Should find ComplexContract");

        // Verify the contract is properly collected
        assert_eq!(contract.name(), "ComplexContract");

        // Verify functions are associated with this contract
        let public_func = analysis
            .functions
            .iter()
            .find(|f| f.name() == "publicFunc")
            .expect("Should find publicFunc");
        let private_func = analysis
            .functions
            .iter()
            .find(|f| f.name() == "privateFunc")
            .expect("Should find privateFunc");
        let modifier = analysis
            .functions
            .iter()
            .find(|f| f.name() == "onlyOwner")
            .expect("Should find onlyOwner modifier");

        // All functions/modifiers should be associated with the contract
        assert_eq!(public_func.contract().unwrap().name(), "ComplexContract");
        assert_eq!(private_func.contract().unwrap().name(), "ComplexContract");
        assert_eq!(modifier.contract().unwrap().name(), "ComplexContract");
    }
}
