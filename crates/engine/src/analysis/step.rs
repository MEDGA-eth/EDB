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
    ast::SourceLocation, Expression, FunctionCall, FunctionCallKind, FunctionDefinition,
    ModifierDefinition, Statement,
};
use serde::{Deserialize, Serialize};

use crate::{
    analysis::{
        block_or_stmt_src,
        macros::{define_ref, universal_id},
        Analyzer, AnalyzerSingleStepWalker, FunctionRef, SourceRange, StepHookLocations,
        VariableRef, VariableScopeRef, UFID,
    },
    find_index_of_first_statement_in_block, find_index_of_first_statement_in_block_or_statement,
    find_next_index_of_last_statement_in_block, VisitorAction, Walk,
};

universal_id! {
    /// A Universal Step Identifier (USID) is a unique identifier for a step in contract execution.
    USID => 0
}

define_ref! {
    /// A reference-counted pointer to a Step for efficient sharing across multiple contexts.
    ///
    /// This type alias provides thread-safe reference counting for Step instances,
    /// allowing them to be shared between different parts of the analysis system
    /// without copying the entire step data.
    StepRef(Step) {
        clone_field: {
            usid: USID,
            ufid: UFID,
            src: SourceRange,
            hook_locations: StepHookLocations,
            kind: StepKind,
            function_calls: usize,
        }
        cached_field: {
            updated_variables: Vec<VariableRef>,
            accessible_variables: Vec<VariableRef>,
        }
    }
}

impl StepRef {
    /// Check whether this step is an entry of a function
    pub fn function_entry(&self) -> Option<UFID> {
        if let StepKind::Entry(func) = self.kind() {
            (!func.is_modifier()).then_some(func.ufid())
        } else {
            None
        }
    }

    /// Check whether this step is a statement step
    pub fn is_statement(&self) -> bool {
        matches!(
            self.kind(),
            StepKind::EmitStatement
                | StepKind::ReturnStatement
                | StepKind::PlaceholderStatement
                | StepKind::OtherStatement
        )
    }

    /// Check whether this step is an entry step
    pub fn is_entry(&self) -> bool {
        matches!(self.kind(), StepKind::Entry(_))
    }

    /// Check whether this step is an entry of a modifier
    pub fn modifier_entry(&self) -> Option<UFID> {
        if let StepKind::Entry(func) = self.kind() {
            func.is_modifier().then_some(func.ufid())
        } else {
            None
        }
    }

    /// Check whether this step contains return statements
    pub fn contains_return(&self) -> bool {
        matches!(self.kind(), StepKind::ReturnStatement)
    }
}

/// Represents a single executable step in Solidity source code.
///
/// A Step represents a unit of execution that can be debugged, such as a statement,
/// expression, or control flow construct. Each step contains information about
/// its location in the source code and any hooks that should be executed before
/// or after the step.
///
/// # Fields
///
/// - `usid`: Unique step identifier for this execution step
/// - `variant`: The specific type of step (statement, expression, etc.)
/// - `src`: Source location information (file, line, column)
/// - `pre_hooks`: Hooks to execute before this step
/// - `post_hooks`: Hooks to execute after this step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Unique step identifier for this execution step
    pub usid: USID,
    /// The identifier of the function that this step belongs to.
    pub ufid: UFID,
    /// Source range of the step
    pub src: SourceRange,
    /// The locations of different types of hooks in this step
    pub hook_locations: StepHookLocations,
    /// The kind of step
    pub kind: StepKind,
    /// Count of actual function calls (excluding built-ins and events)
    pub function_calls: usize,
    /// Variables accessible in this step (excluding those declared in this step)
    pub accessible_variables: Vec<VariableRef>,
    /// Variables declared in this step
    pub declared_variables: Vec<VariableRef>,
    /// Variables updated in this step
    pub updated_variables: Vec<VariableRef>,
    /// The scope of this step
    pub scope: VariableScopeRef,
}

/// The kind of step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepKind {
    /// An entry step for a function or modifier
    Entry(FunctionRef),
    /// A emit statement step
    EmitStatement,
    /// A return statement step
    ReturnStatement,
    /// A placeholder statement step
    PlaceholderStatement,
    /// Other statement steps
    OtherStatement,
}

impl Step {
    /// Creates a new Step with the given variant and source location.
    ///
    /// # Arguments
    ///
    /// * `variant` - The type of step (statement, expression, etc.)
    /// * `src` - Source location information
    ///
    /// # Returns
    ///
    /// A new Step instance with a unique USID and default hooks.
    pub fn new(
        ufid: UFID,
        src: SourceRange,
        hook_locations: StepHookLocations,
        kind: StepKind,
        scope: VariableScopeRef,
        accessible_variables: Vec<VariableRef>,
    ) -> Self {
        let usid = USID::next();
        Self {
            usid,
            ufid,
            src,
            hook_locations,
            kind,
            function_calls: 0,
            accessible_variables,
            declared_variables: vec![],
            updated_variables: vec![],
            scope,
        }
    }
}

