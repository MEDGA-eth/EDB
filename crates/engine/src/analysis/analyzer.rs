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

use std::path::PathBuf;

use foundry_compilers::artifacts::{
    Assignment, Block, ContractDefinition, EnumDefinition, ErrorDefinition, EventDefinition,
    ForStatement, FunctionCall, FunctionDefinition, IfStatement, ModifierDefinition,
    PragmaDirective, SourceUnit, Statement, StructDefinition, UncheckedBlock,
    UserDefinedValueTypeDefinition, VariableDeclaration, WhileStatement,
};
use std::collections::HashMap;

use thiserror::Error;
use tracing::debug;

use crate::{
    analysis::{
        ContractRef, FunctionRef, FunctionTypeNameRef, SourceAnalysis, StatementBody, StepRef,
        UserDefinedTypeRef, VariableRef, VariableScopeRef,
    },
    utils::{Visitor, VisitorAction, Walk},
};

/// Main analyzer for processing Solidity source code and extracting execution steps.
///
/// The Analyzer walks through the Abstract Syntax Tree (AST) of Solidity source code
/// and identifies executable steps, manages variable scopes, and tracks function
/// visibility and mutability requirements.
///
/// # Fields
///
/// - `scope_stack`: Stack of variable scopes for managing variable visibility
/// - `finished_steps`: Completed execution steps that have been fully analyzed
/// - `current_step`: Currently being analyzed step (if any)
/// - `private_state_variables`: State variables that should be made public
/// - `private_functions`: Functions that should be made public
/// - `immutable_functions`: Functions that should be made mutable
#[derive(Debug, Clone)]
pub struct Analyzer {
    pub(in crate::analysis) source_id: u32,
    source_path: PathBuf,
    pub(super) source: String,
    version_requirements: Vec<String>,

    pub(super) scope_stack: Vec<VariableScopeRef>,

    pub(super) finished_steps: Vec<StepRef>,
    pub(super) current_step: Option<StepRef>,
    pub(super) current_function: Option<FunctionRef>,
    pub(super) current_contract: Option<ContractRef>,
    /// All statement bodies in this file.
    pub(super) statement_bodies: Vec<StatementBody>,
    /// List of all contracts in this file.
    pub(super) contracts: Vec<ContractRef>,
    /// List of all functions in this file.
    pub(super) functions: Vec<FunctionRef>,
    /// A mapping from the `VariableDeclaration` AST node ID to the variable reference.
    pub(super) variables: HashMap<usize, VariableRef>,
    /// List of all state variables in this file.
    pub(super) state_variables: Vec<VariableRef>,
    /// State variables that should be made public
    pub(super) private_state_variables: Vec<VariableRef>,
    /// Functions that should be made public
    pub(super) private_functions: Vec<FunctionRef>,
    /// Functions that should be made mutable (i.e., neither pure nor view)
    pub(super) immutable_functions: Vec<FunctionRef>,
    /// Function types defined in this file.
    pub(super) function_types: Vec<FunctionTypeNameRef>,
    /// User defined types defined in this file.
    pub(in crate::analysis) user_defined_types: Vec<UserDefinedTypeRef>,
    /// Context flag: true when declaring function parameters
    pub(super) is_declaring_param: bool,
    /// Context flag: true when declaring return variables
    pub(super) is_declaring_return: bool,
    /// Context flag: true when we are inside a `VariableDeclarationStatement` with an initial
    /// value
    pub(super) is_declaring_with_initial_expr: bool,
}

impl Analyzer {
    /// Creates a new instance of the Analyzer.
    ///
    /// This method initializes a fresh analyzer with default state, ready to analyze
    /// Solidity source code.
    ///
    /// # Returns
    ///
    /// A new `Analyzer` instance with empty scope stack and step collections.
    pub fn new(source_id: u32, source_path: PathBuf, source: String) -> Self {
        Self {
            source_id,
            source,
            source_path,
            version_requirements: Vec::new(),
            scope_stack: Vec::new(),
            finished_steps: Vec::new(),
            current_step: None,
            current_function: None,
            current_contract: None,
            statement_bodies: Vec::new(),
            contracts: Vec::new(),
            functions: Vec::new(),
            variables: HashMap::default(),
            state_variables: Vec::new(),
            private_state_variables: Vec::new(),
            private_functions: Vec::new(),
            immutable_functions: Vec::new(),
            function_types: Vec::new(),
            user_defined_types: Vec::new(),
            is_declaring_param: false,
            is_declaring_return: false,
            is_declaring_with_initial_expr: false,
        }
    }

