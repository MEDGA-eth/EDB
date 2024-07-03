use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use eyre::{eyre, Result};
use foundry_compilers::artifacts::{
    ast::SourceLocation,
    yul::{YulExpression, YulStatement},
    ExpressionOrVariableDeclarationStatement, InlineAssembly, Statement,
};
use solang_parser::{lexer, pt};

use crate::{analysis::ast_visitor::Visitor, utils::ast::get_source_location_for_expression};

#[derive(Clone, Debug)]
pub struct UnitLocation {
    pub start: usize,
    pub length: usize,
    pub index: usize,
}

impl TryFrom<&SourceLocation> for UnitLocation {
    type Error = eyre::Error;

    fn try_from(src: &SourceLocation) -> Result<Self, Self::Error> {
        let start = src.start.ok_or_else(|| eyre!("invalid source location"))? as usize;
        let length = src.length.ok_or_else(|| eyre!("invalid source location"))? as usize;
        let index = src.index.ok_or_else(|| eyre!("invalid source location"))? as usize;

        Ok(Self { start, length, index })
    }
}

trait AsSourceLocation {
    fn as_source_location(&self, l_off: usize, g_off: usize) -> Result<SourceLocation>;
}

impl AsSourceLocation for pt::Loc {
    fn as_source_location(&self, l_off: usize, g_off: usize) -> Result<SourceLocation> {
        match self {
            pt::Loc::File(file_index, start, end) => Ok(SourceLocation {
                index: Some(*file_index),
                start: Some(*start - l_off + g_off), // we need to adjust the offset
                length: Some(*end - *start),
            }),
            _ => Err(eyre!("invalid source location")),
        }
    }
}

/// A hyper unit is a collection of primitive units whose compiled opcodes are fused together.
#[derive(Clone, Debug)]
pub struct HyperUnit {
    pub id: u32,
    pub location: UnitLocation,
    pub code: Arc<String>,
    pub children: Vec<UnitLocation>,
}

pub type PrimitiveUnits = BTreeMap<usize, BTreeMap<usize, UnitLocation>>;

/// Visitor to collect all primative "statements", i.e., debugging unit.
///
/// A primative debugging unit is a statement that does not contain any other statements (e.g. a
/// block statement). A primative unit can also be the condition of a loop or if statement.
/// Primative debugging units are the basic stepping blocks for debugging.
/// This visitor will collect all primative statements and their locations.
#[derive(Clone, Debug)]
pub struct PrimativeUnitVisitor(pub PrimitiveUnits, pub BTreeMap<usize, Arc<String>>);

impl PrimativeUnitVisitor {
    pub fn new() -> Self {
        Self(BTreeMap::new(), BTreeMap::new())
    }

    pub fn register(&mut self, index: usize, code: Arc<String>) {
        self.1.insert(index, code);
    }
}

impl Deref for PrimativeUnitVisitor {
    type Target = BTreeMap<usize, BTreeMap<usize, UnitLocation>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for PrimativeUnitVisitor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Visitor for PrimativeUnitVisitor {
    fn visit_statement(&mut self, statement: &Statement) -> Result<()> {
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
                self.update(get_source_location_for_expression(&stmt.condition))?
            }
            // For do-whiles, the condition is a primative statement.
            Statement::DoWhileStatement(stmt) => {
                self.update(get_source_location_for_expression(&stmt.condition))?
            }
            // For while statements, the condition is also a primative statement.
            // Note that other part, e.g., body, will be visited by the visitor later.
            Statement::WhileStatement(stmt) => {
                self.update(get_source_location_for_expression(&stmt.condition))?
            }
            // For for statements, the condition, the initial expression, and the loop expression
            // are also primative statements. Note that other part, e.g., body, will be
            // visited by the visitor later.
            Statement::ForStatement(stmt) => {
                if let Some(cond) = &stmt.condition {
                    self.update(get_source_location_for_expression(cond))?;
                }
                if let Some(init) = &stmt.initialization_expression {
                    match init {
                        ExpressionOrVariableDeclarationStatement::ExpressionStatement(stmt) => {
                            self.update(&stmt.src)?
                        }
                        ExpressionOrVariableDeclarationStatement::VariableDeclarationStatement(
                            stmt,
                        ) => self.update(&stmt.src)?,
                    }
                }
                if let Some(loop_expr) = &stmt.loop_expression {
                    self.update(&loop_expr.src)?;
                }
            }
            // For try statement, we wil handle the external function call as a primative statement.
            // The catch and finally block will be visited by the visitor later.
            Statement::TryStatement(stmt) => {
                self.update(get_source_location_for_expression(&stmt.external_call.expression))?
            }
            // We will provide more fine-grained information for inline assembly if the Yul block is
            // presented.
            Statement::InlineAssembly(stmt) => {
                if stmt.ast.statements.is_empty() {
                    // If the Yul block is empty, it means the AST is from an older version of
                    // Solidity. In that case, the source location of the inline assembly block
                    // is quite inaccurate. We will need to adjust the source location to the
                    // whole inline assembly block.
                    self.visit_inline_assembly_old(stmt)?;
                } else {
                    for yul_stmt in &stmt.ast.statements {
                        self.visit_yul_statment(yul_stmt)?;
                    }
                }
            }
            Statement::VariableDeclarationStatement(stmt) => self.update(&stmt.src)?,
            Statement::Break(stmt) => self.update(&stmt.src)?,
            Statement::Continue(stmt) => self.update(&stmt.src)?,
            Statement::EmitStatement(stmt) => self.update(&stmt.src)?,
            Statement::ExpressionStatement(stmt) => self.update(&stmt.src)?,
            Statement::PlaceholderStatement(stmt) => self.update(&stmt.src)?,
            Statement::Return(stmt) => self.update(&stmt.src)?,
            Statement::RevertStatement(stmt) => self.update(&stmt.src)?,
        }

