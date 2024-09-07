use std::{
    collections::BTreeMap,
    fmt::{self, Debug},
    hash::{Hash, Hasher},
    ops::{Deref, DerefMut},
    sync::Arc,
};

use eyre::{bail, ensure, eyre, OptionExt, Result};
use foundry_compilers::artifacts::{
    ast::SourceLocation,
    yul::{YulExpression, YulStatement},
    ContractDefinition, ExpressionOrVariableDeclarationStatement, FunctionDefinition,
    InlineAssembly, ModifierDefinition, StateMutability, Statement,
};
use solang_parser::{helpers::CodeLocation, lexer, pt};

use crate::{
    analysis::ast_visitor::{Visitor, Walk},
    artifact::deploy::DeployArtifact,
    utils::ast::get_source_location_for_expression,
};

use super::AnalysisStore;

trait AsSourceLocation {
    fn as_source_location(&self, l_off: usize, g_off: usize) -> Result<SourceLocation>;
}

impl AsSourceLocation for pt::Loc {
    fn as_source_location(&self, l_off: usize, g_off: usize) -> Result<SourceLocation> {
        match self {
            Self::File(file_index, start, end) => Ok(SourceLocation {
                index: Some(*file_index),
                start: Some(*start - l_off + g_off), // we need to adjust the offset
                length: Some(*end - *start),
            }),
            _ => Err(eyre!("invalid source location")),
        }
    }
}

/// A more easy-to-use unit location, which includes the corresponding source code.
#[derive(Clone, Debug)]
pub struct UnitLocation {
    pub start: usize,
    pub length: usize,
    pub index: usize,
    pub code: Arc<String>,
}

impl UnitLocation {
    pub fn contains(&self, start: usize, length: usize) -> bool {
        self.start <= start && self.start + self.length >= start + length
    }

    pub fn matches(&self, start: usize, length: usize) -> bool {
        self.start == start && self.length == length
    }
}

impl Hash for UnitLocation {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.start.hash(state);
        self.length.hash(state);
        self.index.hash(state);
    }
}

impl PartialEq for UnitLocation {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start && self.length == other.length && self.index == other.index
    }
}

impl Eq for UnitLocation {}

impl PartialOrd for UnitLocation {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UnitLocation {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.index != other.index {
            self.index.cmp(&other.index)
        } else if self.start != other.start {
            self.start.cmp(&other.start)
        } else {
            self.length.cmp(&other.length)
        }
    }
}

impl fmt::Display for UnitLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.length < 50 {
            let source = &self.code.as_str()[self.start..self.start + self.length];
            write!(f, "{}", source.escape_debug())
        } else {
            let source = &self.code.as_str()[self.start..self.start + 50];
            write!(f, "{}...", source.escape_debug())
        }
    }
}

impl TryFrom<&SourceLocation> for UnitLocation {
    type Error = eyre::Error;

    fn try_from(src: &SourceLocation) -> Result<Self, Self::Error> {
        let start = src.start.ok_or_else(|| eyre!("invalid source location"))?;
        let length = src.length.ok_or_else(|| eyre!("invalid source location"))?;
        let index = src.index.ok_or_else(|| eyre!("invalid source location"))?;

        Ok(Self { start, length, index, code: Arc::default() })
    }
}

/// Metadata for Function Unit.
#[derive(Clone, Debug, PartialEq, Ord, Eq, PartialOrd, Hash, Copy)]
pub struct FunctionMeta {
    pub is_modifier: bool,
    pub is_pure: bool,
}

impl FunctionMeta {
    pub fn new(is_modifier: bool, is_pure: bool) -> Self {
        Self { is_modifier, is_pure }
    }
}

impl From<&FunctionDefinition> for FunctionMeta {
    fn from(func: &FunctionDefinition) -> Self {
        Self::new(false, func.state_mutability == Some(StateMutability::Pure))
    }
}

impl From<&ModifierDefinition> for FunctionMeta {
    fn from(_modifier: &ModifierDefinition) -> Self {
        Self::new(true, false)
    }
}

/// Different kind of debugging units.
/// A debugging unit can be either an execution unit (singleton primitive or block-level inline
/// assembly) or a non-execution unit (function or contract). The execution units are the basic
/// stepping blocks for debugging. The non-execution units are tags for function and contract
/// definitions.
#[derive(Clone, Debug, PartialEq, Ord, Eq, PartialOrd, Hash)]
pub enum DebugUnit {
    /// A primitive unit is a single statement or expression (execution unit)
    Primitive(UnitLocation),