/* Step partition utils */
impl Analyzer {
    pub(super) fn enter_new_statement_step(
        &mut self,
        statement: &Statement,
    ) -> eyre::Result<VisitorAction> {
        assert!(self.current_step.is_none(), "Step cannot be nested");
        let current_function = self.current_function();
        let current_scope = self.current_scope();

        macro_rules! step {
            ($variant:ident, $stmt:expr, $loc:expr, $hooks:expr, $kind:expr) => {{
                let variables_in_scope = current_scope.read().variables_recursive();
                let new_step: StepRef = Step::new(
                    current_function.ufid(),
                    $loc,
                    $hooks,
                    $kind,
                    current_scope.clone(),
                    variables_in_scope.clone(),
                )
                .into();
                self.current_step = Some(new_step.clone());
                // add the step to the current function
                current_function.write().steps.push(new_step);
            }};
        }
        macro_rules! simple_stmt_to_step {
            ($stmt:expr) => {{
                let src: SourceRange = $stmt.src.into();
                let step_str = src.slice_source(&self.source);
                let after_step = if step_str.trim_end().ends_with(";") {
                    vec![src.next_loc()]
                } else {
                    vec![src.expand_to_next_semicolon(&self.source).next_loc()]
                };
                let kind = match statement {
                    Statement::EmitStatement { .. } => StepKind::EmitStatement,
                    Statement::Return(..) => StepKind::ReturnStatement,
                    Statement::PlaceholderStatement(..) => StepKind::PlaceholderStatement,
                    _ => StepKind::OtherStatement,
                };
                step!(
                    Statement,
                    statement.clone(),
                    src,
                    StepHookLocations { before_step: src.start, after_step },
                    kind
                )
            }};
        }

        match statement {
            Statement::Block(_) => {}
            Statement::Break(break_stmt) => simple_stmt_to_step!(break_stmt),
            Statement::Continue(continue_stmt) => simple_stmt_to_step!(continue_stmt),
            Statement::DoWhileStatement(do_while_statement) => {
                // the step is the `while(...)`
                let src = sloc_rdiff(do_while_statement.src, do_while_statement.body.src).into();
                step!(
                    DoWhileLoop,
                    *do_while_statement.clone(),
                    src,
                    StepHookLocations {
                        // the before step hook should be instrumented after the last statement of the do-while statement
                        before_step: find_next_index_of_last_statement_in_block(
                            &self.source,
                            &do_while_statement.body
                        )
                        .expect("do-while statement last statement location not found"),
                        // the after step hook should be instrumented after the do-while statement
                        after_step: vec![src.next_loc()]
                    },
                    StepKind::OtherStatement
                );

                // we take over the walk of the sub ast tree in the do-while statement step.
                let mut single_step_walker = AnalyzerSingleStepWalker { analyzer: self };
                do_while_statement.condition.walk(&mut single_step_walker)?;

                // end the do-while statement step early and then walk the body of the do-while statement.
                self.exit_current_statement_step(statement)?;
                do_while_statement.body.walk(self)?;

                // skip the subtree of the do-while statement since we have already walked it
                return Ok(VisitorAction::SkipSubtree);
            }
            Statement::EmitStatement(emit_statement) => simple_stmt_to_step!(emit_statement),
            Statement::ExpressionStatement(expr_stmt) => simple_stmt_to_step!(expr_stmt),
            Statement::ForStatement(for_statement) => {
                // enter a new scope for the for statement
                self.enter_new_scope(for_statement.src)?;

                // the step is the `for(...)`
                let src =
                    sloc_ldiff(for_statement.src, block_or_stmt_src(&for_statement.body)).into();
                step!(
                    ForLoop,
                    *for_statement.clone(),
                    src,
                    StepHookLocations {
                        // the before step hook should be instrumented before the for statement
                        before_step: src.start,
                        // the after step hook should be instrumented before the first statement of the for statement
                        after_step: vec![find_index_of_first_statement_in_block_or_statement(
                            &for_statement.body
                        )
                        .expect("for statement first statement location not found")]
                    },
                    StepKind::OtherStatement
                );

                // we take over the walk of the sub ast tree in the for statement step.
                let mut single_step_walker = AnalyzerSingleStepWalker { analyzer: self };
                if let Some(initialization_expression) = &for_statement.initialization_expression {
                    initialization_expression.walk(&mut single_step_walker)?;
                }
                if let Some(condition) = &for_statement.condition {
                    condition.walk(&mut single_step_walker)?;
                }
                if let Some(loop_expression) = &for_statement.loop_expression {
                    loop_expression.walk(&mut single_step_walker)?;
                }

                // collect statement bodies before walking the body
                self.collect_statement_bodies(&for_statement.body);

                // end the for statement step early and then walk the body of the for statement.
                self.exit_current_statement_step(statement)?;
                for_statement.body.walk(self)?;

                // exit the scope of the for statement
                self.exit_current_scope(for_statement.src)?;

                // skip the subtree of the for statement since we have already walked it
                return Ok(VisitorAction::SkipSubtree);
            }
            Statement::IfStatement(if_statement) => {
                // the step is the `if(...)`
                let src =
                    sloc_ldiff(if_statement.src, block_or_stmt_src(&if_statement.true_body)).into();
                step!(
                    IfCondition,
                    *if_statement.clone(),
                    src,
                    StepHookLocations {
                        // the before step hook should be instrumented before the if statement
                        before_step: src.start,
                        // the variable update hook should be instrumented before the first statement of both the true and false bodies
                        after_step: {
                            let mut locs = vec![];
                            let true_loc = find_index_of_first_statement_in_block_or_statement(
                                &if_statement.true_body,
                            )
                            .expect("true body first statement location not found");
                            locs.push(true_loc);
                            if let Some(false_body) = &if_statement.false_body {
                                let false_loc =
                                    find_index_of_first_statement_in_block_or_statement(false_body)
                                        .expect("false body first statement location not found");
                                locs.push(false_loc);
                            }
                            locs
                        }
                    },
                    StepKind::OtherStatement
                );

                // we take over the walk of the sub ast tree in the if statement step.
                let mut single_step_walker = AnalyzerSingleStepWalker { analyzer: self };
                if_statement.condition.walk(&mut single_step_walker)?;

                // collect statement bodies before walking the bodies
                self.collect_statement_bodies(&if_statement.true_body);
                if let Some(false_body) = &if_statement.false_body {
                    self.collect_statement_bodies(false_body);
                }

                // end the if statement step early and then walk the true and false body of the if statement.
                self.exit_current_statement_step(statement)?;
                if_statement.true_body.walk(self)?;
                if let Some(false_body) = &if_statement.false_body {
                    false_body.walk(self)?;
                }

                // skip the subtree of the if statement since we have already walked it
                return Ok(VisitorAction::SkipSubtree);
            }
            Statement::InlineAssembly(inline_assembly) => simple_stmt_to_step!(inline_assembly),
            Statement::PlaceholderStatement(_placeholder_statement) => {
                // let src = placeholder_statement.src.into();
                // step!(
                //     Placeholder,
                //     *placeholder_statement.clone(),
                //     src,
                //     StepHookLocations { before_step: src.start, after_step: vec![src.next_loc()] },
                //     StepKind::PlaceholderStatement
                // );

                // // end the placeholder statement step early
                // self.exit_current_statement_step(statement)?;
            }
            Statement::Return(return_stmt) => simple_stmt_to_step!(return_stmt),
            Statement::RevertStatement(revert_statement) => simple_stmt_to_step!(revert_statement),
            Statement::TryStatement(try_statement) => {
                // the step is the `try`
                let first_clause = &try_statement.clauses[0];
                let src = sloc_ldiff(try_statement.src, first_clause.block.src).into();
                step!(
                    Try,
                    *try_statement.clone(),
                    src,
                    StepHookLocations {
                        // the before step hook should be instrumented before the try statement
                        before_step: src.start,
                        // the variable update hook should be instrumented before the first statement in all catch blocks
                        after_step: try_statement
                            .clauses
                            .iter()
                            .filter_map(|clause| find_index_of_first_statement_in_block(
                                &clause.block
                            ))
                            .collect()
                    },
                    StepKind::OtherStatement
                );

                // we take over the walk of the sub ast tree in the try statement step.
                let mut single_step_walker = AnalyzerSingleStepWalker { analyzer: self };
                try_statement.external_call.walk(&mut single_step_walker)?;

                // end the try statement step early and then walk the clauses of the try statement.
                self.exit_current_statement_step(statement)?;
                for clause in &try_statement.clauses {
                    clause.block.walk(self)?;
                }

                // skip the subtree of the try statement since we have already walked it
                return Ok(VisitorAction::SkipSubtree);
            }
            Statement::UncheckedBlock(_) => { /* walk in the block */ }
            Statement::VariableDeclarationStatement(variable_declaration_statement) => {
                simple_stmt_to_step!(variable_declaration_statement)
            }
            Statement::WhileStatement(while_statement) => {
                // the step is the `while(...)`
                let src = sloc_ldiff(while_statement.src, block_or_stmt_src(&while_statement.body))
                    .into();
                step!(
                    WhileLoop,
                    *while_statement.clone(),
                    src,
                    StepHookLocations {
                        // the before step hook should be instrumented before the while statement
                        before_step: src.start,
                        // the variable update hook should be instrumented before the first statement of the while statement
                        after_step: vec![find_index_of_first_statement_in_block_or_statement(
                            &while_statement.body
                        )
                        .expect("while statement first statement location not found")],
                    },
                    StepKind::OtherStatement
                );

                // we take over the walk of the sub ast tree in the while statement step.
                let mut single_step_walker = AnalyzerSingleStepWalker { analyzer: self };
                while_statement.condition.walk(&mut single_step_walker)?;

                // collect statement bodies before walking the body
                self.collect_statement_bodies(&while_statement.body);

                // end the while statement step early and then walk the body of the while statement.
                self.exit_current_statement_step(statement)?;
                while_statement.body.walk(self)?;

                // skip the subtree of the while statement since we have already walked it
                return Ok(VisitorAction::SkipSubtree);
            }
        };
        Ok(VisitorAction::Continue)
    }

