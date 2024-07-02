use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use eyre::{eyre, Result};
use foundry_compilers::artifacts::{
    ast::SourceLocation,
    visitor::{Visitor, Walk},
    yul::{YulExpression, YulStatement},
    ExpressionOrVariableDeclarationStatement, SourceUnitPart, Statement,
};

use crate::{
    artifact::compilation::CompilationArtifact,
    utils::ast::{get_source_location_for_expression, source_with_primative_statements},
};

#[derive(Clone, Debug)]
pub struct ValidSourceLocation {
    pub start: usize,
    pub length: usize,
    pub index: usize,
}

impl TryFrom<&SourceLocation> for ValidSourceLocation {
    type Error = eyre::Error;

    fn try_from(src: &SourceLocation) -> Result<Self, Self::Error> {
        let start = src.start.ok_or_else(|| eyre!("invalid source location"))? as usize;
        let length = src.length.ok_or_else(|| eyre!("invalid source location"))? as usize;
        let index = src.index.ok_or_else(|| eyre!("invalid source location"))? as usize;

        Ok(Self { start, length, index })
    }
}

/// A super statement is a collection of statements whose compiled opcode is fused together.
#[derive(Clone, Debug)]
pub struct SuperStatement<'a> {
    pub id: u32,
    pub location: ValidSourceLocation,
    pub code: Arc<String>,
    pub children: Vec<&'a SourceUnitPart>,
}

pub type PrimitiveStmts = BTreeMap<usize, BTreeMap<usize, ValidSourceLocation>>;

/// Visitor to collect all primative "statements".
///
/// A primative statement is a statement that does not contain any other statements (e.g. a block
/// statement). A primative statement can also be the condition of a loop or if statement.
/// Primative statements are the basic stepping blocks for debugging.
/// This visitor will collect all primative statements and their locations.
#[derive(Clone, Debug)]
struct PrimativeStmtVisitor(pub PrimitiveStmts);

impl PrimativeStmtVisitor {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }
}

impl Deref for PrimativeStmtVisitor {
    type Target = BTreeMap<usize, BTreeMap<usize, ValidSourceLocation>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for PrimativeStmtVisitor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Visitor for PrimativeStmtVisitor {
    fn visit_statement(&mut self, statement: &Statement) {
        // node_group! {
        //     Statement;
        //
        //     Block,
        //     Break,
        //     Continue,
        //     DoWhileStatement,
        //     EmitStatement,
        //     ExpressionStatement,
        //     ForStatement,
        //     IfStatement,
        //     InlineAssembly,
        //     PlaceholderStatement,
        //     Return,
        //     RevertStatement,
        //     TryStatement,
        //     UncheckedBlock,
        //     VariableDeclarationStatement,
        //     WhileStatement,
        //
        // }
        match statement {
            // Do nothing, since we are only interested in primative statements.
            // All the inner statements will be visited by the visitor later.
            Statement::Block(_) => {}
            // Do nothing, since we are only interested in primative statements.
            // All the inner statements will be visited by the visitor later.
            Statement::UncheckedBlock(_) => {}
            // For if statements, the condition is also a primative statement.
            // Note that other part, e.g., init, post, body, will be visited by the visitor
            // later.
            Statement::IfStatement(stmt) => {
                self.update(get_source_location_for_expression(&stmt.condition))
            }
            // For do-whiles, the condition is a primative statement.
            Statement::DoWhileStatement(stmt) => {
                self.update(get_source_location_for_expression(&stmt.condition))
            }
            // For while statements, the condition is also a primative statement.
            // Note that other part, e.g., body, will be visited by the visitor later.
            Statement::WhileStatement(stmt) => {
                self.update(get_source_location_for_expression(&stmt.condition))
            }
            // For for statements, the condition, the initial expression, and the loop expression
            // are also primative statements. Note that other part, e.g., body, will be
            // visited by the visitor later.
            Statement::ForStatement(stmt) => {
                if let Some(cond) = &stmt.condition {
                    self.update(get_source_location_for_expression(cond));
                }
                if let Some(init) = &stmt.initialization_expression {
                    match init {
                        ExpressionOrVariableDeclarationStatement::ExpressionStatement(stmt) => {
                            self.update(&stmt.src)
                        }
                        ExpressionOrVariableDeclarationStatement::VariableDeclarationStatement(
                            stmt,
                        ) => self.update(&stmt.src),
                    }
                }
                if let Some(loop_expr) = &stmt.loop_expression {
                    self.update(&loop_expr.src);
                }
            }
            // For try statement, we wil handle the external function call as a primative statement.
            // The catch and finally block will be visited by the visitor later.
            Statement::TryStatement(stmt) => {
                self.update(get_source_location_for_expression(&stmt.external_call.expression))
            }
            // We will provide more fine-grained information for inline assembly if the Yul block is
            // presented.
            Statement::InlineAssembly(stmt) => {
                if stmt.ast.statements.is_empty() {
                    self.update(&stmt.src);
                } else {
                    for yul_stmt in &stmt.ast.statements {
                        self.visit_yul_statment(yul_stmt);
                    }
                }
            }
            Statement::VariableDeclarationStatement(stmt) => self.update(&stmt.src),
            Statement::Break(stmt) => self.update(&stmt.src),
            Statement::Continue(stmt) => self.update(&stmt.src),
            Statement::EmitStatement(stmt) => self.update(&stmt.src),
            Statement::ExpressionStatement(stmt) => self.update(&stmt.src),
            Statement::PlaceholderStatement(stmt) => self.update(&stmt.src),
            Statement::Return(stmt) => self.update(&stmt.src),
            Statement::RevertStatement(stmt) => self.update(&stmt.src),
        }
    }
}

impl PrimativeStmtVisitor {
    fn update(&mut self, src: &SourceLocation) {
        if let Ok(src) = ValidSourceLocation::try_from(src) {
            self.0.entry(src.index).or_insert_with(BTreeMap::new).insert(src.start, src);
        }
    }