    /// Inline assembly block
    InlineAssembly(UnitLocation, Vec<UnitLocation>),

    /// A function unit is a tag for a function definition (non-execution unit).
    Function(UnitLocation, FunctionMeta),

    /// A contract unit is a tag for a contract definition (non-execution unit).
    Contract(UnitLocation),
}

impl Deref for DebugUnit {
    type Target = UnitLocation;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Primitive(loc) |
            Self::InlineAssembly(loc, _) |
            Self::Function(loc, ..) |
            Self::Contract(loc) => loc,
        }
    }
}

impl DerefMut for DebugUnit {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Primitive(loc) |
            Self::InlineAssembly(loc, _) |
            Self::Function(loc, ..) |
            Self::Contract(loc) => loc,
        }
    }
}

impl DebugUnit {
    pub fn loc(&self) -> &UnitLocation {
        match self {
            Self::Primitive(loc) |
            Self::InlineAssembly(loc, _) |
            Self::Function(loc, ..) |
            Self::Contract(loc) => loc,
        }
    }

    pub fn loc_mut(&mut self) -> &mut UnitLocation {
        match self {
            Self::Primitive(loc) |
            Self::InlineAssembly(loc, _) |
            Self::Function(loc, ..) |
            Self::Contract(loc) => loc,
        }
    }

    pub fn get_asm_stmts(&self) -> Option<&Vec<UnitLocation>> {
        match self {
            Self::InlineAssembly(_, stmts) => Some(stmts),
            _ => None,
        }
    }

    pub fn get_function_meta(&self) -> Option<&FunctionMeta> {
        match self {
            Self::Function(_, meta) => Some(meta),
            _ => None,
        }
    }

    pub fn is_execution_unit(&self) -> bool {
        match self {
            Self::Primitive(_) | Self::InlineAssembly(_, _) => true,
            Self::Function(..) | Self::Contract(_) => false,
        }
    }

    pub fn iter(&self) -> DebugUnitIterator<'_> {
        match self {
            Self::Primitive(loc) | Self::Function(loc, ..) | Self::Contract(loc) => {
                DebugUnitIterator { unit: vec![loc], index: 0 }
            }
            Self::InlineAssembly(_, stmts) => {
                DebugUnitIterator { unit: stmts.iter().collect(), index: 0 }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct DebugUnitIterator<'a> {
    unit: Vec<&'a UnitLocation>,
    index: usize,
}

impl<'a> Iterator for DebugUnitIterator<'a> {
    type Item = &'a UnitLocation;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.unit.len() {
            let result = self.unit[self.index];
            self.index += 1;
            Some(result)
        } else {
            None
        }
    }
}

impl fmt::Display for DebugUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.loc())
    }
}

/// Debugging units is a mapping from the source index to a mapping from the start position to the
/// corresponding debugging unit.
///
/// Note that at each start position, there is only one debugging unit (contract, function,
/// primitive, and hyper).
#[derive(Clone, Debug, Default)]
pub struct DebugUnits {
    units: BTreeMap<usize, BTreeMap<usize, DebugUnit>>,
    functions: BTreeMap<DebugUnit, DebugUnit>,
    contracts: BTreeMap<DebugUnit, DebugUnit>,

    // The position of each statement/assembly block within its corresponding function.
    // The key is the debugging unit of the statement/assembly block, and the value is its
    // position within the function.
    positions: BTreeMap<DebugUnit, usize>,
}

impl DebugUnits {
    pub fn units_per_file(&self, index: usize) -> Option<&BTreeMap<usize, DebugUnit>> {
        self.units.get(&index)
    }

    pub fn function(&self, unit: &DebugUnit) -> Option<&DebugUnit> {
        self.functions.get(unit)
    }

    pub fn contract(&self, unit: &DebugUnit) -> Option<&DebugUnit> {
        self.contracts.get(unit)
    }

    pub fn position(&self, unit: &DebugUnit) -> Option<usize> {
        self.positions.get(unit).copied()
    }

    pub fn iter(&self) -> DebugUnitsIterator<'_> {
        DebugUnitsIterator::new(self)
    }
}

impl Deref for DebugUnits {
    type Target = BTreeMap<usize, BTreeMap<usize, DebugUnit>>;