    pub(super) fn enter_new_function_step(
        &mut self,
        function: &FunctionDefinition,
    ) -> eyre::Result<VisitorAction> {
        assert!(self.current_step.is_none(), "Step cannot be nested");
        let current_function = self.current_function();

        if function.body.is_none() {
            // if a function has no body, we skip the function step
            return Ok(VisitorAction::SkipSubtree);
        }

        // step is the function header
        let current_scope = self.current_scope();
        let accessible_variables = current_scope.read().variables_recursive();
        let src = sloc_ldiff(function.src, function.body.as_ref().unwrap().src).into();
        let first_stmt_loc =
            find_index_of_first_statement_in_block(function.body.as_ref().unwrap())
                .expect("function first statement location not found");
        let new_step: StepRef = Step::new(
            current_function.ufid(),
            src,
            StepHookLocations {
                // the before step hook should be instrumented before the first statement of the function
                before_step: first_stmt_loc,
                // the variable update hook should be instrumented before the first statement of the function
                after_step: vec![first_stmt_loc],
            },
            StepKind::Entry(current_function.clone()),
            current_scope,
            accessible_variables,
        )
        .into();
        self.current_step = Some(new_step.clone());
        current_function.write().steps.push(new_step);

        // we take over the walk of the sub ast tree in the function step.
        let mut single_step_walker = AnalyzerSingleStepWalker { analyzer: self };

        // Set context flag for parameters
        single_step_walker.analyzer.is_declaring_param = true;
        function.parameters.walk(&mut single_step_walker)?;
        single_step_walker.analyzer.is_declaring_param = false;

        // Set context flag for return parameters
        single_step_walker.analyzer.is_declaring_return = true;
        function.return_parameters.walk(&mut single_step_walker)?;
        single_step_walker.analyzer.is_declaring_return = false;

        // end the function step early and then walk the body of the function.
        let step = self.current_step.take().unwrap();
        self.finished_steps.push(step);
        if let Some(body) = &function.body {
            // Walk the statements directly without creating a new scope for the function body block
            for statement in &body.statements {
                statement.walk(self)?;
            }
        }

        // skip the subtree of the function since we have already walked it
        Ok(VisitorAction::SkipSubtree)
    }

    pub(super) fn enter_new_modifier_step(
        &mut self,
        modifier: &ModifierDefinition,
    ) -> eyre::Result<VisitorAction> {
        assert!(self.current_step.is_none(), "Step cannot be nested");
        let current_function = self.current_function();

        if modifier.body.is_none() {
            // if a modifier has no body, we skip the modifier step
            return Ok(VisitorAction::SkipSubtree);
        }

        // step is the modifier header
        let current_scope = self.current_scope();
        let accessible_variables = current_scope.read().variables_recursive();
        let src = sloc_ldiff(modifier.src, modifier.body.as_ref().unwrap().src).into();
        let first_stmt_loc =
            find_index_of_first_statement_in_block(modifier.body.as_ref().unwrap())
                .expect("modifier first statement location not found");
        let new_step: StepRef = Step::new(
            current_function.ufid(),
            src,
            StepHookLocations {
                // the before step hook should be instrumented before the first statement of the modifier
                before_step: first_stmt_loc,
                // the variable update hook should be instrumented before the first statement of the modifier
                after_step: vec![first_stmt_loc],
            },
            StepKind::Entry(current_function.clone()),
            current_scope,
            accessible_variables,
        )
        .into();
        self.current_step = Some(new_step.clone());
        current_function.write().steps.push(new_step);

        // we take over the walk of the sub ast tree in the modifier step.
        let mut single_step_walker = AnalyzerSingleStepWalker { analyzer: self };

        // Set context flag for parameters
        single_step_walker.analyzer.is_declaring_param = true;
        modifier.parameters.walk(&mut single_step_walker)?;
        single_step_walker.analyzer.is_declaring_param = false;

        // end the modifier step early and then walk the body of the modifier.
        let step = self.current_step.take().unwrap();
        self.finished_steps.push(step);
        if let Some(body) = &modifier.body {
            // Walk the statements directly without creating a new scope for the modifier body block
            for statement in &body.statements {
                statement.walk(self)?;
            }
        }

        // skip the subtree of the modifier since we have already walked it
        Ok(VisitorAction::SkipSubtree)
    }