    fn visit_yul_statment(&mut self, stmt: &YulStatement) {
        // node_group! {
        //     YulStatement;

        //     YulAssignment,
        //     YulBlock,
        //     YulBreak,
        //     YulContinue,
        //     YulExpressionStatement,
        //     YulLeave,
        //     YulForLoop,
        //     YulFunctionDefinition,
        //     YulIf,
        //     YulSwitch,
        //     YulVariableDeclaration,
        // }
        match stmt {
            YulStatement::YulBlock(stmt) => {
                for yul_stmt in &stmt.statements {
                    self.visit_yul_statment(yul_stmt);
                }
            }
            YulStatement::YulForLoop(stmt) => {
                self.visit_yul_expression(&stmt.condition);
                for yul_stmt in stmt
                    .pre
                    .statements
                    .iter()
                    .chain(stmt.body.statements.iter())
                    .chain(&stmt.post.statements)
                {
                    self.visit_yul_statment(yul_stmt);
                }
            }
            YulStatement::YulFunctionDefinition(stmt) => {
                for yul_stmt in &stmt.body.statements {
                    self.visit_yul_statment(yul_stmt);
                }
            }
            YulStatement::YulIf(stmt) => {
                self.visit_yul_expression(&stmt.condition);
                for yul_stmt in &stmt.body.statements {
                    self.visit_yul_statment(yul_stmt);
                }
            }
            YulStatement::YulSwitch(stmt) => {
                self.visit_yul_expression(&stmt.expression);
                for case in &stmt.cases {
                    for yul_stmt in &case.body.statements {
                        self.visit_yul_statment(yul_stmt);
                    }
                }
            }
            YulStatement::YulVariableDeclaration(stmt) => self.update(&stmt.src),
            YulStatement::YulAssignment(stmt) => self.update(&stmt.src),
            YulStatement::YulBreak(stmt) => self.update(&stmt.src),
            YulStatement::YulContinue(stmt) => self.update(&stmt.src),
            YulStatement::YulExpressionStatement(stmt) => self.update(&stmt.src),
            YulStatement::YulLeave(stmt) => self.update(&stmt.src),
        }
    }

    fn visit_yul_expression(&mut self, expr: &YulExpression) {
        // node_group! {
        //     YulExpression;
        //
        //     YulFunctionCall,
        //     YulIdentifier,
        //     YulLiteral,
        // }
        match expr {
            YulExpression::YulFunctionCall(expr) => self.update(&expr.src),
            YulExpression::YulIdentifier(expr) => self.update(&expr.src),
            YulExpression::YulLiteral(expr) => self.update(&expr.src),
        }
    }

    /// Check whether there is any overlapping primitive statements.
    fn check_integrity(&self) -> Result<()> {
        for (_, stmts) in &self.0 {
            let mut last_end = 0;
            for (start, src) in stmts {
                if start < &last_end {
                    return Err(eyre!(format!("overlapping statements at {:?}", src)));
                }
                last_end = start + src.length;
            }
        }

        Ok(())
    }

    /// Produce the PrimativeStmts.
    fn produce(self) -> Result<PrimitiveStmts> {
        self.check_integrity()?;
        Ok(self.0)
    }
}

/// A more reliable source map analysis.
pub struct SourceMapAnalysis {}

impl SourceMapAnalysis {
    /// Analyze the source map of a compilation artifact.
    pub fn analyze(artifact: &CompilationArtifact) -> Result<PrimitiveStmts> {
        let mut visitor = PrimativeStmtVisitor::new();
        for (_, source) in artifact.sources.iter() {
            source.ast.walk(&mut visitor);
        }

        let units = visitor.produce()?;

        for (index, stmts) in &units {
            let source = artifact
                .sources
                .get(&(*index as u32))
                .ok_or(eyre!("missing source"))?
                .code
                .as_str();

            println!("{}", source_with_primative_statements(source, stmts));
        }

        Ok(units)
    }
}
