//! This module implements an analysis pass on AST that divdes the AST into steps.

use std::{collections::BTreeMap, sync::Arc};

use foundry_compilers::artifacts::{
    ast::SourceLocation, Expression, ExpressionOrVariableDeclarationStatement, FunctionDefinition,
    Statement,
};

use crate::analysis::{ast_visitor::Visitor, source_map::debug_unit::UnitLocation};

/// A step division defines the AST node and the source of that node forming a step.
///
/// The unit location is different from the source location of that node. For example,
/// If the Statement is a IfStatement, the source location contains the entire if block,
/// including block of its branches. However, the unit location only contains the
/// condition of the if statement since we will only step the condition during debugging.
///
/// There are two items in StepDivision:
/// - The first item is the AST node(s), before or after which we can insert a hook for that step (e.g., the IfStatement).
/// - The second item is the source of that node which are executed in a step (e.g., the condition of IfStatement).
#[derive(Debug)]
pub enum StepDivision {
    /// Normally, in each step we execute a single statement.
    Statement(Statement, UnitLocation),

    /// A list of consecutive statements. This is the case when we cannot distinctly execute a single statement.
    Statements(Vec<Statement>, UnitLocation),

    /// The step into a function.
    Function(FunctionDefinition, UnitLocation),
}

impl StepDivision {
    /// Get the source location of the step.
    pub fn source_location(&self) -> &UnitLocation {
        match self {
            StepDivision::Statement(_, loc) => loc,
            StepDivision::Statements(_, loc) => loc,
            StepDivision::Function(_, loc) => loc,
        }
    }
}

/// The step divider performs the analysis of step division.
#[derive(Debug)]
pub struct StepDivider {
    /// The mapping from source index to source code (as a string).
    pub sources: BTreeMap<usize, Arc<String>>,

    /// All collected steps.
    pub steps: Vec<StepDivision>,
}

impl StepDivider {
    /// Create a new step divider.
    pub fn new(sources: BTreeMap<usize, Arc<String>>) -> Self {
        Self { sources, steps: vec![] }
    }
}

impl StepDivider {
    /// Create unit location from source location by pretaining all source code.
    fn unit_location(&self, src: &SourceLocation) -> eyre::Result<UnitLocation> {
        let mut loc = UnitLocation::try_from(src)?;
        let source = self.sources.get(&loc.index).ok_or(eyre::eyre!("source not found"))?;
        let source = &source.as_bytes()[loc.start..loc.start + loc.length];
        loc.code = Arc::new(String::from_utf8_lossy(source).to_string());
        Ok(loc)
    }
}