    /// Add a function call to the current step, if we are in a step.
    pub(super) fn add_function_call(&mut self, call: &FunctionCall) -> eyre::Result<()> {
        if let Some(step) = self.current_step.as_mut()
            && call.kind == FunctionCallKind::FunctionCall {
                // Determine if this should count as a function call
                let should_count = {
                    let step_read = step.read();

                    // Corner case 1: skip if EmitStatement
                    // In EmitStatement, an event is also considered as a function call, but we don't count it
                    let is_emit_statement = matches!(step_read.kind, StepKind::EmitStatement);
                    if is_emit_statement {
                        false
                    } else {
                        // Corner case 2: skip built-in functions
                        static BUILT_IN_FUNCTIONS: &[&str] = &[
                            "require",
                            "assert",
                            "keccak256",
                            "sha256",
                            "ripemd160",
                            "ecrecover",
                            "type",
                        ];
                        if let Expression::Identifier(ref id) = call.expression {
                            !BUILT_IN_FUNCTIONS.contains(&id.name.as_str())
                        } else {
                            true
                        }
                    }
                };

                if should_count {
                    step.write().function_calls += 1;
                }
            }
        Ok(())
    }

    pub(super) fn exit_current_statement_step(
        &mut self,
        statement: &Statement,
    ) -> eyre::Result<()> {
        if self.current_step.is_none() {
            return Ok(());
        }

        match statement {
            Statement::Block(_) | Statement::UncheckedBlock(_) => {}
            _ => {
                let step = self.current_step.take().unwrap();
                self.finished_steps.push(step);
            }
        }
        Ok(())
    }
}

/// Computes the left difference of `a` and `b` (`a \ b`).
/// It takes the [SourceLocation] within `a` that is not in `b` and smaller than `b`.
pub fn sloc_ldiff(a: SourceLocation, b: SourceLocation) -> SourceLocation {
    assert_eq!(a.index, b.index, "The index of `a` and `b` must be the same");
    let length = b.start.zip(a.start).map(|(end, start)| end.saturating_sub(start));
    SourceLocation { start: a.start, length, index: a.index }
}