    fn deref(&self) -> &Self::Target {
        &self.units
    }
}

impl DerefMut for DebugUnits {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.units
    }
}

#[derive(Debug, Default)]
pub struct DebugUnitsIterator<'a> {
    units: Vec<&'a DebugUnit>,
    index: usize,
}

impl<'a> DebugUnitsIterator<'a> {
    pub fn new(units: &'a DebugUnits) -> Self {
        let mut units: Vec<_> = units.units.values().flat_map(|m| m.values()).collect();
        units.sort();
        Self { units, index: 0 }
    }
}

impl<'a> Iterator for DebugUnitsIterator<'a> {
    type Item = &'a DebugUnit;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.units.len() {
            let result = self.units[self.index];
            self.index += 1;
            Some(result)
        } else {
            None
        }
    }
}

/// Visitor to collect all primative "statements", i.e., debugging unit.
///
/// A primative debugging unit is a statement that does not contain any other statements (e.g. a
/// block statement). A primative unit can also be the condition of a loop or if statement.
/// Primative debugging units are the basic stepping blocks for debugging.
/// This visitor will collect all primative statements and their locations, as well as other
/// non-execution units (e.g., function and contract definitions).
///
/// Note that, at this stage, no hyper units are collected.
#[derive(Clone, Debug, Default)]
pub struct DebugUnitVisitor {
    units: BTreeMap<usize, BTreeMap<usize, DebugUnit>>,
    sources: BTreeMap<usize, Arc<String>>,

    last_inline_assembly: Option<DebugUnit>,

    functions: BTreeMap<DebugUnit, DebugUnit>,
    last_function: Option<DebugUnit>,

    contracts: BTreeMap<DebugUnit, DebugUnit>,
    last_contract: Option<DebugUnit>,
}

impl DebugUnitVisitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, index: usize, code: Arc<String>) {
        self.sources.insert(index, code);
    }
}

impl Visitor for DebugUnitVisitor {
    fn visit_contract_definition(&mut self, definition: &ContractDefinition) -> Result<()> {
        self.update_contract(&definition.src)
    }

    fn visit_function_definition(&mut self, definition: &FunctionDefinition) -> Result<()> {
        self.update_function(&definition.src, definition.into())
    }

    fn visit_modifier_definition(&mut self, definition: &ModifierDefinition) -> Result<()> {
        self.update_function(&definition.src, definition.into())
    }

