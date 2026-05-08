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

//! Parsing utilities for expression evaluation.
//!
//! This module provides utilities for parsing Solidity-like expressions into
//! abstract syntax trees that can be evaluated by the expression evaluator.

use eyre::{Result, bail};
use solang_parser::{
    parse,
    pt::{Expression, SourceUnit, SourceUnitPart, Statement},
};

/// Parse a string input into a Solidity expression AST.
///
/// This function wraps the input string in a dummy function definition to
/// leverage the Solang parser, then extracts the expression from the function body.
///
/// # Arguments
/// * `input` - The expression string to parse (e.g., "a + b", "balanceOf(user)")
///
/// # Returns
/// A parsed [`Expression`] AST node that can be evaluated
///
/// # Errors
/// Returns an error if the input cannot be parsed as a valid Solidity expression
///
/// # Examples
/// ```rust,ignore
/// let expr = parse_input("msg.sender == owner")?;
/// let expr = parse_input("balances[user] > 1000")?;
/// let expr = parse_input("totalSupply() * price / 1e18")?;
/// ```
pub fn parse_input(input: &str) -> Result<Expression> {
    let trimmed = input.trim();
    let wrapped_input = if trimmed.ends_with(";") {
        format!("function __edb_sol_repl_() public {{ {trimmed} }}")
    } else {
        format!("function __edb_sol_repl_() public {{ {trimmed}; }}")
    };

    let (SourceUnit(parts), _comments) = parse(&wrapped_input, 0).map_err(|e| {
        eyre::eyre!("{}", e.last().map(|d| d.message.as_str()).unwrap_or("parsing error"))
    })?;

    if parts.len() != 1 {
        bail!("Expected a single function definition");
    }
    let statements = match parts[0] {
        SourceUnitPart::FunctionDefinition(ref def) => {
            if let Some(Statement::Block { ref statements, .. }) = def.body {
                statements.clone()
            } else {
                bail!("Function body is not a block");
            }
        }
        _ => bail!("Expected a function definition"),
    };

    if statements.len() != 1 {
        bail!("Expected a single statement in function body");
    }
    if let Statement::Expression(_, ref expr) = statements[0] {
        Ok(expr.clone())
    } else {
        bail!("Expected an expression statement")
    }
}

#[cfg(test)]
mod tests {
    use solang_parser::pt::Identifier;

    use super::*;

    #[test]
    fn test_parse_input() {
        let expr = parse_input("a + b").unwrap();
        if let Expression::Add(_, left, right) = expr {
            if let (
                Expression::Variable(Identifier { name: n1, .. }),
                Expression::Variable(Identifier { name: n2, .. }),
            ) = (*left, *right)
            {
                assert_eq!(n1, "a");
                assert_eq!(n2, "b");
            } else {
                panic!("Add operands are not variables");
            }
        } else {
            panic!("Parsed expression is not an addition");
        }

        let expr = parse_input("x * (y - z);").unwrap();
        if let Expression::Multiply(_, left, right) = expr {
            if let Expression::Variable(Identifier { name: n1, .. }) = *left {
                // The parenthesis might result in a Parenthesis expression wrapping the subtraction
                match *right {
                    Expression::Subtract(_, sub_left, sub_right) => {
                        if let (
                            Expression::Variable(Identifier { name: n2, .. }),
                            Expression::Variable(Identifier { name: n3, .. }),
                        ) = (*sub_left, *sub_right)
                        {
                            assert_eq!(n1, "x");
                            assert_eq!(n2, "y");
                            assert_eq!(n3, "z");
                        } else {
                            panic!("Subtraction operands are not variables");
                        }
                    }
                    Expression::Parenthesis(_, inner) => {
                        if let Expression::Subtract(_, sub_left, sub_right) = *inner {
                            if let (
                                Expression::Variable(Identifier { name: n2, .. }),
                                Expression::Variable(Identifier { name: n3, .. }),
                            ) = (*sub_left, *sub_right)
                            {
                                assert_eq!(n1, "x");
                                assert_eq!(n2, "y");
                                assert_eq!(n3, "z");
                            } else {
                                panic!("Subtraction operands are not variables");
                            }
                        } else {
                            panic!("Parenthesis does not contain a subtraction");
                        }
                    }
                    _ => {
                        panic!("Right operand is neither subtraction nor parenthesis: {right:?}")
                    }
                }
            } else {
                panic!("Left operand is not a variable");
            }
        } else {
            panic!("Parsed expression is not a multiplication");
        }
    }
}
