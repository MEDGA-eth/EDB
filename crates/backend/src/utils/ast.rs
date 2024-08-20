use std::collections::BTreeMap;

use foundry_compilers::artifacts::{ast::SourceLocation, Expression};

#[inline]
pub fn get_source_location_for_expression(expr: &Expression) -> &SourceLocation {
    // node_group! {
    //     Expression;
    //
    //     Assignment,
    //     BinaryOperation,
    //     Conditional,
    //     ElementaryTypeNameExpression,
    //     FunctionCall,
    //     FunctionCallOptions,
    //     Identifier,
    //     IndexAccess,
    //     IndexRangeAccess,
    //     Literal,
    //     MemberAccess,
    //     NewExpression,
    //     TupleExpression,
    //     UnaryOperation,
    // }
    match expr {
        Expression::Assignment(assignment) => &assignment.src,
        Expression::BinaryOperation(binary) => &binary.src,
        Expression::Conditional(conditional) => &conditional.src,
        Expression::ElementaryTypeNameExpression(type_expr) => &type_expr.src,
        Expression::FunctionCall(func_call) => &func_call.src,
        Expression::FunctionCallOptions(func_call_opts) => &func_call_opts.src,
        Expression::Identifier(ident) => &ident.src,
        Expression::IndexAccess(index_access) => &index_access.src,
        Expression::IndexRangeAccess(index_range) => &index_range.src,
        Expression::Literal(literal) => &literal.src,
        Expression::MemberAccess(member_access) => &member_access.src,
        Expression::NewExpression(new_expr) => &new_expr.src,
        Expression::TupleExpression(tuple_expr) => &tuple_expr.src,
        Expression::UnaryOperation(unary) => &unary.src,
    }
}

#[cfg(debug_assertions)]
/// Print the given source code with highlighted debug units.
pub fn source_with_debug_units(
    source: &str,
    units: &BTreeMap<usize, crate::analysis::source_map::debug_unit::DebugUnit>,
) -> String {
    let colors = ["\x1b[31m", "\x1b[33m", "\x1b[34m", "\x1b[32m"]; // Red, Yellow, Blue, Green
    let reset = "\x1b[0m"; // Reset color

    let mut result = String::new();
    let mut current_index = 0;

    // It is likely that the stmts have the same size as the units, so we preallocate the vector
    // with the same size.
    let mut stmts = Vec::with_capacity(units.len());
    for (_, unit) in units.iter().filter(|(_, unit)| unit.is_execution_unit()) {
        stmts.extend(unit.iter())
    }
    stmts.sort();

    for (i, stmt) in stmts.iter().enumerate() {
        let offset = stmt.start;
        let length = stmt.length;
        // Append the text before the segment
        if current_index < offset {
            result.push_str(&source[current_index..offset]);
        }

        // Select the color in a rotating fashion
        let color = colors[i % colors.len()];

        // Append the highlighted segment using ANSI escape codes for yellow
        let segment = &source[offset..offset + length];
        result.push_str(&format!("{color}{segment}{reset}"));

        current_index = offset + length;
    }

    // Append the remaining text after the last segment
    if current_index < source.len() {
        result.push_str(&source[current_index..]);
    }

    result
}