    fn post_visit_statement(&mut self, statement: &Statement) -> Result<()> {
        if matches!(statement, Statement::InlineAssembly(_)) {
            self.update_inline_assembly()
        } else {
            Ok(())
        }
    }

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
                self.update_primitive(get_source_location_for_expression(&stmt.condition))?
            }
            // For do-whiles, the condition is a primative statement.
            Statement::DoWhileStatement(stmt) => {
                self.update_primitive(get_source_location_for_expression(&stmt.condition))?
            }
            // For while statements, the condition is also a primative statement.
            // Note that other part, e.g., body, will be visited by the visitor later.
            Statement::WhileStatement(stmt) => {
                self.update_primitive(get_source_location_for_expression(&stmt.condition))?
            }
            // For for statements, the condition, the initial expression, and the loop expression
            // are also primative statements. Note that other part, e.g., body, will be
            // visited by the visitor later.
            Statement::ForStatement(stmt) => {
                if let Some(cond) = &stmt.condition {
                    self.update_primitive(get_source_location_for_expression(cond))?;
                }
                if let Some(init) = &stmt.initialization_expression {
                    match init {
                        ExpressionOrVariableDeclarationStatement::ExpressionStatement(stmt) => {
                            self.update_primitive(&stmt.src)?
                        }
                        ExpressionOrVariableDeclarationStatement::VariableDeclarationStatement(
                            stmt,
                        ) => self.update_primitive(&stmt.src)?,
                    }
                }
                if let Some(loop_expr) = &stmt.loop_expression {
                    self.update_primitive(&loop_expr.src)?;
                }
            }
            // For try statement, we wil handle the external function call as a primative statement.
            // The catch and finally block will be visited by the visitor later.
            Statement::TryStatement(stmt) => self.update_primitive(
                get_source_location_for_expression(&stmt.external_call.expression),
            )?,
            // We will provide more fine-grained information for inline assembly if the Yul block is
            // presented.
            Statement::InlineAssembly(stmt) => {
                if let Some(yul_block) = stmt.ast.as_ref() {
                    if yul_block.statements.is_empty() {
                        // If the Yul block is empty, it is possible that the AST is from an older
                        // version of Solidity. In that case, the source
                        // location of the inline assembly block
                        // is quite inaccurate. We will need to adjust the source location to the
                        // whole inline assembly block.
                        self.visit_inline_assembly_old(stmt)?;
                    } else {
                        ensure!(
                            self.last_inline_assembly.is_none(),
                            "nested inline assembly block"
                        );
                        self.last_inline_assembly = Some(DebugUnit::InlineAssembly(
                            self.get_unit_location(&stmt.src)?,
                            Vec::new(),
                        ));
                        for yul_stmt in &yul_block.statements {
                            self.visit_yul_statment(yul_stmt)?;
                        }
                    }
                } else {
                    // If the Yul block is not presented, it is also possible that the AST is from
                    // an older version of Solidity. In that case, we also need
                    // to adjust the source location to the whole inline
                    // assembly block.
                    self.visit_inline_assembly_old(stmt)?;
                }
            }
            Statement::VariableDeclarationStatement(stmt) => self.update_primitive(&stmt.src)?,
            Statement::Break(stmt) => self.update_primitive(&stmt.src)?,
            Statement::Continue(stmt) => self.update_primitive(&stmt.src)?,
            Statement::EmitStatement(stmt) => self.update_primitive(&stmt.src)?,
            Statement::ExpressionStatement(stmt) => self.update_primitive(&stmt.src)?,
            Statement::PlaceholderStatement(stmt) => self.update_primitive(&stmt.src)?,
            Statement::Return(stmt) => self.update_primitive(&stmt.src)?,
            Statement::RevertStatement(stmt) => self.update_primitive(&stmt.src)?,
        }

        Ok(())
    }
}

impl DebugUnitVisitor {
    #[inline]
    fn get_unit_location(&self, src: &SourceLocation) -> Result<UnitLocation> {
        let mut src = UnitLocation::try_from(src)?;
        src.code = Arc::clone(self.sources.get(&src.index).ok_or_eyre("missing source")?);
        Ok(src)
    }

    #[inline]
    fn insert_debug_unit(&mut self, unit: DebugUnit) -> Result<()> {
        self.units
            .entry(unit.index)
            .or_default()
            .insert(unit.start, unit)
            .map_or(Ok(()), |_| Err(eyre!("overlapping contract units")))
    }

    #[inline]
    fn insert_execution_unit(&mut self, unit: DebugUnit) -> Result<()> {
        debug_assert!(unit.is_execution_unit());

        let function = self.last_function.as_ref().ok_or_eyre("statement outside of function")?;
        self.functions.insert(unit.clone(), function.clone());

        let contract = self.last_contract.as_ref().ok_or_eyre("statement outside of contract")?;
        self.contracts.insert(unit.clone(), contract.clone());

        self.insert_debug_unit(unit)
    }

    fn update_inline_assembly(&mut self) -> Result<()> {
        ensure!(self.last_inline_assembly.is_some(), "we are not in inline assembly block");
        let mut asm_unit =
            self.last_inline_assembly.take().ok_or_eyre("no inline assembly found")?;

        // Sort the Yul statements by their start position.
        let DebugUnit::InlineAssembly(_, stmt) = &mut asm_unit else {
            bail!("invalid inline assembly unit");
        };
        stmt.sort();

        trace!("wrap up an inline assembly block: {}", asm_unit.loc());
        self.insert_execution_unit(asm_unit)
    }

    fn update_primitive(&mut self, src: &SourceLocation) -> Result<()> {
        ensure!(self.last_inline_assembly.is_none(), "we are in inline assembly block");

        let src = self.get_unit_location(src)?;
        trace!("find a primative debug unit: {}", src);
        self.insert_execution_unit(DebugUnit::Primitive(src))
    }

    fn update_yul_primitive(&mut self, src: &SourceLocation) -> Result<()> {
        let Some(DebugUnit::InlineAssembly(loc, stmt)) = self.last_inline_assembly.as_mut() else {
            bail!("no inline assembly found")
        };

        let mut src = UnitLocation::try_from(src)?;
        src.code = Arc::clone(self.sources.get(&src.index).ok_or_eyre("missing source")?);
        ensure!(loc.contains(src.start, src.length), "invalid Yul source location");

        stmt.push(src);

        Ok(())
    }