        Ok(())
    }
}

impl PrimativeUnitVisitor {
    fn update(&mut self, src: &SourceLocation) -> Result<()> {
        let src = UnitLocation::try_from(src)?;
        self.0.entry(src.index).or_insert_with(BTreeMap::new).insert(src.start, src);
        Ok(())
    }

    /// Check whether there is any overlapping primitive debugging unit.
    pub fn check_integrity(&self) -> Result<()> {
        for (_, stmts) in &self.0 {
            let mut last_end = 0;
            for (start, src) in stmts {
                if start < &last_end {
                    return Err(eyre!(format!("overlapping primitive units at {:?}", src)));
                }
                last_end = start + src.length;
            }
        }

        Ok(())
    }

    /// Produce the PrimativeStmts.
    pub fn produce(self) -> Result<PrimitiveUnits> {
        self.check_integrity()?;
        Ok(self.0)
    }

    /// Special handling for inline assembly for older versions of Solidity.
    fn visit_inline_assembly_old(&mut self, stmt: &InlineAssembly) -> Result<()> {
        let sloc: UnitLocation = (&stmt.src).try_into()?;
        let source = self.1.get(&sloc.index).ok_or(eyre!("missing source"))?.as_str();
        let mut asm_code = &source[sloc.start..sloc.start + sloc.length];

        // wrap the inline assembly code in a random function to parse it
        let mut wrapped_func = format!("function _medga_edb_150502() {{ {} }}", asm_code);

        // get the AST of the inline assembly
        let mut asm_ast = match solang_parser::parse(wrapped_func.as_str(), sloc.index) {
            Ok((source, _)) => source,
            Err(_) => {
                // For Solidity 0.4.x, its AST for assembly is bogus and will always include an
                // addtion identifier. Let's analyze its lexical structure to find
                // the correct source location.
                let mut comments = Vec::new();
                let mut lexer_errors = Vec::new();
                let lex = lexer::Lexer::new(asm_code, sloc.index, &mut comments, &mut lexer_errors);

                let (last_start, _, _) = lex.last().ok_or(eyre!("no token in the inline asm"))?;
                asm_code = &asm_code[..last_start];
                wrapped_func = format!("function _medga_edb_150502() {{ {} }}", asm_code);
                solang_parser::parse(wrapped_func.as_str(), sloc.index)
                    .map_err(|e| eyre!(format!("fail to parse inline assembly: {:?}", e)))?
                    .0
            }
        };

        // start to parse the AST
        if asm_ast.0.len() != 1 {
            return Err(eyre!(format!("invalid inline assembly AST: {}", asm_ast)));
        }
        let func = match asm_ast.0.remove(0) {
            pt::SourceUnitPart::FunctionDefinition(func) => func,
            _ => return Err(eyre!("invalid inline assembly AST when parsing function")),
        };
        let body = func.body.ok_or(eyre!("missing body"))?;
        let stmts = match body {
            pt::Statement::Block { statements, .. } => statements,
            _ => return Err(eyre!("invalid inline assembly AST when parsing function body")),
        };
        if stmts.len() != 1 {
            return Err(eyre!(
                "invalid inline assembly AST when parsing statments in function body"
            ));
        }
        let yul_block = match stmts[0] {
            pt::Statement::Assembly { block: ref yul_block, .. } => yul_block,
            _ => {
                return Err(eyre!(
                    "invalid inline assembly AST when parsing the first statment in the function"
                ))
            }
        };

        // parse each Yul statments
        let local_offset = wrapped_func.find(asm_code).expect("this should not happen");
        let global_offset = stmt.src.start.ok_or(eyre!("invalid source location"))? as usize;
        for yul_stmt in &yul_block.statements {
            self.visit_yul_statment_solang(yul_stmt, local_offset, global_offset)?;
        }

        Ok(())
    }