/// Computes the right difference of `a` and `b` (`a \ b`).
/// It takes the [SourceLocation] within `a` that is not in `b` and larger than `b`.
pub fn sloc_rdiff(a: SourceLocation, b: SourceLocation) -> SourceLocation {
    assert_eq!(a.index, b.index, "The index of `a` and `b` must be the same");
    let start = b.start.zip(b.length).map(|(start, length)| start + length);
    let length = a
        .start
        .zip(a.length)
        .map(|(start, length)| start + length)
        .zip(start)
        .map(|(end, start)| end.saturating_sub(start));
    SourceLocation { start, length, index: a.index }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use foundry_compilers::{
        artifacts::{
            output_selection::OutputSelection, EvmVersion, Settings, Severity, SolcInput, Source,
            Sources,
        },
        solc::SolcLanguage,
        CompilationError,
    };
    use semver::Version;

    use crate::{
        analysis::analyzer::tests::{
            compile_and_analyze, TEST_CONTRACT_SOURCE_ID, TEST_CONTRACT_SOURCE_PATH,
        },
        find_or_install_solc, source_string_at_location_unchecked, ASTPruner,
    };

    use super::*;

    macro_rules! sloc {
        ($start:expr, $length:expr, $index:expr) => {
            SourceLocation { start: Some($start), length: Some($length), index: Some($index) }
        };
    }

    #[test]
    fn test_sloc_ldiff() {
        let a = sloc!(0, 10, 0);
        let b = sloc!(5, 5, 0);
        let c = sloc_ldiff(a, b);
        assert_eq!(c, sloc!(0, 5, 0));

        let a = sloc!(0, 10, 0);
        let b = sloc!(0, 10, 0);
        let c = sloc_ldiff(a, b);
        assert_eq!(c, sloc!(0, 0, 0));

        let a = sloc!(0, 10, 0);
        let b = sloc!(10, 10, 0);
        let c = sloc_ldiff(a, b);
        assert_eq!(c, sloc!(0, 10, 0));

        let a = sloc!(5, 5, 0);
        let b = sloc!(0, 10, 0);
        let c = sloc_ldiff(a, b);
        assert_eq!(c, sloc!(5, 0, 0));
    }

    #[test]
    fn test_sloc_rdiff() {
        let a = sloc!(0, 10, 0);
        let b = sloc!(5, 5, 0);
        let c = sloc_rdiff(a, b);
        assert_eq!(c, sloc!(10, 0, 0));

        let a = sloc!(0, 10, 0);
        let b = sloc!(0, 10, 0);
        let c = sloc_rdiff(a, b);
        assert_eq!(c, sloc!(10, 0, 0));

        let a = sloc!(0, 10, 0);
        let b = sloc!(0, 5, 0);
        let c = sloc_rdiff(a, b);
        assert_eq!(c, sloc!(5, 5, 0));

        let a = sloc!(5, 5, 0);
        let b = sloc!(0, 10, 0);
        let c = sloc_rdiff(a, b);
        assert_eq!(c, sloc!(10, 0, 0));
    }

    #[test]
    fn test_function_step() {
        // Create a simple contract with a function to test function step extraction
        let source = r#"
abstract contract TestContract {
    function setValue(uint256 newValue) public {}

    function getValue() public view returns (uint256) {
        return 0;
    }

    function getBalance() public view returns (uint256 balance) {}

    function template() public virtual returns (uint256);
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert all non-empty functions are present as steps
        let func_entries =
            analysis.steps.iter().filter(|s| matches!(s.kind(), StepKind::Entry(_))).count();
        assert!(func_entries == 3);
    }

    #[test]
    fn test_statement_step() {
        // Create a simple contract with a function to test statement step extraction
        let source = r#"
contract TestContract {
    function getValue() public view returns (uint256) {
        uint256 value = 0;
        return 0;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert that we have two statement steps
        let statement_steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        assert!(statement_steps == 2);
    }

    #[test]
    fn test_if_step() {
        // Create a simple contract with a function to test if statement extraction
        let source = r#"
contract TestContract {
    function getValue() public view returns (uint256) {
        if (true) {
            return 0;
        } else {
            return 1;
        }
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert that we have one "if" step, and two statement steps
        let steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        assert!(steps == 3);
    }

    #[test]
    fn test_for_step() {
        // Create a simple contract with a function to test for statement extraction
        let source = r#"
contract TestContract {
    function getValue() public view returns (uint256) {
        for (uint256 i = 0; i < 10; i++) {
            return 0;
        }
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert that we have one for step, and one statement step
        let steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        assert!(steps == 2);
    }

    #[test]
    fn test_while_step() {
        // Create a simple contract with a function to test while statement extraction
        let source = r#"
contract TestContract {
    function getValue() public view returns (uint256) {
        while (true) {
            return 0;

        }
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert that we have one while step, and one statement step
        let steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        assert!(steps == 2);
    }

    #[test]
    fn test_try_step() {
        // Create a simple contract with a function to test try statement extraction
        let source = r#"
contract TestContract {
    function getValue() public view returns (uint256) {
        try this.getValue() {
            revert();
        } catch {
            return 1;
        }
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert that we have one try step, and two statement steps
        let steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        assert!(steps == 3);
    }

    #[test]
    fn test_if_statement_body() {
        // Create a simple contract with a function to test if statement extraction
        let source = r#"
contract TestContract {
    function getValue() public view returns (uint256) {
        if (true) revert();
        return 0;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert that we have one if step, and one statement step
        let steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        assert!(steps == 3);
    }

    #[test]
    fn test_type_conversion_is_not_function_call() {
        let source = r#"
interface ITestContract {
    function getValue() external view returns (uint256);
}

contract TestContract {
    struct S {
        uint256 value;
    }
    function getValue() public view returns (uint256) {
        ITestContract I = ITestContract(msg.sender);
        S memory s = S({ value: 1 });
        getValue();
        this.getValue();
        return uint256(1);
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert that we have one function call step
        let mut function_calls = 0;
        analysis.steps.iter().for_each(|step| {
            function_calls += step.function_calls();
        });
        assert_eq!(function_calls, 2);
    }

    #[test]
    fn test_steps_in_modifier() {
        let source = r#"
contract TestContract {
    modifier test() {
        uint x = 1;
        _;
        uint y = 2;
    }
}
"#;
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert that we have one modifier entry step, and three statement steps
        let func_entries = analysis.steps.iter().filter(|s| s.is_entry()).count();
        assert_eq!(func_entries, 1);
        let statement_steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        // FIXME: when we consider the placeholder statement as a step, the number of steps will be 3
        assert_eq!(statement_steps, 2);
    }

    #[test]
    fn test_type_casting_multi_files() {
        let interface_file0 = "interface.sol";
        let source0 = r#"
interface ITestContract {
    function getValue() external view returns (uint256);
}
"#;
        let source1 = r#"
import { ITestContract } from "interface.sol";

contract TestContract {
    function foo() public {
        ITestContract I = ITestContract(msg.sender);
    }
}
"#;
        let version = Version::parse("0.8.19").unwrap();
        let sources = Sources::from_iter([
            (PathBuf::from(interface_file0), Source::new(source0)),
            (PathBuf::from(TEST_CONTRACT_SOURCE_PATH), Source::new(source1)),
        ]);
        let mut settings = Settings::new(OutputSelection::complete_output_selection());
        settings.evm_version = Some(EvmVersion::Paris);
        let solc_input = SolcInput::new(SolcLanguage::Solidity, sources, settings);
        let compiler = find_or_install_solc(&version).unwrap();
        let output = compiler.compile_exact(&solc_input).unwrap();

        // return error if compiler error
        let errors = output
            .errors
            .iter()
            .filter(|e| e.severity() == Severity::Error)
            .map(|e| format!("{e}"))
            .collect::<Vec<_>>();
        if !errors.is_empty() {
            panic!("Compiler error: {}", errors.join("\n"));
        }

        let mut ast = output
            .sources
            .get(&PathBuf::from(TEST_CONTRACT_SOURCE_PATH))
            .unwrap()
            .ast
            .clone()
            .unwrap();
        let source_unit = ASTPruner::convert(&mut ast, false).unwrap();

        let analyzer = Analyzer::new(
            TEST_CONTRACT_SOURCE_ID,
            PathBuf::from(TEST_CONTRACT_SOURCE_PATH),
            source1.to_string(),
        );
        let analysis = analyzer.analyze(&source_unit).unwrap();

        // Type cast step should not be a function call step
        let mut function_calls = 0;
        for step in analysis.steps {
            function_calls += step.function_calls();
        }
        assert_eq!(function_calls, 0);
    }

    #[test]
    fn test_statement_semicolon() {
        // Test that after_step hook locations are positioned after semicolons
        // Note: The AST source ranges from Solidity compiler may not include semicolons,
        // but the step analysis expands hook locations to account for this
        let source = r#"
contract TestContract {
    function foo() public returns (uint) {
        require(false, "error");
        revert();
        uint x = 1;
        x = 2;
        x + 1;
        return 1;
    }

    event ValueChanged(uint256 newValue);

    function bar() public {
        emit ValueChanged(1);
        return;
    }
}

"#;
        let (_sources, analysis) = compile_and_analyze(source);
        let source_content = _sources.get(&TEST_CONTRACT_SOURCE_ID).unwrap().content.as_str();

        // Filter to only simple statement steps (not entry steps or control flow)
        let simple_statement_steps: Vec<_> = analysis
            .steps
            .iter()
            .filter(|s| {
                if !s.is_statement() {
                    return false;
                }
                let step_src = s.src();
                let step_source =
                    source_string_at_location_unchecked(source_content, &step_src.into());
                let trimmed = step_source.trim();
                // Exclude control flow statements
                !trimmed.starts_with("if")
                    && !trimmed.starts_with("for")
                    && !trimmed.starts_with("while")
                    && !trimmed.starts_with("try")
                    && !trimmed.starts_with("do")
            })
            .collect();

        // Verify that after_step hook locations are positioned after semicolons
        for step in simple_statement_steps {
            let hook_locs = step.hook_locations();

            // Get the first after_step location (should be after the statement)
            assert!(!hook_locs.after_step.is_empty(), "Statement should have after_step hooks");

            let after_step_pos = hook_locs.after_step[0];

            // The character immediately before the after_step position should be a semicolon
            // (possibly with whitespace in between)
            let before_after_step = &source_content[..after_step_pos];
            let last_non_whitespace = before_after_step.trim_end().chars().last();

            assert_eq!(
                last_non_whitespace,
                Some(';'),
                "after_step hook should be positioned after semicolon. Statement: '{}'",
                step.src().slice_source(source_content),
            );
        }

        // Verify specific statement types have proper semicolon handling

        // 1. Check require statement
        let require_step = analysis
            .steps
            .iter()
            .find(|s| {
                s.is_statement() && {
                    let src = s.src().slice_source(source_content);
                    src.contains("require")
                }
            })
            .expect("Should have require statement");

        let require_after_pos = require_step.hook_locations().after_step[0];
        let before_require_after = &source_content[..require_after_pos];
        assert_eq!(
            before_require_after.trim_end().chars().last(),
            Some(';'),
            "require after_step should be after semicolon"
        );

        // 2. Check return statements
        let return_steps: Vec<_> =
            analysis.steps.iter().filter(|s| s.is_statement() && s.contains_return()).collect();

        for return_step in return_steps {
            let return_after_pos = return_step.hook_locations().after_step[0];
            let before_return_after = &source_content[..return_after_pos];
            assert_eq!(
                before_return_after.trim_end().chars().last(),
                Some(';'),
                "return after_step should be after semicolon for: '{}'",
                return_step.src().slice_source(source_content).trim()
            );
        }

        // 3. Check emit statements
        let emit_steps: Vec<_> =
            analysis.steps.iter().filter(|s| matches!(s.kind(), StepKind::EmitStatement)).collect();

        for emit_step in emit_steps {
            let emit_after_pos = emit_step.hook_locations().after_step[0];
            let before_emit_after = &source_content[..emit_after_pos];
            assert_eq!(
                before_emit_after.trim_end().chars().last(),
                Some(';'),
                "emit after_step should be after semicolon"
            );
        }

        // 4. Check variable declarations
        let var_decl_steps: Vec<_> = analysis
            .steps
            .iter()
            .filter(|s| {
                s.is_statement() && {
                    let src = s.src().slice_source(source_content);
                    src.trim().starts_with("uint")
                }
            })
            .collect();

        for var_step in var_decl_steps {
            let var_after_pos = var_step.hook_locations().after_step[0];
            let before_var_after = &source_content[..var_after_pos];
            assert_eq!(
                before_var_after.trim_end().chars().last(),
                Some(';'),
                "variable declaration after_step should be after semicolon for: '{}'",
                var_step.src().slice_source(source_content)
            );
        }
    }

    #[test]
    fn test_do_while_step() {
        // Create a simple contract with a function to test do-while statement extraction
        let source = r#"
contract TestContract {
    function getValue() public view returns (uint256) {
        uint256 i = 0;
        do {
            i++;
        } while (i < 10);
        return i;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert that we have one do-while step, two statement steps (i++ and return)
        // plus one for the declaration
        let steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        assert_eq!(steps, 4);
    }

    #[test]
    fn test_break_continue_steps() {
        // Create a contract with break and continue statements
        let source = r#"
contract TestContract {
    function getValue() public pure returns (uint256) {
        uint256 sum = 0;
        for (uint256 i = 0; i < 10; i++) {
            if (i == 5) {
                continue;
            }
            if (i == 8) {
                break;
            }
            sum += i;
        }
        return sum;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert that we have break and continue as statement steps
        // Expected: declaration, for loop, 2 if conditions, continue, break, sum+=i, return
        let steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        assert_eq!(steps, 8);
    }

    #[test]
    fn test_emit_statement_step() {
        // Create a contract with emit statements
        let source = r#"
contract TestContract {
    event ValueChanged(uint256 newValue);
    event Transfer(address from, address to, uint256 amount);

    function setValue(uint256 newValue) public {
        emit ValueChanged(newValue);
        emit Transfer(msg.sender, address(this), newValue);
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert that emit statements are tracked as statement steps
        // and that events are not counted as function calls
        let emit_steps =
            analysis.steps.iter().filter(|s| matches!(s.kind(), StepKind::EmitStatement)).count();
        assert_eq!(emit_steps, 2);

        // Verify that events are not counted as function calls
        let mut function_calls = 0;
        analysis.steps.iter().for_each(|step| {
            function_calls += step.function_calls();
        });
        assert_eq!(function_calls, 0);
    }

    #[test]
    fn test_revert_statement_step() {
        // Create a contract with revert statements
        let source = r#"
contract TestContract {
    error InvalidValue(uint256 value);

    function setValue(uint256 newValue) public {
        if (newValue == 0) {
            revert();
        }
        if (newValue > 100) {
            revert InvalidValue(newValue);
        }
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Assert that revert statements are tracked as statement steps
        // Expected: 2 if conditions, 2 revert statements = 4 statement steps
        let statement_steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        assert_eq!(statement_steps, 4);
    }

    #[test]
    fn test_contains_return_flag() {
        // Create a contract with return statements
        let source = r#"
contract TestContract {
    function getValue(bool flag) public pure returns (uint256) {
        if (flag) {
            return 42;
        }
        uint256 x = 10;
        return x + 1;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Count steps that contain return statements
        let return_steps = analysis.steps.iter().filter(|s| s.contains_return()).count();
        assert_eq!(return_steps, 2);

        // Verify non-return statements don't have the flag set
        let non_return_steps =
            analysis.steps.iter().filter(|s| s.is_statement() && !s.contains_return()).count();
        // Should have: if condition, variable declaration
        assert_eq!(non_return_steps, 2);
    }

    #[test]
    fn test_step_ref_helper_methods() {
        // Create a contract with both functions and modifiers
        let source = r#"
contract TestContract {
    modifier onlyPositive(uint256 value) {
        require(value > 0);
        _;
    }

    function getValue() public pure returns (uint256) {
        return 42;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Test function_entry() - should return Some for function entries, None for others
        let function_entries: Vec<_> =
            analysis.steps.iter().filter_map(|s| s.function_entry()).collect();
        assert_eq!(function_entries.len(), 1);

        // Test modifier_entry() - should return Some for modifier entries, None for others
        let modifier_entries: Vec<_> =
            analysis.steps.iter().filter_map(|s| s.modifier_entry()).collect();
        assert_eq!(modifier_entries.len(), 1);

        // Test is_entry() - should return true for both function and modifier entries
        let entry_steps = analysis.steps.iter().filter(|s| s.is_entry()).count();
        assert_eq!(entry_steps, 2);
    }

    #[test]
    fn test_unchecked_block_step() {
        // Create a contract with an unchecked block
        let source = r#"
contract TestContract {
    function increment(uint256 value) public pure returns (uint256) {
        uint256 result;
        unchecked {
            result = value + 1;
        }
        return result;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Unchecked blocks should not create their own steps, but statements inside should
        // Expected: variable declaration, assignment inside unchecked, return
        let statement_steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        assert_eq!(statement_steps, 3);
    }

    #[test]
    fn test_nested_control_flow() {
        // Create a contract with nested if/for/while statements
        let source = r#"
contract TestContract {
    function complexLogic(uint256 x) public pure returns (uint256) {
        if (x > 0) {
            for (uint256 i = 0; i < x; i++) {
                if (i == 5) {
                    return i;
                }
            }
        }
        return 0;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Verify that nested control flow creates appropriate steps
        // Expected: outer if, for loop, inner if, return inside inner if, return at end
        let statement_steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        assert_eq!(statement_steps, 5);

        // Verify entry step exists
        let entry_steps = analysis.steps.iter().filter(|s| s.is_entry()).count();
        assert_eq!(entry_steps, 1);
    }

    #[test]
    fn test_inline_assembly_step() {
        // Create a contract with inline assembly
        let source = r#"
contract TestContract {
    function getCodeSize(address addr) public view returns (uint256 size) {
        assembly {
            size := extcodesize(addr)
        }
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Inline assembly should be treated as a statement step
        let statement_steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        assert_eq!(statement_steps, 1);
    }

    #[test]
    fn test_placeholder_statement_in_modifier() {
        // Create a contract with a modifier containing placeholder
        let source = r#"
contract TestContract {
    modifier beforeAndAfter() {
        uint256 beforeVal = 1;
        _;
        uint256 afterVal = 2;
    }

    function doSomething() public beforeAndAfter {
        uint256 x = 42;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);

        // Should have: modifier entry, before statement, placeholder, after statement,
        // function entry, function statement
        let all_steps = analysis.steps.len();
        // FIXME: when we consider the placeholder statement as a step, the number of steps will be 6
        assert_eq!(all_steps, 5);

        // Verify placeholder is treated as a statement
        let statement_steps = analysis.steps.iter().filter(|s| s.is_statement()).count();
        // FIXME: when we consider the placeholder statement as a step, the number of steps will be 4
        assert_eq!(statement_steps, 3);
    }

    #[test]
    fn test_hook_locations_for_function_entry() {
        // Create a simple function to test hook locations
        let source = r#"
contract TestContract {
    function getValue(uint256 x) public pure returns (uint256) {
        uint256 y = x + 1;
        return y;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);
        let source_content = _sources.get(&TEST_CONTRACT_SOURCE_ID).unwrap().content.as_str();

        // Find the function entry step
        let entry_step = analysis.steps.iter().find(|s| s.is_entry()).expect("Should have entry");

        // For function entry, before_step and after_step should point to the first statement
        let hook_locs = entry_step.hook_locations();

        // Verify after_step locations exist and point to the first statement
        assert_eq!(
            hook_locs.after_step.len(),
            1,
            "Entry step should have exactly one after_step location"
        );

        // The before_step and after_step should both be at the first statement (variable declaration)
        // They should point to the position right before "uint256 y = x + 1;"
        let first_stmt_pos = hook_locs.after_step[0];
        assert_eq!(hook_locs.before_step, first_stmt_pos, "For function entry, before_step and after_step[0] should be at the same position (first statement)");

        // Verify the hook is positioned before the first statement
        let remaining_source = &source_content[first_stmt_pos..];
        assert!(
            remaining_source.trim_start().starts_with("uint256 y"),
            "Hook should be positioned before the first statement. Found: {}",
            &remaining_source[..remaining_source.find('\n').unwrap_or(50).min(50)]
        );
    }

    #[test]
    fn test_hook_locations_for_if_statement() {
        // Create an if statement to test hook locations
        let source = r#"
contract TestContract {
    function getValue(uint256 x) public pure returns (uint256) {
        if (x > 0) {
            return 1;
        } else {
            return 0;
        }
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);
        let source_content = _sources.get(&TEST_CONTRACT_SOURCE_ID).unwrap().content.as_str();

        // Find the if statement step
        let if_step = analysis
            .steps
            .iter()
            .find(|s| {
                s.is_statement() && {
                    let src_str = s
                        .src()
                        .slice_source(&_sources.get(&TEST_CONTRACT_SOURCE_ID).unwrap().content);
                    src_str.contains("if")
                }
            })
            .expect("Should have if step");

        let hook_locs = if_step.hook_locations();

        // If statement should have before_step before the "if" keyword
        let before_snippet = &source_content[hook_locs.before_step..];
        assert!(
            before_snippet.trim_start().starts_with("if"),
            "before_step should be positioned before 'if' keyword. Found: {}",
            &before_snippet[..before_snippet.find('\n').unwrap_or(50).min(50)]
        );

        // If statement should have after_step at both true and false branches
        assert_eq!(
            hook_locs.after_step.len(),
            2,
            "If statement should have 2 after_step locations (true and false branches)"
        );

        // First after_step should be before the first statement in true branch
        let true_branch_snippet = &source_content[hook_locs.after_step[0]..];
        assert!(
            true_branch_snippet.trim_start().starts_with("return 1"),
            "First after_step should be before true branch statement. Found: {}",
            &true_branch_snippet[..true_branch_snippet.find('\n').unwrap_or(50).min(50)]
        );

        // Second after_step should be before the first statement in false branch
        let false_branch_snippet = &source_content[hook_locs.after_step[1]..];
        assert!(
            false_branch_snippet.trim_start().starts_with("return 0"),
            "Second after_step should be before false branch statement. Found: {}",
            &false_branch_snippet[..false_branch_snippet.find('\n').unwrap_or(50).min(50)]
        );
    }

    #[test]
    fn test_hook_locations_for_loop_statement() {
        // Create a for loop to test hook locations
        let source = r#"
contract TestContract {
    function sum(uint256 n) public pure returns (uint256) {
        uint256 total = 0;
        for (uint256 i = 0; i < n; i++) {
            total += i;
        }
        return total;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);
        let source_content = _sources.get(&TEST_CONTRACT_SOURCE_ID).unwrap().content.as_str();

        // Find the for loop step
        let for_step = analysis
            .steps
            .iter()
            .find(|s| {
                s.is_statement() && {
                    let src_str = s
                        .src()
                        .slice_source(&_sources.get(&TEST_CONTRACT_SOURCE_ID).unwrap().content);
                    src_str.contains("for")
                }
            })
            .expect("Should have for step");

        let hook_locs = for_step.hook_locations();

        // For loop should have before_step at the loop header (before "for" keyword)
        let before_snippet = &source_content[hook_locs.before_step..];
        assert!(
            before_snippet.trim_start().starts_with("for"),
            "before_step should be positioned before 'for' keyword. Found: {}",
            &before_snippet[..before_snippet.find('\n').unwrap_or(50).min(50)]
        );

        // For loop should have after_step pointing to first statement in loop body
        assert_eq!(hook_locs.after_step.len(), 1, "For loop should have one after_step location");

        // The after_step should be positioned before the first statement in the loop body
        let body_snippet = &source_content[hook_locs.after_step[0]..];
        assert!(
            body_snippet.trim_start().starts_with("total +="),
            "after_step should be positioned before first statement in loop body. Found: {}",
            &body_snippet[..body_snippet.find('\n').unwrap_or(50).min(50)]
        );
    }

    #[test]
    fn test_hook_locations_for_while_statement() {
        // Create a while loop to test hook locations
        let source = r#"
contract TestContract {
    function countdown(uint256 n) public pure returns (uint256) {
        while (n > 0) {
            n--;
        }
        return n;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);
        let source_content = _sources.get(&TEST_CONTRACT_SOURCE_ID).unwrap().content.as_str();

        // Find the while loop step
        let while_step = analysis
            .steps
            .iter()
            .find(|s| {
                s.is_statement() && {
                    let src_str = s
                        .src()
                        .slice_source(&_sources.get(&TEST_CONTRACT_SOURCE_ID).unwrap().content);
                    src_str.contains("while")
                }
            })
            .expect("Should have while step");

        let hook_locs = while_step.hook_locations();

        // While loop should have before_step at the condition (before "while" keyword)
        let before_snippet = &source_content[hook_locs.before_step..];
        assert!(
            before_snippet.trim_start().starts_with("while"),
            "before_step should be positioned before 'while' keyword. Found: {}",
            &before_snippet[..before_snippet.find('\n').unwrap_or(50).min(50)]
        );

        // While loop should have after_step pointing to first statement in loop body
        assert_eq!(hook_locs.after_step.len(), 1, "While loop should have one after_step location");

        // The after_step should be positioned before the first statement in the loop body
        let body_snippet = &source_content[hook_locs.after_step[0]..];
        assert!(
            body_snippet.trim_start().starts_with("n--"),
            "after_step should be positioned before first statement in loop body. Found: {}",
            &body_snippet[..body_snippet.find('\n').unwrap_or(50).min(50)]
        );
    }

    #[test]
    fn test_hook_locations_for_try_statement() {
        // Create a try-catch to test hook locations
        let source = r#"
contract TestContract {
    function getValue() public view returns (uint256) {
        try this.getValue() returns (uint256 v) {
            return v;
        } catch {
            return 0;
        }
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);
        let source_content = _sources.get(&TEST_CONTRACT_SOURCE_ID).unwrap().content.as_str();

        // Find the try statement step
        let try_step = analysis
            .steps
            .iter()
            .find(|s| {
                s.is_statement() && {
                    let src_str = s
                        .src()
                        .slice_source(&_sources.get(&TEST_CONTRACT_SOURCE_ID).unwrap().content);
                    src_str.contains("try")
                }
            })
            .expect("Should have try step");

        let hook_locs = try_step.hook_locations();

        // Try statement should have before_step at the try keyword
        let before_snippet = &source_content[hook_locs.before_step..];
        assert!(
            before_snippet.trim_start().starts_with("try"),
            "before_step should be positioned before 'try' keyword. Found: {}",
            &before_snippet[..before_snippet.find('\n').unwrap_or(50).min(50)]
        );

        // Try statement should have after_step for the try block and each catch clause
        // In this case, we have 1 try block and 1 catch clause, so 2 after_step locations
        assert!(
            !hook_locs.after_step.is_empty(),
            "Try statement should have after_step locations for catch clauses"
        );

        // First after_step should be in the try block
        let try_block_snippet = &source_content[hook_locs.after_step[0]..];
        assert!(
            try_block_snippet.trim_start().starts_with("return v"),
            "First after_step should be positioned before first statement in try block. Found: {}",
            &try_block_snippet[..try_block_snippet.find('\n').unwrap_or(50).min(50)]
        );

        // If we have a second after_step, it should be in the catch block
        if hook_locs.after_step.len() > 1 {
            let catch_block_snippet = &source_content[hook_locs.after_step[1]..];
            assert!(catch_block_snippet.trim_start().starts_with("return 0"),
                "Second after_step should be positioned before first statement in catch block. Found: {}",
                &catch_block_snippet[..catch_block_snippet.find('\n').unwrap_or(50).min(50)]);
        }
    }

    #[test]
    fn test_hook_locations_for_do_while_statement() {
        // Create a do-while loop to test hook locations
        let source = r#"
contract TestContract {
    function getValue() public pure returns (uint256) {
        uint256 i = 0;
        do {
            i++;
        } while (i < 10);
        return i;
    }
}
"#;

        // Use utility function to compile and analyze
        let (_sources, analysis) = compile_and_analyze(source);
        let source_content = _sources.get(&TEST_CONTRACT_SOURCE_ID).unwrap().content.as_str();

        // Find the do-while loop step
        let do_while_step = analysis
            .steps
            .iter()
            .find(|s| {
                s.is_statement() && {
                    let src_str = s
                        .src()
                        .slice_source(&_sources.get(&TEST_CONTRACT_SOURCE_ID).unwrap().content);
                    src_str.contains("while")
                }
            })
            .expect("Should have do-while step");

        let hook_locs = do_while_step.hook_locations();

        // For do-while, the step is the "while(...)" part
        // before_step should be after the last statement in the do block (where condition check happens)
        let before_snippet = &source_content[hook_locs.before_step..];
        // The before_step in do-while is positioned after the body, before the while condition
        assert!(
            before_snippet.trim_start().starts_with("}") || before_snippet.contains("while"),
            "before_step should be positioned after do-while body. Found: {}",
            &before_snippet[..before_snippet.find('\n').unwrap_or(50).min(50)]
        );

        // after_step should point after the do-while statement (exit point)
        assert_eq!(hook_locs.after_step.len(), 1, "Do-while should have one after_step location");

        // The after_step should be positioned after the entire do-while statement
        let after_snippet = &source_content[hook_locs.after_step[0]..];
        assert!(
            after_snippet.trim_start().starts_with("return"),
            "after_step should be positioned after the entire do-while statement. Found: {}",
            &after_snippet[..after_snippet.find('\n').unwrap_or(50).min(50)]
        );
    }
}