    /// Analyzes a source unit and returns the analysis results.
    ///
    /// This method walks through the AST of the source unit, identifies execution steps,
    /// manages variable scopes, and collects recommendations for code improvements.
    ///
    /// # Arguments
    ///
    /// * `source_id` - Unique identifier for the source file
    /// * `source_path` - File system path to the source file
    /// * `source_unit` - The source unit to analyze
    ///
    /// # Returns
    ///
    /// A `Result` containing the `SourceAnalysis` on success, or an `AnalysisError` on failure.
    ///
    /// # Errors
    ///
    /// Returns an error if the AST walk fails or if there are issues with step partitioning.
    pub fn analyze(mut self, source_unit: &SourceUnit) -> Result<SourceAnalysis, AnalysisError> {
        debug!(path=?self.source_path, "start walking the AST");
        source_unit.walk(&mut self).map_err(AnalysisError::Other)?;
        debug!(path=?self.source_path, "finished walking the AST");

        assert!(self.scope_stack.len() == 1, "scope stack should have exactly one scope");
        assert!(self.current_step.is_none(), "current step should be none");
        let global_scope = self.scope_stack.pop().expect("global scope should not be empty");
        let steps = self.finished_steps;
        let functions = self.functions;
        Ok(SourceAnalysis {
            id: self.source_id,
            path: self.source_path,
            source: self.source,
            unit: source_unit.clone(),
            global_scope,
            steps,
            statement_bodies: self.statement_bodies,
            contracts: self.contracts,
            private_state_variables: self.private_state_variables,
            state_variables: self.state_variables,
            functions,
            private_functions: self.private_functions,
            immutable_functions: self.immutable_functions,
            function_types: self.function_types,
            user_defined_types: self.user_defined_types,
        })
    }
}

impl Visitor for Analyzer {
    fn visit_source_unit(&mut self, source_unit: &SourceUnit) -> eyre::Result<VisitorAction> {
        // enter a global scope
        self.enter_new_scope(source_unit.src)?;
        Ok(VisitorAction::Continue)
    }

    fn post_visit_source_unit(&mut self, _source_unit: &SourceUnit) -> eyre::Result<()> {
        assert_eq!(
            self.scope_stack.len(),
            1,
            "Scope stack should only have one scope (the global scope)"
        );
        assert!(self.current_step.is_none(), "Step should be finished");
        Ok(())
    }

    fn visit_pragma_directive(
        &mut self,
        directive: &PragmaDirective,
    ) -> eyre::Result<VisitorAction> {
        let literals = &directive.literals;
        if literals.len() > 1 && literals[0].trim() == "solidity" {
            let mut version_str = vec![];
            let mut current_req = String::new();
            let mut i = 1;
            while i < literals.len() {
                let literal = &literals[i];
                if literal.starts_with('.') {
                    current_req.push_str(literal);
                } else if ["=", "<", ">", "~", "^"].iter().any(|p| literal.starts_with(p)) {
                    version_str.push(current_req);
                    current_req = literal.clone();
                    i += 1;
                    current_req.push_str(&literals[i]);
                } else {
                    version_str.push(current_req);
                    current_req = literal.clone();
                }
                i += 1;
            }
            version_str.push(current_req);

            let version_str =
                version_str.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(",");
            // one source file may have multiple `pragma solidity` directives, we collect all of
            // them
            self.version_requirements.push(version_str);
        }
        Ok(VisitorAction::Continue)
    }

    fn visit_contract_definition(
        &mut self,
        _definition: &ContractDefinition,
    ) -> eyre::Result<VisitorAction> {
        // record the contract type
        self.record_contract_type(_definition)?;

        // enter a contract scope
        self.enter_new_scope(_definition.src)?;

        // enter a new contract
        self.enter_new_contract(_definition)?;

        Ok(VisitorAction::Continue)
    }

    fn post_visit_contract_definition(
        &mut self,
        _definition: &ContractDefinition,
    ) -> eyre::Result<()> {
        // exit the contract scope
        self.exit_current_scope(_definition.src)?;

        // exit the contract
        self.exit_current_contract()?;
        Ok(())
    }

    fn visit_user_defined_value_type(
        &mut self,
        _value_type: &UserDefinedValueTypeDefinition,
    ) -> eyre::Result<VisitorAction> {
        self.record_user_defined_value_type(_value_type)?;
        Ok(VisitorAction::Continue)
    }

    fn visit_struct_definition(
        &mut self,
        _definition: &StructDefinition,
    ) -> eyre::Result<VisitorAction> {
        self.record_struct_type(_definition)?;
        Ok(VisitorAction::SkipSubtree)
    }

    fn visit_enum_definition(
        &mut self,
        _definition: &EnumDefinition,
    ) -> eyre::Result<VisitorAction> {
        self.record_enum_type(_definition)?;
        Ok(VisitorAction::SkipSubtree)
    }