    /// Special handling for Yul statements generated by solang.
    fn visit_yul_statment_solang(
        &mut self,
        stmt: &pt::YulStatement,
        l_off: usize,
        g_off: usize,
    ) -> Result<()> {
        // pub enum YulStatement {
        //     /// `<1>,+ = <2>`
        //     Assign(Loc, Vec<YulExpression>, YulExpression),
        //     /// `let <1>,+ [:= <2>]`
        //     VariableDeclaration(Loc, Vec<YulTypedIdentifier>, Option<YulExpression>),
        //     /// `if <1> <2>`
        //     If(Loc, YulExpression, YulBlock),
        //     /// A [YulFor] statement.
        //     For(YulFor),
        //     /// A [YulSwitch] statement.
        //     Switch(YulSwitch),
        //     /// `leave`
        //     Leave(Loc),
        //     /// `break`
        //     Break(Loc),
        //     /// `continue`
        //     Continue(Loc),
        //     /// A [YulBlock] statement.
        //     Block(YulBlock),
        //     /// A [YulFunctionDefinition] statement.
        //     FunctionDefinition(Box<YulFunctionDefinition>),
        //     /// A [YulFunctionCall] statement.
        //     FunctionCall(Box<YulFunctionCall>),
        //     /// An error occurred during parsing.
        //     Error(Loc),
        // }
        match stmt {
            pt::YulStatement::Assign(loc, ..) |
            pt::YulStatement::VariableDeclaration(loc, ..) |
            pt::YulStatement::Leave(loc) |
            pt::YulStatement::Break(loc) |
            pt::YulStatement::Continue(loc) => {
                let loc = loc.as_source_location(l_off, g_off)?;
                self.update(&loc)?;
            }
            pt::YulStatement::FunctionCall(func) => {
                let loc = func.loc.as_source_location(l_off, g_off)?;
                self.update(&loc)?;
            }
            pt::YulStatement::If(_, expr, block) => {
                self.visit_yul_expression_solang(expr, l_off, g_off)?;
                for yul_stmt in &block.statements {
                    self.visit_yul_statment_solang(yul_stmt, l_off, g_off)?;
                }
            }
            pt::YulStatement::For(for_block) => {
                self.visit_yul_expression_solang(&for_block.condition, l_off, g_off)?;
                for yul_stmt in for_block
                    .init_block
                    .statements
                    .iter()
                    .chain(for_block.execution_block.statements.iter())
                    .chain(for_block.post_block.statements.iter())
                {
                    self.visit_yul_statment_solang(yul_stmt, l_off, g_off)?;
                }
            }
            pt::YulStatement::Switch(switch_block) => {
                self.visit_yul_expression_solang(&switch_block.condition, l_off, g_off)?;
                for case in &switch_block.cases {
                    match case {
                        pt::YulSwitchOptions::Default(_, block) |
                        pt::YulSwitchOptions::Case(.., block) => {
                            for yul_stmt in &block.statements {
                                self.visit_yul_statment_solang(yul_stmt, l_off, g_off)?;
                            }
                        }
                    }
                }
            }
            pt::YulStatement::Block(block) => {
                for yul_stmt in &block.statements {
                    self.visit_yul_statment_solang(yul_stmt, l_off, g_off)?;
                }
            }
            pt::YulStatement::FunctionDefinition(func) => {
                for yul_stmt in &func.body.statements {
                    self.visit_yul_statment_solang(yul_stmt, l_off, g_off)?;
                }
            }
            pt::YulStatement::Error(_) => {
                return Err(eyre!("error in Yul statement"));
            }
        }
        Ok(())
    }