    fn update_function(&mut self, src: &SourceLocation, meta: FunctionMeta) -> Result<()> {
        let src = self.get_unit_location(src)?;
        trace!("find a function unit: {}", src);

        self.last_function = Some(DebugUnit::Function(src.clone(), meta));

        self.insert_debug_unit(DebugUnit::Function(src, meta))
    }

    fn update_contract(&mut self, src: &SourceLocation) -> Result<()> {
        let src = self.get_unit_location(src)?;
        trace!("find a contract unit: {}", src);

        self.last_contract = Some(DebugUnit::Contract(src.clone()));

        self.insert_debug_unit(DebugUnit::Contract(src))
    }

    /// Check whether there is any overlapping primitive debugging unit.
    pub fn check_integrity(&self) -> Result<()> {
        for stmts in self.units.values() {
            let stmts = stmts.values();

            // Check whether there is any overlapping execution debugging unit.
            do_integrity_checking(
                stmts.clone().filter(|u| u.is_execution_unit()).map(|u| u.loc()),
            )?;

            // Check whether there is any overlapping non-execution debugging unit.
            do_integrity_checking(
                stmts.clone().filter(|u| matches!(u, DebugUnit::Function(..))).map(|u| u.loc()),
            )?;
            do_integrity_checking(
                stmts.clone().filter(|u| matches!(u, DebugUnit::Contract(..))).map(|u| u.loc()),
            )?;

            // Check inline-assembly block.
            for asm_stmts in stmts.filter_map(|u| u.get_asm_stmts()) {
                do_integrity_checking(asm_stmts.iter())?;
            }
        }

        Ok(())
    }

    /// Produce the PrimativeStmts.
    pub fn produce(self) -> Result<DebugUnits> {
        self.check_integrity()?;
        let positions = self.calculate_positions()?;
        Ok(DebugUnits {
            units: self.units,
            functions: self.functions,
            contracts: self.contracts,
            positions,
        })
    }

    /// Calculate the position of each statement/assembly block within its corresponding function.
    fn calculate_positions(&self) -> Result<BTreeMap<DebugUnit, usize>> {
        // Group the statements by their corresponding function.
        let mut function_to_units = BTreeMap::new();
        for (unit, function) in &self.functions {
            debug_assert!(unit.is_execution_unit());
            function_to_units.entry(function).or_insert_with(Vec::new).push(unit);
        }

        // Source the statements by their start position.
        for units in function_to_units.values_mut() {
            units.sort_by_key(|unit| unit.start);
        }

        // Calculate the position of each statement within its corresponding function.
        let mut positions = BTreeMap::new();
        for units in function_to_units.into_values() {
            for (index, unit) in units.into_iter().enumerate() {
                positions.insert(unit.clone(), index);
            }
        }

        Ok(positions)
    }