impl Visitor for StepDivider {
    /// Visit each statement, construct a step for each of them unless they are block or unchecked block.
    fn visit_statement(&mut self, statement: &Statement) -> eyre::Result<()> {
        macro_rules! construct_stmt_step {
            ($stmt:expr, $src:expr) => {{
                let step = StepDivision::Statement($stmt.clone(), self.unit_location($src)?);
                self.steps.push(step);
            }};
        }
        match statement {
            Statement::Block(_) => {}
            Statement::Break(stmt) => construct_stmt_step!(statement, &stmt.src),
            Statement::Continue(stmt) => construct_stmt_step!(statement, &stmt.src),
            Statement::DoWhileStatement(stmt) => {
                // for do-while statement, we only step the condition
                let src = expression_source_location(&stmt.condition);
                construct_stmt_step!(statement, &src)
            }
            Statement::EmitStatement(stmt) => construct_stmt_step!(statement, &stmt.src),
            Statement::ExpressionStatement(stmt) => {
                let src = expression_source_location(&stmt.expression);
                construct_stmt_step!(statement, &src)
            }
            Statement::ForStatement(stmt) => {
                // for for statement, we only step the content in the brackets,
                // i.e., the initialization, condition, and loop expression
                let get_start = |src: &SourceLocation| src.start;
                let get_length = |src: &SourceLocation| src.length;
                // the source location of the content in the brackets
                let src = SourceLocation {
                    start: stmt
                        .initialization_expression
                        .as_ref()
                        .map(expression_or_variable_declaration_statement_source_location)
                        .map(get_start)
                        .flatten()
                        .or(stmt
                            .condition
                            .as_ref()
                            .map(expression_source_location)
                            .map(get_start)
                            .flatten())
                        .or(stmt
                            .loop_expression
                            .as_ref()
                            .map(|s| &s.expression)
                            .map(expression_source_location)
                            .map(get_start)
                            .flatten()),
                    length: Some(
                        stmt.initialization_expression
                            .as_ref()
                            .map(expression_or_variable_declaration_statement_source_location)
                            .map(get_length)
                            .flatten()
                            .unwrap_or_default()
                            + stmt
                                .condition
                                .as_ref()
                                .map(expression_source_location)
                                .map(get_length)
                                .flatten()
                                .unwrap_or_default()
                            + stmt
                                .loop_expression
                                .as_ref()
                                .map(|s| &s.expression)
                                .map(expression_source_location)
                                .map(get_length)
                                .flatten()
                                .unwrap_or_default(),
                    ),
                    index: stmt.src.index,
                };
                construct_stmt_step!(statement, &src)
            }
            Statement::IfStatement(stmt) => {
                // for if statement, we only step the condition
                let src = expression_source_location(&stmt.condition);
                construct_stmt_step!(statement, &src)
            }
            Statement::InlineAssembly(stmt) => construct_stmt_step!(statement, &stmt.src),
            Statement::PlaceholderStatement(stmt) => construct_stmt_step!(statement, &stmt.src),
            Statement::Return(stmt) => construct_stmt_step!(statement, &stmt.src),
            Statement::RevertStatement(stmt) => construct_stmt_step!(statement, &stmt.src),
            Statement::TryStatement(stmt) => {
                // for try statement, we step the function call
                let src = &stmt.external_call.src;
                construct_stmt_step!(statement, src)
            }
            Statement::UncheckedBlock(_) => {
                // for unchecked block, we look into the body of it.
            }
            Statement::VariableDeclarationStatement(stmt) => {
                construct_stmt_step!(statement, &stmt.src)
            }
            Statement::WhileStatement(stmt) => {
                // for while statement, we only step the condition
                let src = expression_source_location(&stmt.condition);
                construct_stmt_step!(statement, &src)
            }
        };
        Ok(())
    }

    fn visit_function_definition(&mut self, definition: &FunctionDefinition) -> eyre::Result<()> {
        let step = StepDivision::Function(definition.clone(), self.unit_location(&definition.src)?);
        self.steps.push(step);
        Ok(())
    }
}

fn expression_source_location<'a>(expr: &'a Expression) -> &'a SourceLocation {
    match expr {
        Expression::Assignment(expr) => &expr.src,
        Expression::BinaryOperation(expr) => &expr.src,
        Expression::Conditional(expr) => &expr.src,
        Expression::ElementaryTypeNameExpression(expr) => &expr.src,
        Expression::FunctionCall(expr) => &expr.src,
        Expression::FunctionCallOptions(expr) => &expr.src,
        Expression::Identifier(expr) => &expr.src,
        Expression::IndexAccess(expr) => &expr.src,
        Expression::IndexRangeAccess(expr) => &expr.src,
        Expression::Literal(expr) => &expr.src,
        Expression::MemberAccess(expr) => &expr.src,
        Expression::NewExpression(expr) => &expr.src,
        Expression::TupleExpression(expr) => &expr.src,
        Expression::UnaryOperation(expr) => &expr.src,
    }
}

fn expression_or_variable_declaration_statement_source_location<'a>(
    stmt: &'a ExpressionOrVariableDeclarationStatement,
) -> &'a SourceLocation {
    match stmt {
        ExpressionOrVariableDeclarationStatement::ExpressionStatement(stmt) => {
            expression_source_location(&stmt.expression)
        }
        ExpressionOrVariableDeclarationStatement::VariableDeclarationStatement(stmt) => &stmt.src,
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use crate::{
        analysis::ast_visitor::Walk,
        utils::compilation::compile_to_ast,
    };

    #[test]
    fn test_divide_normal_statements() {
        let code = r#"
            contract A {
                function foo() public returns (uint256) {
                    uint256 a = 1;
                    a = a + 1;
                    return a;
                }
            }
        "#;
        let (id, src) = compile_to_ast(code).expect("compilation failed");
        let sources = BTreeMap::from_iter(vec![(id as usize, Arc::new(code.to_string()))]);
        let mut divider = super::StepDivider::new(sources);
        src.walk(&mut divider).expect("walk failed");
        assert_eq!(divider.steps.len(), 4);
    }
}