    fn visit_event_definition(
        &mut self,
        _definition: &EventDefinition,
    ) -> eyre::Result<VisitorAction> {
        Ok(VisitorAction::SkipSubtree)
    }

    fn visit_error_definition(
        &mut self,
        _definition: &ErrorDefinition,
    ) -> eyre::Result<VisitorAction> {
        Ok(VisitorAction::SkipSubtree)
    }

    fn visit_function_definition(
        &mut self,
        definition: &FunctionDefinition,
    ) -> eyre::Result<VisitorAction> {
        // enter a variable scope for the function
        self.enter_new_scope(definition.src)?;

        // enter a new function
        self.enter_new_function(definition)?;

        // enter a function step
        self.enter_new_function_step(definition)
    }

    fn post_visit_function_definition(
        &mut self,
        definition: &FunctionDefinition,
    ) -> eyre::Result<()> {
        // exit the function scope
        self.exit_current_scope(definition.src)?;

        // exit the function
        self.exit_current_function()?;
        Ok(())
    }

    fn visit_modifier_definition(
        &mut self,
        definition: &ModifierDefinition,
    ) -> eyre::Result<VisitorAction> {
        // enter a variable scope for the modifier
        self.enter_new_scope(definition.src)?;

        // enter a new modifier
        self.enter_new_modifier(definition)?;

        // enter a modifier step
        self.enter_new_modifier_step(definition)
    }

    fn post_visit_modifier_definition(
        &mut self,
        definition: &ModifierDefinition,
    ) -> eyre::Result<()> {
        // exit the modifier scope
        self.exit_current_scope(definition.src)?;

        // exit the modifier
        self.exit_current_modifier()?;
        Ok(())
    }

    fn visit_block(&mut self, block: &Block) -> eyre::Result<VisitorAction> {
        // enter a block scope
        self.enter_new_scope(block.src)?;
        Ok(VisitorAction::Continue)
    }

    fn post_visit_block(&mut self, block: &Block) -> eyre::Result<()> {
        // exit the block scope
        self.exit_current_scope(block.src)?;
        Ok(())
    }

    fn visit_unchecked_block(
        &mut self,
        unchecked_block: &UncheckedBlock,
    ) -> eyre::Result<VisitorAction> {
        // enter an unchecked block scope
        self.enter_new_scope(unchecked_block.src)?;
        Ok(VisitorAction::Continue)
    }

    fn post_visit_unchecked_block(&mut self, unchecked_block: &UncheckedBlock) -> eyre::Result<()> {
        // exit the unchecked block scope
        self.exit_current_scope(unchecked_block.src)?;
        Ok(())
    }

    fn visit_statement(&mut self, _statement: &Statement) -> eyre::Result<VisitorAction> {
        // try to enter a new step
        self.enter_new_statement_step(_statement)
    }

    fn post_visit_statement(&mut self, _statement: &Statement) -> eyre::Result<()> {
        // exit the current step
        self.exit_current_statement_step(_statement)?;

        Ok(())
    }

    fn visit_function_call(&mut self, function_call: &FunctionCall) -> eyre::Result<VisitorAction> {
        self.add_function_call(function_call)?;
        Ok(VisitorAction::Continue)
    }

    fn visit_variable_declaration(
        &mut self,
        declaration: &VariableDeclaration,
    ) -> eyre::Result<VisitorAction> {
        // declare a variable
        self.declare_variable(declaration)?;
        // record the declared variable
        self.record_declared_varaible(declaration)?;
        Ok(VisitorAction::Continue)
    }

    fn visit_assignment(&mut self, assignment: &Assignment) -> eyre::Result<VisitorAction> {
        // record updated variables
        self.record_assignment(assignment)
    }

    fn visit_if_statement(&mut self, if_statement: &IfStatement) -> eyre::Result<VisitorAction> {
        self.collect_statement_bodies(&if_statement.true_body);
        if let Some(false_body) = &if_statement.false_body {
            self.collect_statement_bodies(false_body);
        }
        Ok(VisitorAction::Continue)
    }

    fn visit_while_statement(
        &mut self,
        while_statement: &WhileStatement,
    ) -> eyre::Result<VisitorAction> {
        self.collect_statement_bodies(&while_statement.body);
        Ok(VisitorAction::Continue)
    }

    fn visit_variable_declaration_statement(
        &mut self,
        declaration_stmt: &foundry_compilers::artifacts::VariableDeclarationStatement,
    ) -> eyre::Result<VisitorAction> {
        self.is_declaring_with_initial_expr = declaration_stmt.initial_value.is_some();
        Ok(VisitorAction::Continue)
    }