    /// Special handling for inline assembly for older versions of Solidity.
    fn visit_inline_assembly_old(&mut self, stmt: &InlineAssembly) -> Result<()> {
        let sloc: UnitLocation = (&stmt.src).try_into()?;
        let source = self.sources.get(&sloc.index).ok_or_eyre("missing source")?.as_str();
        let mut asm_code = &source[sloc.start..sloc.start + sloc.length];

        // wrap the inline assembly code in a random function to parse it
        let mut wrapped_func = format!("function _medga_edb_150502() {{ {asm_code} }}");

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

                let (last_start, _, _) = lex.last().ok_or_eyre("no token in the inline asm")?;
                asm_code = &asm_code[..last_start];
                wrapped_func = format!("function _medga_edb_150502() {{ {asm_code} }}");
                solang_parser::parse(wrapped_func.as_str(), sloc.index)
                    .map_err(|e| eyre!(format!("fail to parse inline assembly: {:?}", e)))?
                    .0
            }
        };

        // start to parse the AST
        if asm_ast.0.len() != 1 {
            bail!(format!("invalid inline assembly AST: {}", asm_ast));
        }
        let pt::SourceUnitPart::FunctionDefinition(func) = asm_ast.0.remove(0) else {
            bail!("invalid inline assembly AST when parsing function");
        };
        let body = func.body.ok_or_eyre("missing body")?;
        let pt::Statement::Block { statements: stmts, .. } = body else {
            bail!("invalid inline assembly AST when parsing function body");
        };
        if stmts.len() != 1 {
            bail!("invalid inline assembly AST when parsing statments in function body");
        }
        let pt::Statement::Assembly { block: ref yul_block, .. } = stmts[0] else {
            bail!("invalid inline assembly AST when parsing the first statment in the function");
        };

        // Prepare the InlineAssembly unit. Note that we need to adjust the length.
        let mut asm_loc = self.get_unit_location(&stmt.src)?;
        asm_loc.length = stmts[0].loc().range().len();
        ensure!(self.last_inline_assembly.is_none(), "nested inline assembly block");
        self.last_inline_assembly = Some(DebugUnit::InlineAssembly(asm_loc, Vec::new()));

        // Parse each Yul statments.
        let local_offset = wrapped_func.find(asm_code).expect("this should not happen");
        let global_offset = stmt.src.start.ok_or_eyre("invalid source location")?;
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
                self.update_yul_primitive(&loc)?;
            }
            pt::YulStatement::FunctionCall(func) => {
                let loc = func.loc.as_source_location(l_off, g_off)?;
                self.update_yul_primitive(&loc)?;
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
                trace!(switch_block=?switch_block, "switch block in pt-yul");
                for case in &switch_block.cases {
                    match case {
                        pt::YulSwitchOptions::Case(.., block) => {
                            for yul_stmt in &block.statements {
                                self.visit_yul_statment_solang(yul_stmt, l_off, g_off)?;
                            }
                        }
                        _ => bail!("invalid case in Yul switch"),
                    }
                }
                if let Some(default_case) = &switch_block.default {
                    match default_case {
                        pt::YulSwitchOptions::Default(_, block) => {
                            for yul_stmt in &block.statements {
                                self.visit_yul_statment_solang(yul_stmt, l_off, g_off)?;
                            }
                        }
                        _ => bail!("invalid default case in Yul switch"),
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
                bail!("error in Yul statement");
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
                self.update_yul_primitive(&loc)
            }
            pt::YulExpression::FunctionCall(func) => {
                let loc = func.loc.as_source_location(l_off, g_off)?;
                self.update_yul_primitive(&loc)
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
            YulStatement::YulVariableDeclaration(stmt) => self.update_yul_primitive(&stmt.src)?,
            YulStatement::YulAssignment(stmt) => self.update_yul_primitive(&stmt.src)?,
            YulStatement::YulBreak(stmt) => self.update_yul_primitive(&stmt.src)?,
            YulStatement::YulContinue(stmt) => self.update_yul_primitive(&stmt.src)?,
            YulStatement::YulExpressionStatement(stmt) => self.update_yul_primitive(&stmt.src)?,
            YulStatement::YulLeave(stmt) => self.update_yul_primitive(&stmt.src)?,
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
            YulExpression::YulFunctionCall(expr) => self.update_yul_primitive(&expr.src),
            YulExpression::YulIdentifier(expr) => self.update_yul_primitive(&expr.src),
            YulExpression::YulLiteral(expr) => self.update_yul_primitive(&expr.src),
        }
    }
}

pub struct DebugUnitAnlaysis {}

impl DebugUnitAnlaysis {
    pub fn analyze(artifact: &DeployArtifact, store: &mut AnalysisStore<'_>) -> Result<()> {
        let mut visitor = DebugUnitVisitor::new();
        for (id, source) in artifact.sources.iter() {
            visitor.register(*id as usize, Arc::clone(&source.code));
            source.ast.walk(&mut visitor)?;
        }

        let units = visitor.produce()?;

        #[cfg(debug_assertions)]
        for (index, stmts) in &units.units {
            let source =
                artifact.sources.get(&(*index as u32)).ok_or_eyre("missing source")?.code.as_str();

            trace!("{}", crate::utils::ast::source_with_debug_units(source, stmts));
        }

        store.debug_units = Some(units);
        Ok(())
    }
}

#[inline]
fn do_integrity_checking<'a, T>(units: T) -> Result<()>
where
    T: Iterator<Item = &'a UnitLocation>,
{
    let mut last_end = 0;
    for unit in units {
        if unit.start < last_end {
            bail!(format!("overlapping primitive units at {:?}", unit));
        }
        last_end = unit.start + unit.length;
    }

    Ok(())
}