    /// Special handling for Yul expressions generated by solang.
    fn visit_yul_expression_solang(
        &mut self,
        expr: &pt::YulExpression,
        l_off: usize,
        g_off: usize,
    ) -> Result<()> {
        // pub enum YulExpression {
        //     /// `<1> [: <2>]`
        //     BoolLiteral(Loc, bool, Option<Identifier>),
        //     /// `<1>[e<2>] [: <2>]`
        //     NumberLiteral(Loc, String, String, Option<Identifier>),
        //     /// `<1> [: <2>]`
        //     HexNumberLiteral(Loc, String, Option<Identifier>),
        //     /// `<0> [: <1>]`
        //     HexStringLiteral(HexLiteral, Option<Identifier>),
        //     /// `<0> [: <1>]`
        //     StringLiteral(StringLiteral, Option<Identifier>),
        //     /// Any valid [Identifier].
        //     Variable(Identifier),
        //     /// [YulFunctionCall].
        //     FunctionCall(Box<YulFunctionCall>),
        //     /// `<1>.<2>`
        //     SuffixAccess(Loc, Box<YulExpression>, Identifier),
        // }
        match expr {
            pt::YulExpression::BoolLiteral(loc, ..) |
            pt::YulExpression::NumberLiteral(loc, ..) |
            pt::YulExpression::HexNumberLiteral(loc, ..) |
            pt::YulExpression::HexStringLiteral(pt::HexLiteral { loc, .. }, ..) |
            pt::YulExpression::StringLiteral(pt::StringLiteral { loc, .. }, ..) |
            pt::YulExpression::Variable(pt::Identifier { loc, .. }) |
            pt::YulExpression::SuffixAccess(loc, ..) => {
                let loc = loc.as_source_location(l_off, g_off)?;
                self.update(&loc)
            }
            pt::YulExpression::FunctionCall(func) => {
                let loc = func.loc.as_source_location(l_off, g_off)?;
                self.update(&loc)
            }
        }
    }

    /// Special handling for Yul statements.
    fn visit_yul_statment(&mut self, stmt: &YulStatement) -> Result<()> {
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
                    self.visit_yul_statment(yul_stmt)?;
                }
            }
            YulStatement::YulForLoop(stmt) => {
                self.visit_yul_expression(&stmt.condition)?;
                for yul_stmt in stmt
                    .pre
                    .statements
                    .iter()
                    .chain(stmt.body.statements.iter())
                    .chain(&stmt.post.statements)
                {
                    self.visit_yul_statment(yul_stmt)?;
                }
            }
            YulStatement::YulFunctionDefinition(stmt) => {
                for yul_stmt in &stmt.body.statements {
                    self.visit_yul_statment(yul_stmt)?;
                }
            }
            YulStatement::YulIf(stmt) => {
                self.visit_yul_expression(&stmt.condition)?;
                for yul_stmt in &stmt.body.statements {
                    self.visit_yul_statment(yul_stmt)?;
                }
            }
            YulStatement::YulSwitch(stmt) => {
                self.visit_yul_expression(&stmt.expression)?;
                for case in &stmt.cases {
                    for yul_stmt in &case.body.statements {
                        self.visit_yul_statment(yul_stmt)?;
                    }
                }
            }
            YulStatement::YulVariableDeclaration(stmt) => self.update(&stmt.src)?,
            YulStatement::YulAssignment(stmt) => self.update(&stmt.src)?,
            YulStatement::YulBreak(stmt) => self.update(&stmt.src)?,
            YulStatement::YulContinue(stmt) => self.update(&stmt.src)?,
            YulStatement::YulExpressionStatement(stmt) => self.update(&stmt.src)?,
            YulStatement::YulLeave(stmt) => self.update(&stmt.src)?,
        }

        Ok(())
    }

    fn visit_yul_expression(&mut self, expr: &YulExpression) -> Result<()> {
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
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use alloy_chains::Chain;
    use alloy_primitives::Address;
    use edb_utils::cache::Cache;
    use serial_test::serial;

    use crate::{analysis::ast_visitor::Walk, artifact::deploy::DeployArtifact};

    use super::*;

    fn run_test(chain: Chain, addr: Address) -> Result<PrimitiveUnits> {
        // load cached artifacts
        let cache_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/cache/backend")
            .join(chain.to_string());
        let cache = Cache::new(cache_root, None)?;
        let artifact: DeployArtifact =
            cache.load_cache(addr.to_string()).ok_or(eyre!("missing artifact"))?;

        let mut visitor = PrimativeUnitVisitor::new();
        for (id, source) in artifact.sources.iter() {
            visitor.register(*id as usize, Arc::clone(&source.code));
            source.ast.walk(&mut visitor)?;
        }

        visitor.produce()
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_usd() {
        run_test(
            Chain::mainnet(),
            Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap(),
        )
        .unwrap();
    }
}