    fn post_visit_variable_declaration_statement(
        &mut self,
        _declaration_stmt: &foundry_compilers::artifacts::VariableDeclarationStatement,
    ) -> eyre::Result<()> {
        self.is_declaring_with_initial_expr = false;
        Ok(())
    }
}

/// A walker wrapping [`Analyzer`] that only walks a single step.
#[derive(derive_more::Deref, derive_more::DerefMut)]
pub(super) struct AnalyzerSingleStepWalker<'a> {
    #[deref]
    #[deref_mut]
    pub(super) analyzer: &'a mut Analyzer,
}

impl<'a> Visitor for AnalyzerSingleStepWalker<'a> {
    fn visit_function_call(&mut self, function_call: &FunctionCall) -> eyre::Result<VisitorAction> {
        self.analyzer.add_function_call(function_call)?;
        Ok(VisitorAction::Continue)
    }

    fn visit_variable_declaration(
        &mut self,
        declaration: &VariableDeclaration,
    ) -> eyre::Result<VisitorAction> {
        self.analyzer.declare_variable(declaration)?;
        self.analyzer.record_declared_varaible(declaration)?;
        Ok(VisitorAction::Continue)
    }

    fn visit_for_statement(&mut self, for_statement: &ForStatement) -> eyre::Result<VisitorAction> {
        self.analyzer.enter_new_scope(for_statement.src)?;
        Ok(VisitorAction::Continue)
    }

    fn post_visit_for_statement(&mut self, for_statement: &ForStatement) -> eyre::Result<()> {
        self.analyzer.exit_current_scope(for_statement.src)?;
        Ok(())
    }

    fn visit_if_statement(
        &mut self,
        _if_statement: &foundry_compilers::artifacts::IfStatement,
    ) -> eyre::Result<VisitorAction> {
        self.analyzer.collect_statement_bodies(&_if_statement.true_body);
        if let Some(false_body) = &_if_statement.false_body {
            self.analyzer.collect_statement_bodies(false_body);
        }
        Ok(VisitorAction::Continue)
    }

    fn visit_while_statement(
        &mut self,
        while_statement: &WhileStatement,
    ) -> eyre::Result<VisitorAction> {
        self.analyzer.collect_statement_bodies(&while_statement.body);
        Ok(VisitorAction::Continue)
    }
}

/// Errors that can occur during source code analysis.
///
/// This enum represents all possible error conditions that can arise during
/// the analysis process, from compilation failures to step partitioning errors.
#[derive(Debug, Error)]
pub enum AnalysisError {
    /// AST data is not available in the compiled artifact
    #[error("AST is not selected as compiler output")]
    MissingAst,

    /// Source is not available in the compiled artifact
    #[error("Source is not available in the compiled artifact")]
    MissingSource,

    /// Error during AST conversion
    #[error("failed to convert AST: {0}")]
    ASTConversionError(eyre::Report),

    /// Error during step partitioning of source code
    #[error("failed to partition source steps: {0}")]
    StepPartitionError(eyre::Report),

    /// Other analysis-related errors
    #[error("other error: {0}")]
    Other(eyre::Report),
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use foundry_compilers::artifacts::Source;
    use semver::Version;

    use crate::test_utils::compile_contract_source_to_source_unit;

    use super::*;

    pub(crate) const TEST_CONTRACT_SOURCE_PATH: &str = "test.sol";
    pub(crate) const TEST_CONTRACT_SOURCE_ID: u32 = 0;

    /// Utility function to compile Solidity source code and analyze it
    ///
    /// This function encapsulates the common pattern used across all tests:
    /// 1. Compile the source code to get the AST
    /// 2. Create an analyzer and analyze the contract
    /// 3. Return the analysis result
    ///
    /// # Arguments
    /// * `source` - The Solidity source code as a string
    ///
    /// # Returns
    /// * `SourceAnalysis` - The analysis result containing steps, scopes, and recommendations
    pub(crate) fn compile_and_analyze(source: &str) -> (BTreeMap<u32, Source>, SourceAnalysis) {
        // Compile the source code to get the AST
        let version = Version::parse("0.8.30").unwrap();
        let result = compile_contract_source_to_source_unit(version, source, false);
        assert!(result.is_ok(), "Source compilation should succeed: {}", result.unwrap_err());

        let source_unit = result.unwrap();
        let sources = BTreeMap::from([(TEST_CONTRACT_SOURCE_ID, Source::new(source))]);

        // Create an analyzer and analyze the contract
        let analyzer = Analyzer::new(
            TEST_CONTRACT_SOURCE_ID,
            PathBuf::from(TEST_CONTRACT_SOURCE_PATH),
            source.to_string(),
        );
        let analysis = analyzer.analyze(&source_unit).unwrap();

        (sources, analysis)
    }
}
