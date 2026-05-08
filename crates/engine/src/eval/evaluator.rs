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

use std::sync::Arc;

use alloy_dyn_abi::DynSolValue;
use alloy_primitives::{Address, B256, I256, U256};
use eyre::{Result, bail};
use revm::{Database, DatabaseCommit, DatabaseRef, database::CacheDB};
use solang_parser::pt::{Expression, Identifier, Loc, Parameter, Type};

use crate::{
    EngineContext,
    eval::handlers::{debug::create_debug_handlers, edb::EdbHandler},
};

use super::{handlers::EvaluatorHandlers, utils::parse_input};

/// Main expression evaluator for Solidity-like expressions.
///
/// The `ExpressionEvaluator` parses and evaluates Solidity-like expressions against
/// configurable data sources through the handler pattern. It supports a wide range
/// of expression types including variables, function calls, arithmetic operations,
/// type casting, and blockchain context access.
///
/// # Architecture
///
/// The evaluator uses a handler-based architecture where different aspects of
/// evaluation are delegated to specialized handlers:
/// - Variable resolution
/// - Function calls
/// - Mapping/array access
/// - Member access
/// - Blockchain context (`msg`, `tx`, `block`)
/// - Value validation
///
/// # Supported Operations
///
/// - **Variables**: Local variables, state variables, special variables (`this`)
/// - **Literals**: Numbers (decimal/hex), strings, booleans, addresses
/// - **Arithmetic**: `+`, `-`, `*`, `/`, `%`, `**`
/// - **Comparison**: `==`, `!=`, `<`, `<=`, `>`, `>=`
/// - **Logical**: `&&`, `||`, `!`
/// - **Bitwise**: `&`, `|`, `^`, `~`, `<<`, `>>`
/// - **Indexing**: Arrays and mappings with bracket notation
/// - **Member Access**: Dot notation for structs and contract members
/// - **Function Calls**: Contract functions and built-in functions
/// - **Type Casting**: Explicit type conversions (e.g., `uint256(value)`)
/// - **Ternary**: Conditional operator `? :`
///
/// # Example
///
/// ```rust,ignore
/// use edb_engine::eval::{ExpressionEvaluator, handlers::EdbHandler};
///
/// // Create evaluator with EDB handlers
/// let handlers = EdbHandler::create_handlers(engine_context);
/// let evaluator = ExpressionEvaluator::new(handlers);
///
/// // Evaluate various expressions
/// let balance = evaluator.eval("balances[msg.sender]", snapshot_id)?;
/// let is_owner = evaluator.eval("msg.sender == owner", snapshot_id)?;
/// let calculation = evaluator.eval("totalSupply() * price / 1e18", snapshot_id)?;
/// ```
#[derive(Clone)]
pub struct ExpressionEvaluator {
    handlers: EvaluatorHandlers,
}

impl ExpressionEvaluator {
    /// Create a new evaluator with the given handlers
    pub fn new(handlers: EvaluatorHandlers) -> Self {
        Self { handlers }
    }

    /// Create a new evaluator with default (empty) handlers
    pub fn new_default() -> Self {
        Self { handlers: EvaluatorHandlers::new() }
    }

    /// Create a new evaluator with debug handlers that always error
    pub fn new_debug() -> Self {
        let handlers = create_debug_handlers();
        Self { handlers }
    }

    /// Create a new evaluator with EDB handlers using the given context
    pub fn new_edb<DB>(context: Arc<EngineContext<DB>>) -> Self
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
        <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
        <DB as Database>::Error: Clone + Send + Sync,
    {
        let handlers = EdbHandler::create_handlers(context);
        Self { handlers }
    }

    /// Evaluate an expression against a specific snapshot
    pub fn eval(&self, expr: &str, snapshot_id: usize) -> Result<DynSolValue> {
        // Parse the expression
        let parsed_expr =
            parse_input(expr).map_err(|_| eyre::eyre!("Invalid expression \"{expr}\""))?;

        // Evaluate the parsed expression
        let value = self.evaluate_expression(&parsed_expr, snapshot_id)?;

        // If a validation handler is set, validate the final value
        if let Some(validation_handler) = &self.handlers.validation_handler {
            validation_handler.validate_value(value)
        } else {
            Ok(value)
        }
    }

    /// Main evaluation dispatcher for different expression types
    fn evaluate_expression(&self, expr: &Expression, snapshot_id: usize) -> Result<DynSolValue> {
        match expr {
            // Literals
            Expression::NumberLiteral(_, value, _, ident) => {
                if ident.is_none() {
                    self.evaluate_number_literal(value)
                } else {
                    bail!("Invalid number literal")
                }
            }
            Expression::HexNumberLiteral(_, value, ident) => {
                if ident.is_none() {
                    self.evaluate_number_literal(value)
                } else {
                    bail!("Invalid hex number literal")
                }
            }
            Expression::StringLiteral(literals) => self.evaluate_string_literal(literals),
            Expression::BoolLiteral(_, value) => Ok(DynSolValue::Bool(*value)),
            Expression::AddressLiteral(_, addr) => self.evaluate_address_literal(addr),

            // Variables and member access
            Expression::Variable(ident) => self.evaluate_variable(ident, snapshot_id),
            Expression::MemberAccess(_, base, member) => {
                self.evaluate_member_access(base, member, snapshot_id)
            }

            // Array and mapping access
            Expression::ArraySubscript(_, array, index) => self.evaluate_array_or_mapping_access(
                array,
                index.as_ref().map(|v| &**v),
                snapshot_id,
            ),

            // Function calls
            Expression::FunctionCall(_, func, args) => {
                self.evaluate_function_call(func, args, snapshot_id)
            }

            // Arithmetic operations
            Expression::Add(_, left, right) => {
                self.evaluate_binary_arithmetic(left, right, snapshot_id, ArithmeticOp::Add)
            }
            Expression::Subtract(_, left, right) => {
                self.evaluate_binary_arithmetic(left, right, snapshot_id, ArithmeticOp::Subtract)
            }
            Expression::Multiply(_, left, right) => {
                self.evaluate_binary_arithmetic(left, right, snapshot_id, ArithmeticOp::Multiply)
            }
            Expression::Divide(_, left, right) => {
                self.evaluate_binary_arithmetic(left, right, snapshot_id, ArithmeticOp::Divide)
            }
            Expression::Modulo(_, left, right) => {
                self.evaluate_binary_arithmetic(left, right, snapshot_id, ArithmeticOp::Modulo)
            }
            Expression::Power(_, base, exp) => {
                self.evaluate_binary_arithmetic(base, exp, snapshot_id, ArithmeticOp::Power)
            }

            // Bitwise operations
            Expression::BitwiseAnd(_, left, right) => {
                self.evaluate_bitwise(left, right, snapshot_id, BitwiseOp::And)
            }
            Expression::BitwiseOr(_, left, right) => {
                self.evaluate_bitwise(left, right, snapshot_id, BitwiseOp::Or)
            }
            Expression::BitwiseXor(_, left, right) => {
                self.evaluate_bitwise(left, right, snapshot_id, BitwiseOp::Xor)
            }
            Expression::BitwiseNot(_, operand) => self.evaluate_bitwise_not(operand, snapshot_id),
            Expression::ShiftLeft(_, left, right) => {
                self.evaluate_bitwise(left, right, snapshot_id, BitwiseOp::ShiftLeft)
            }
            Expression::ShiftRight(_, left, right) => {
                self.evaluate_bitwise(left, right, snapshot_id, BitwiseOp::ShiftRight)
            }

            // Comparison operations
            Expression::Equal(_, left, right) => {
                self.evaluate_comparison(left, right, snapshot_id, ComparisonOp::Equal)
            }
            Expression::NotEqual(_, left, right) => {
                self.evaluate_comparison(left, right, snapshot_id, ComparisonOp::NotEqual)
            }
            Expression::Less(_, left, right) => {
                self.evaluate_comparison(left, right, snapshot_id, ComparisonOp::Less)
            }
            Expression::More(_, left, right) => {
                self.evaluate_comparison(left, right, snapshot_id, ComparisonOp::Greater)
            }
            Expression::LessEqual(_, left, right) => {
                self.evaluate_comparison(left, right, snapshot_id, ComparisonOp::LessEqual)
            }
            Expression::MoreEqual(_, left, right) => {
                self.evaluate_comparison(left, right, snapshot_id, ComparisonOp::GreaterEqual)
            }

            // Logical operations
            Expression::And(_, left, right) => {
                self.evaluate_logical(left, right, snapshot_id, LogicalOp::And)
            }
            Expression::Or(_, left, right) => {
                self.evaluate_logical(left, right, snapshot_id, LogicalOp::Or)
            }
            Expression::Not(_, operand) => self.evaluate_logical_not(operand, snapshot_id),

            // Unary operations
            Expression::UnaryPlus(_, operand) => self.evaluate_expression(operand, snapshot_id),
            Expression::Negate(_, operand) => self.evaluate_unary_minus(operand, snapshot_id),

            // Ternary conditional
            Expression::ConditionalOperator(_, condition, true_expr, false_expr) => {
                self.evaluate_conditional(condition, true_expr, false_expr, snapshot_id)
            }

            // Parenthesis (just evaluate the inner expression)
            Expression::Parenthesis(_, inner) => self.evaluate_expression(inner, snapshot_id),

            // Array slice: arr[start:end]
            Expression::ArraySlice(_, array, start, end) => {
                self.evaluate_array_slice(array, start.as_deref(), end.as_deref(), snapshot_id)
            }

            // Hex literal: hex"deadbeef"
            Expression::HexLiteral(literals) => self.evaluate_hex_literal(literals),

            // Array literal: [1, 2, 3]
            Expression::ArrayLiteral(_, elements) => {
                self.evaluate_array_literal(elements, snapshot_id)
            }

            // List (tuple): (a, b, c)
            Expression::List(_, parameters) => {
                self.evaluate_list_parameters(parameters, snapshot_id)
            }

            Expression::New(..)
            | Expression::Delete(..)
            | Expression::PostDecrement(..)
            | Expression::PostIncrement(..)
            | Expression::PreDecrement(..)
            | Expression::PreIncrement(..)
            | Expression::Assign(..)
            | Expression::AssignAdd(..)
            | Expression::AssignSubtract(..)
            | Expression::AssignMultiply(..)
            | Expression::AssignDivide(..)
            | Expression::AssignModulo(..)
            | Expression::AssignShiftLeft(..)
            | Expression::AssignShiftRight(..)
            | Expression::AssignAnd(..)
            | Expression::AssignOr(..)
            | Expression::AssignXor(..)
            | Expression::FunctionCallBlock(..)
            | Expression::NamedFunctionCall(..)
            | Expression::RationalNumberLiteral(..)
            | Expression::Type(..) => bail!("Unsupported expression type: {:?}", expr),
        }
    }

    /// Evaluate number literals (default to uint256 unless explicitly typed)
    fn evaluate_number_literal(&self, value: &str) -> Result<DynSolValue> {
        // Remove underscores and parse
        let cleaned = value.replace('_', "");

        // Check for hex prefix
        let val = if cleaned.starts_with("0x") || cleaned.starts_with("0X") {
            U256::from_str_radix(&cleaned[2..], 16)?
        } else {
            U256::from_str_radix(&cleaned, 10)?
        };

        Ok(DynSolValue::Uint(val, 256))
    }

    /// Evaluate string literals
    fn evaluate_string_literal(
        &self,
        literals: &[solang_parser::pt::StringLiteral],
    ) -> Result<DynSolValue> {
        let mut result = String::new();
        for lit in literals {
            result.push_str(&lit.string);
        }
        Ok(DynSolValue::String(result))
    }

    /// Evaluate address literals
    fn evaluate_address_literal(&self, addr: &str) -> Result<DynSolValue> {
        let address = addr.parse::<Address>()?;
        Ok(DynSolValue::Address(address))
    }

    /// Evaluate variables (including special ones like msg.sender)
    fn evaluate_variable(&self, ident: &Identifier, snapshot_id: usize) -> Result<DynSolValue> {
        match ident.name.as_str() {
            "msg" | "tx" | "block" => {
                // These need to be handled in member access
                bail!("Cannot evaluate {} directly, use member access", ident.name)
            }
            name => {
                // Get variable value from snapshot
                self.get_variable_value(name, snapshot_id)
            }
        }
    }

    /// Evaluate member access (e.g., msg.sender, array.length)
    fn evaluate_member_access(
        &self,
        base: &Expression,
        member: &Identifier,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        // Special handling for global variables (msg, tx, block)
        if let Expression::Variable(base_ident) = base {
            match (base_ident.name.as_str(), member.name.as_str()) {
                ("msg", "sender") => return self.get_msg_sender(snapshot_id),
                ("msg", "value") => return self.get_msg_value(snapshot_id),
                ("tx", "origin") => return self.get_tx_origin(snapshot_id),
                ("block", "number") => return self.get_block_number(snapshot_id),
                ("block", "timestamp") => return self.get_block_timestamp(snapshot_id),
                // Additional msg/tx/block properties should be handled by extending handlers
                // rather than hardcoding values here
                _ => {}
            }
        }

        // Regular member access with built-in property handling
        let base_value = self.evaluate_expression(base, snapshot_id)?;

        // First try built-in properties, then fall back to handlers
        if let Some(builtin_result) = self.handle_builtin_property(&base_value, &member.name)? {
            return Ok(builtin_result);
        }

        // Handle struct member access via handler
        if let DynSolValue::CustomStruct { name, prop_names, tuple } = base_value {
            if let Some((idx, _)) = prop_names.iter().enumerate().find(|(_, n)| *n == &member.name)
            {
                if let Some(value) = tuple.get(idx) {
                    return Ok(value.clone());
                } else {
                    bail!("Struct {} has no member named {}", name, member.name);
                }
            } else {
                bail!("Struct {} has no member named {}", name, member.name);
            }
        }

        self.access_member(base_value, &member.name, snapshot_id)
    }

    /// Evaluate array or mapping access
    fn evaluate_array_or_mapping_access(
        &self,
        base: &Expression,
        index: Option<&Expression>,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        let index = index.ok_or_else(|| eyre::eyre!("Array/mapping access requires an index"))?;

        // Collect all indices for multi-level access
        let (root, indices) = self.collect_access_chain(base, vec![index], snapshot_id)?;

        // Get the value using all indices at once
        self.get_mapping_or_array_value(root, indices, snapshot_id)
    }

    /// Collect the full chain of array/mapping accesses
    fn collect_access_chain<'a>(
        &self,
        expr: &'a Expression,
        mut indices: Vec<&'a Expression>,
        snapshot_id: usize,
    ) -> Result<(DynSolValue, Vec<DynSolValue>)> {
        match expr {
            Expression::ArraySubscript(_, base, Some(index)) => {
                indices.insert(0, index);
                self.collect_access_chain(base, indices, snapshot_id)
            }
            _ => {
                // Base case - evaluate the root expression
                let root = self.evaluate_expression(expr, snapshot_id)?;
                let evaluated_indices = indices
                    .into_iter()
                    .map(|idx| self.evaluate_expression(idx, snapshot_id))
                    .collect::<Result<Vec<_>>>()?;
                Ok((root, evaluated_indices))
            }
        }
    }

    /// Evaluate function calls
    fn evaluate_function_call(
        &self,
        func: &Expression,
        args: &[Expression],
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        // Evaluate arguments
        let arg_values = args
            .iter()
            .map(|arg| self.evaluate_expression(arg, snapshot_id))
            .collect::<Result<Vec<_>>>()?;

        // Determine function name and callee
        let (func_name, callee) = match func {
            Expression::Variable(ident) => (ident.name.clone(), None),
            Expression::MemberAccess(_, base, member) => {
                let callee = self.evaluate_expression(base, snapshot_id)?;

                // Handle built-in properties/methods
                if let Some(result) =
                    self.handle_builtin_member_call(&member.name, &callee, &arg_values)?
                {
                    return Ok(result);
                }

                (member.name.clone(), Some(callee))
            }
            Expression::Type(_, ty) if arg_values.len() == 1 => {
                // Type conversion like uint256(x)
                let arg = arg_values.into_iter().next().unwrap();
                return self.evaluate_type_casting(ty, arg);
            }
            _ => bail!("Unsupported function call expression: {}", func),
        };

        // Call the function
        self.call_function(&func_name, &arg_values, callee.as_ref(), snapshot_id)
    }

    /// Handle built-in properties (like string.length, address.balance)
    fn handle_builtin_property(
        &self,
        value: &DynSolValue,
        property_name: &str,
    ) -> Result<Option<DynSolValue>> {
        match (property_name, value) {
            // ============ LENGTH PROPERTIES ============
            ("length", DynSolValue::String(s)) => {
                Ok(Some(DynSolValue::Uint(U256::from(s.len()), 256)))
            }
            ("length", DynSolValue::Bytes(b)) => {
                Ok(Some(DynSolValue::Uint(U256::from(b.len()), 256)))
            }
            ("length", DynSolValue::FixedBytes(_, size)) => {
                Ok(Some(DynSolValue::Uint(U256::from(*size), 256)))
            }
            ("length", DynSolValue::Tuple(arr)) => {
                Ok(Some(DynSolValue::Uint(U256::from(arr.len()), 256)))
            }

            // ============ ADDRESS PROPERTIES ============
            // Note: These should ultimately be handled by specialized handlers
            // that can access blockchain state, but we define the interface here
            ("balance", DynSolValue::Address(_)) => {
                // Delegate to handler - this is blockchain state
                Ok(None)
            }
            ("code", DynSolValue::Address(_)) => {
                // Delegate to handler - this is blockchain state
                Ok(None)
            }
            ("codehash", DynSolValue::Address(_)) => {
                // Delegate to handler - this is blockchain state
                Ok(None)
            }
            ("codesize", DynSolValue::Address(_)) => {
                // Delegate to handler - this is blockchain state
                Ok(None)
            }

            // ============ NUMERIC PROPERTIES ============
            ("abs", DynSolValue::Int(val, bits)) => {
                // Absolute value for signed integers
                let abs_val = if val.is_negative() { val.wrapping_neg() } else { *val };
                Ok(Some(DynSolValue::Uint(abs_val.into_raw(), *bits)))
            }

            // ============ TYPE CHECKING PROPERTIES ============
            ("isZero", DynSolValue::Uint(val, _bits)) => Ok(Some(DynSolValue::Bool(val.is_zero()))),
            ("isZero", DynSolValue::Int(val, _bits)) => Ok(Some(DynSolValue::Bool(val.is_zero()))),
            ("isZero", DynSolValue::Address(addr)) => {
                Ok(Some(DynSolValue::Bool(*addr == Address::ZERO)))
            }

            _ => Ok(None), // Not a built-in property we handle
        }
    }

    /// Handle built-in member calls (methods with arguments, like array.push(), string.concat())
    fn handle_builtin_member_call(
        &self,
        member_name: &str,
        callee: &DynSolValue,
        args: &[DynSolValue],
    ) -> Result<Option<DynSolValue>> {
        match (member_name, callee, args.len()) {
            // Note: Length properties are handled in handle_builtin_property since they take no
            // arguments

            // ============ ARRAY/LIST METHODS ============

            // Push method for arrays (returns new length) - with element
            ("push", DynSolValue::Tuple(arr), 1) => {
                let new_length = arr.len() + 1;
                Ok(Some(DynSolValue::Uint(U256::from(new_length), 256)))
            }
            // Push method for arrays - empty push (some Solidity arrays support this)
            ("push", DynSolValue::Tuple(arr), 0) => {
                let new_length = arr.len() + 1;
                Ok(Some(DynSolValue::Uint(U256::from(new_length), 256)))
            }
            // Pop method for arrays (returns popped element)
            ("pop", DynSolValue::Tuple(arr), 0) => {
                if arr.is_empty() {
                    bail!("Cannot pop from empty array")
                } else {
                    // Return the last element (actual popping behavior)
                    Ok(Some(arr.last().unwrap().clone()))
                }
            }

            // ============ STRING METHODS ============

            // String concatenation
            ("concat", DynSolValue::String(s), 1) => {
                if let DynSolValue::String(other) = &args[0] {
                    Ok(Some(DynSolValue::String(format!("{s}{other}"))))
                } else {
                    Ok(None)
                }
            }
            // String slice/substring (start index)
            ("slice", DynSolValue::String(s), 1) => {
                if let DynSolValue::Uint(start, _) = &args[0] {
                    let start_idx = start.to::<usize>();
                    if start_idx <= s.len() {
                        Ok(Some(DynSolValue::String(s[start_idx..].to_string())))
                    } else {
                        Ok(Some(DynSolValue::String(String::new())))
                    }
                } else {
                    Ok(None)
                }
            }
            // String slice/substring (start and end)
            ("slice", DynSolValue::String(s), 2) => {
                if let (DynSolValue::Uint(start, _), DynSolValue::Uint(end, _)) =
                    (&args[0], &args[1])
                {
                    let start_idx = start.to::<usize>();
                    let end_idx = end.to::<usize>();
                    if start_idx <= end_idx && end_idx <= s.len() {
                        Ok(Some(DynSolValue::String(s[start_idx..end_idx].to_string())))
                    } else {
                        Ok(Some(DynSolValue::String(String::new())))
                    }
                } else {
                    Ok(None)
                }
            }

            // ============ BYTES METHODS ============

            // Bytes concatenation
            ("concat", DynSolValue::Bytes(b1), 1) => {
                if let DynSolValue::Bytes(b2) = &args[0] {
                    let mut result = b1.clone();
                    result.extend_from_slice(b2);
                    Ok(Some(DynSolValue::Bytes(result)))
                } else {
                    Ok(None)
                }
            }
            // Bytes slice (start index)
            ("slice", DynSolValue::Bytes(b), 1) => {
                if let DynSolValue::Uint(start, _) = &args[0] {
                    let start_idx = start.to::<usize>();
                    if start_idx <= b.len() {
                        Ok(Some(DynSolValue::Bytes(b[start_idx..].to_vec())))
                    } else {
                        Ok(Some(DynSolValue::Bytes(Vec::new())))
                    }
                } else {
                    Ok(None)
                }
            }
            // Bytes slice (start and end)
            ("slice", DynSolValue::Bytes(b), 2) => {
                if let (DynSolValue::Uint(start, _), DynSolValue::Uint(end, _)) =
                    (&args[0], &args[1])
                {
                    let start_idx = start.to::<usize>();
                    let end_idx = end.to::<usize>();
                    if start_idx <= end_idx && end_idx <= b.len() {
                        Ok(Some(DynSolValue::Bytes(b[start_idx..end_idx].to_vec())))
                    } else {
                        Ok(Some(DynSolValue::Bytes(Vec::new())))
                    }
                } else {
                    Ok(None)
                }
            }

            // ============ MATH FUNCTIONS ============

            // Math.min for uint values
            ("min", DynSolValue::Uint(a, bits), 1) => {
                if let DynSolValue::Uint(b, _) = &args[0] {
                    Ok(Some(DynSolValue::Uint((*a).min(*b), *bits)))
                } else {
                    Ok(None)
                }
            }
            // Math.max for uint values
            ("max", DynSolValue::Uint(a, bits), 1) => {
                if let DynSolValue::Uint(b, _) = &args[0] {
                    Ok(Some(DynSolValue::Uint((*a).max(*b), *bits)))
                } else {
                    Ok(None)
                }
            }

            // ============ TYPE CHECKING FUNCTIONS ============

            // Check if string is empty
            ("isEmpty", DynSolValue::String(s), 0) => Ok(Some(DynSolValue::Bool(s.is_empty()))),
            // Check if bytes is empty
            ("isEmpty", DynSolValue::Bytes(b), 0) => Ok(Some(DynSolValue::Bool(b.is_empty()))),
            // Check if array is empty
            ("isEmpty", DynSolValue::Tuple(arr), 0) => Ok(Some(DynSolValue::Bool(arr.is_empty()))),

            _ => Ok(None), // Not a built-in we handle
        }
    }

    /// Evaluate binary arithmetic operations
    fn evaluate_binary_arithmetic(
        &self,
        left: &Expression,
        right: &Expression,
        snapshot_id: usize,
        op: ArithmeticOp,
    ) -> Result<DynSolValue> {
        let left_val = self.evaluate_expression(left, snapshot_id)?;
        let right_val = self.evaluate_expression(right, snapshot_id)?;

        self.apply_arithmetic_op(left_val, right_val, op)
    }

    /// Apply arithmetic operation on DynSolValues
    fn apply_arithmetic_op(
        &self,
        left: DynSolValue,
        right: DynSolValue,
        op: ArithmeticOp,
    ) -> Result<DynSolValue> {
        match (left, right) {
            (DynSolValue::Uint(l, bits1), DynSolValue::Uint(r, bits2)) => {
                let bits = bits1.max(bits2);
                let result = match op {
                    ArithmeticOp::Add => l.saturating_add(r),
                    ArithmeticOp::Subtract => l.saturating_sub(r),
                    ArithmeticOp::Multiply => l.saturating_mul(r),
                    ArithmeticOp::Divide => {
                        if r.is_zero() {
                            bail!("Division by zero");
                        }
                        l / r
                    }
                    ArithmeticOp::Modulo => {
                        if r.is_zero() {
                            bail!("Modulo by zero");
                        }
                        l % r
                    }
                    ArithmeticOp::Power => l.saturating_pow(r),
                };
                Ok(DynSolValue::Uint(result, bits))
            }
            (DynSolValue::Int(l, bits1), DynSolValue::Int(r, bits2)) => {
                let bits = bits1.max(bits2);
                let result = match op {
                    ArithmeticOp::Add => l.saturating_add(r),
                    ArithmeticOp::Subtract => l.saturating_sub(r),
                    ArithmeticOp::Multiply => l.saturating_mul(r),
                    ArithmeticOp::Divide => {
                        if r.is_zero() {
                            bail!("Division by zero");
                        }
                        l / r
                    }
                    ArithmeticOp::Modulo => {
                        if r.is_zero() {
                            bail!("Modulo by zero");
                        }
                        l % r
                    }
                    ArithmeticOp::Power => {
                        // Convert to U256 for power operation
                        let base_u = U256::from_le_bytes(l.to_le_bytes::<32>());
                        let exp_u = U256::from_le_bytes(r.to_le_bytes::<32>());
                        let result_u = base_u.saturating_pow(exp_u);
                        I256::from_le_bytes(result_u.to_le_bytes::<32>())
                    }
                };
                Ok(DynSolValue::Int(result, bits))
            }
            _ => bail!("Cannot apply arithmetic operation to non-numeric types"),
        }
    }

    /// Evaluate bitwise operations
    fn evaluate_bitwise(
        &self,
        left: &Expression,
        right: &Expression,
        snapshot_id: usize,
        op: BitwiseOp,
    ) -> Result<DynSolValue> {
        let left_val = self.evaluate_expression(left, snapshot_id)?;
        let right_val = self.evaluate_expression(right, snapshot_id)?;

        self.apply_bitwise_op(left_val, right_val, op)
    }

    /// Apply bitwise operation
    fn apply_bitwise_op(
        &self,
        left: DynSolValue,
        right: DynSolValue,
        op: BitwiseOp,
    ) -> Result<DynSolValue> {
        match (left, right) {
            (DynSolValue::Uint(l, bits1), DynSolValue::Uint(r, bits2)) => {
                let bits = bits1.max(bits2);
                let result = match op {
                    BitwiseOp::And => l & r,
                    BitwiseOp::Or => l | r,
                    BitwiseOp::Xor => l ^ r,
                    BitwiseOp::ShiftLeft => l << r,
                    BitwiseOp::ShiftRight => l >> r,
                };
                Ok(DynSolValue::Uint(result, bits))
            }
            _ => bail!("Bitwise operations require unsigned integer types"),
        }
    }

    /// Evaluate bitwise NOT
    fn evaluate_bitwise_not(
        &self,
        operand: &Expression,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        let val = self.evaluate_expression(operand, snapshot_id)?;
        match val {
            DynSolValue::Uint(v, bits) => Ok(DynSolValue::Uint(!v, bits)),
            _ => bail!("Bitwise NOT requires unsigned integer type"),
        }
    }

    /// Evaluate comparison operations
    fn evaluate_comparison(
        &self,
        left: &Expression,
        right: &Expression,
        snapshot_id: usize,
        op: ComparisonOp,
    ) -> Result<DynSolValue> {
        let left_val = self.evaluate_expression(left, snapshot_id)?;
        let right_val = self.evaluate_expression(right, snapshot_id)?;

        let result = self.apply_comparison_op(left_val, right_val, op)?;
        Ok(DynSolValue::Bool(result))
    }

    /// Apply comparison operation
    fn apply_comparison_op(
        &self,
        left: DynSolValue,
        right: DynSolValue,
        op: ComparisonOp,
    ) -> Result<bool> {
        match (left, right) {
            (DynSolValue::Uint(l, _), DynSolValue::Uint(r, _)) => Ok(match op {
                ComparisonOp::Equal => l == r,
                ComparisonOp::NotEqual => l != r,
                ComparisonOp::Less => l < r,
                ComparisonOp::Greater => l > r,
                ComparisonOp::LessEqual => l <= r,
                ComparisonOp::GreaterEqual => l >= r,
            }),
            (DynSolValue::Int(l, _), DynSolValue::Int(r, _)) => Ok(match op {
                ComparisonOp::Equal => l == r,
                ComparisonOp::NotEqual => l != r,
                ComparisonOp::Less => l < r,
                ComparisonOp::Greater => l > r,
                ComparisonOp::LessEqual => l <= r,
                ComparisonOp::GreaterEqual => l >= r,
            }),
            (DynSolValue::Bool(l), DynSolValue::Bool(r)) => Ok(match op {
                ComparisonOp::Equal => l == r,
                ComparisonOp::NotEqual => l != r,
                _ => bail!("Cannot compare booleans with <, >, <=, >="),
            }),
            (DynSolValue::Address(l), DynSolValue::Address(r)) => Ok(match op {
                ComparisonOp::Equal => l == r,
                ComparisonOp::NotEqual => l != r,
                _ => bail!("Cannot compare addresses with <, >, <=, >="),
            }),
            (DynSolValue::String(l), DynSolValue::String(r)) => Ok(match op {
                ComparisonOp::Equal => l == r,
                ComparisonOp::NotEqual => l != r,
                _ => bail!("Cannot compare strings with <, >, <=, >="),
            }),
            _ => bail!("Cannot compare values of different types"),
        }
    }

    /// Evaluate logical operations
    fn evaluate_logical(
        &self,
        left: &Expression,
        right: &Expression,
        snapshot_id: usize,
        op: LogicalOp,
    ) -> Result<DynSolValue> {
        let left_val = self.evaluate_expression(left, snapshot_id)?;

        // Short-circuit evaluation
        match op {
            LogicalOp::And => {
                if !self.to_bool(&left_val)? {
                    return Ok(DynSolValue::Bool(false));
                }
                let right_val = self.evaluate_expression(right, snapshot_id)?;
                Ok(DynSolValue::Bool(self.to_bool(&right_val)?))
            }
            LogicalOp::Or => {
                if self.to_bool(&left_val)? {
                    return Ok(DynSolValue::Bool(true));
                }
                let right_val = self.evaluate_expression(right, snapshot_id)?;
                Ok(DynSolValue::Bool(self.to_bool(&right_val)?))
            }
        }
    }

    /// Evaluate logical NOT
    fn evaluate_logical_not(
        &self,
        operand: &Expression,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        let val = self.evaluate_expression(operand, snapshot_id)?;
        Ok(DynSolValue::Bool(!self.to_bool(&val)?))
    }

    /// Convert value to boolean
    fn to_bool(&self, val: &DynSolValue) -> Result<bool> {
        match val {
            DynSolValue::Bool(b) => Ok(*b),
            DynSolValue::Uint(v, _) => Ok(!v.is_zero()),
            DynSolValue::Int(v, _) => Ok(!v.is_zero()),
            _ => bail!("Cannot convert {:?} to boolean", val),
        }
    }

    /// Evaluate unary minus
    fn evaluate_unary_minus(
        &self,
        operand: &Expression,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        let val = self.evaluate_expression(operand, snapshot_id)?;
        match val {
            DynSolValue::Uint(v, bits) => {
                // Convert to signed integer and negate
                let signed = I256::from_raw(v);
                Ok(DynSolValue::Int(-signed, bits))
            }
            DynSolValue::Int(v, bits) => Ok(DynSolValue::Int(-v, bits)),
            _ => bail!("Unary minus requires numeric type"),
        }
    }

    /// Evaluate conditional (ternary) operator
    fn evaluate_conditional(
        &self,
        condition: &Expression,
        true_expr: &Expression,
        false_expr: &Expression,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        let cond_val = self.evaluate_expression(condition, snapshot_id)?;

        if self.to_bool(&cond_val)? {
            self.evaluate_expression(true_expr, snapshot_id)
        } else {
            self.evaluate_expression(false_expr, snapshot_id)
        }
    }

    /// Evaluate array slice: arr[start:end]
    fn evaluate_array_slice(
        &self,
        array: &Expression,
        start: Option<&Expression>,
        end: Option<&Expression>,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        let array_val = self.evaluate_expression(array, snapshot_id)?;

        // Get the array elements
        let elements = match array_val {
            DynSolValue::Array(elements) => elements,
            DynSolValue::FixedArray(elements) => elements,
            _ => bail!("Array slice can only be applied to arrays, got {:?}", array_val),
        };

        // Evaluate start and end indices
        let start_idx = if let Some(start_expr) = start {
            let start_val = self.evaluate_expression(start_expr, snapshot_id)?;
            match start_val {
                DynSolValue::Uint(v, _) => v.to::<usize>(),
                _ => bail!("Array slice start index must be an unsigned integer"),
            }
        } else {
            0
        };

        let end_idx = if let Some(end_expr) = end {
            let end_val = self.evaluate_expression(end_expr, snapshot_id)?;
            match end_val {
                DynSolValue::Uint(v, _) => v.to::<usize>(),
                _ => bail!("Array slice end index must be an unsigned integer"),
            }
        } else {
            elements.len()
        };

        // Validate indices
        if start_idx > end_idx {
            bail!("Array slice start index {} is greater than end index {}", start_idx, end_idx);
        }
        if end_idx > elements.len() {
            bail!("Array slice end index {} exceeds array length {}", end_idx, elements.len());
        }

        // Create the slice
        let slice = elements[start_idx..end_idx].to_vec();
        Ok(DynSolValue::Array(slice))
    }

    /// Evaluate hex literal: hex"deadbeef"
    fn evaluate_hex_literal(
        &self,
        literals: &[solang_parser::pt::HexLiteral],
    ) -> Result<DynSolValue> {
        let mut bytes = Vec::new();

        for lit in literals {
            // Parse hex string to bytes
            let hex_str = &lit.hex;
            // Remove any spaces or underscores that might be in the hex literal
            let cleaned = hex_str.replace(['_', ' '], "");

            // Ensure even number of characters
            let hex = if cleaned.len() % 2 != 0 { format!("0{cleaned}") } else { cleaned };

            // Convert hex string to bytes
            for chunk in hex.as_bytes().chunks(2) {
                let hex_byte = std::str::from_utf8(chunk)?;
                let byte = u8::from_str_radix(hex_byte, 16)?;
                bytes.push(byte);
            }
        }

        Ok(DynSolValue::Bytes(bytes))
    }

    /// Evaluate array literal: [1, 2, 3]
    fn evaluate_array_literal(
        &self,
        elements: &[Expression],
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        let mut values = Vec::new();

        for element in elements {
            let val = self.evaluate_expression(element, snapshot_id)?;
            values.push(val);
        }

        Ok(DynSolValue::Array(values))
    }

    /// Evaluate list (tuple): (a, b, c)
    fn evaluate_list_parameters(
        &self,
        parameters: &[(Loc, Option<Parameter>)],
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        let mut values = Vec::new();

        for (_, param_opt) in parameters {
            if let Some(param) = param_opt {
                values.push(self.evaluate_expression(&param.ty, snapshot_id)?);
            } else {
                bail!("Invalid none parameter in list");
            }
        }

        Ok(DynSolValue::Tuple(values))
    }

    /// Evaluate type casting: uint256(x), address(y), etc.
    fn evaluate_type_casting(&self, target_type: &Type, value: DynSolValue) -> Result<DynSolValue> {
        match target_type {
            // Address casting
            Type::Address => match value {
                DynSolValue::Address(addr) => Ok(DynSolValue::Address(addr)),
                DynSolValue::Uint(val, _) => {
                    // Convert U256 to Address (take lower 160 bits)
                    let addr_bytes = val.to_be_bytes::<32>();
                    let addr = Address::from_slice(&addr_bytes[12..]);
                    Ok(DynSolValue::Address(addr))
                }
                DynSolValue::FixedBytes(bytes, 20) => {
                    let addr = Address::from_slice(&bytes[..20]);
                    Ok(DynSolValue::Address(addr))
                }
                _ => bail!("Cannot cast {:?} to address", value),
            },

            // Unsigned integer casting with proper size and truncation
            Type::Uint(bits) => {
                let target_bits = *bits as usize;
                // Create mask for truncation
                let mask = if target_bits >= 256 {
                    U256::MAX
                } else {
                    (U256::from(1) << target_bits) - U256::from(1)
                };
                match value {
                    DynSolValue::Uint(val, _) => {
                        let truncated = val & mask;
                        Ok(DynSolValue::Uint(truncated, target_bits))
                    }
                    DynSolValue::Int(val, _) => {
                        let unsigned = val.into_raw() & mask;
                        Ok(DynSolValue::Uint(unsigned, target_bits))
                    }
                    DynSolValue::Address(addr) => {
                        let val = U256::from_be_slice(addr.as_slice()) & mask;
                        Ok(DynSolValue::Uint(val, target_bits))
                    }
                    DynSolValue::Bool(b) => {
                        let val = if b { U256::from(1) } else { U256::ZERO };
                        Ok(DynSolValue::Uint(val, target_bits))
                    }
                    _ => bail!("Cannot cast {:?} to uint{}", value, target_bits),
                }
            }

            // Signed integer casting with proper size and truncation
            Type::Int(bits) => {
                let target_bits = *bits as usize;
                // For signed integers, we need to handle sign extension properly
                let truncate_signed = |val: I256| -> I256 {
                    if target_bits >= 256 {
                        return val;
                    }
                    // Create mask for the value bits
                    let mask = (U256::from(1) << target_bits) - U256::from(1);
                    let truncated = val.into_raw() & mask;

                    // Check if the sign bit is set
                    let sign_bit = U256::from(1) << (target_bits - 1);
                    if truncated & sign_bit != U256::ZERO {
                        // Negative number - sign extend
                        let extension = U256::MAX ^ mask;
                        I256::from_raw(truncated | extension)
                    } else {
                        // Positive number
                        I256::from_raw(truncated)
                    }
                };

                match value {
                    DynSolValue::Int(val, _) => {
                        let truncated = truncate_signed(val);
                        Ok(DynSolValue::Int(truncated, target_bits))
                    }
                    DynSolValue::Uint(val, _) => {
                        let signed = I256::from_raw(val);
                        let truncated = truncate_signed(signed);
                        Ok(DynSolValue::Int(truncated, target_bits))
                    }
                    DynSolValue::Bool(b) => {
                        let val = if b { I256::from_raw(U256::from(1)) } else { I256::ZERO };
                        Ok(DynSolValue::Int(val, target_bits))
                    }
                    _ => bail!("Cannot cast {:?} to int{}", value, target_bits),
                }
            }

            // Boolean casting
            Type::Bool => match value {
                DynSolValue::Bool(b) => Ok(DynSolValue::Bool(b)),
                DynSolValue::Uint(val, _) => Ok(DynSolValue::Bool(!val.is_zero())),
                DynSolValue::Int(val, _) => Ok(DynSolValue::Bool(!val.is_zero())),
                _ => bail!("Cannot cast {:?} to bool", value),
            },

            // Fixed bytes casting (e.g., bytes32)
            Type::Bytes(size) => {
                let target_size = *size as usize;
                match value {
                    DynSolValue::Bytes(bytes) => {
                        // Always use 32-byte buffer for B256
                        let mut fixed_bytes = vec![0u8; 32];
                        let copy_len = bytes.len().min(target_size);
                        fixed_bytes[..copy_len].copy_from_slice(&bytes[..copy_len]);
                        Ok(DynSolValue::FixedBytes(B256::from_slice(&fixed_bytes), target_size))
                    }
                    DynSolValue::FixedBytes(bytes, _) => {
                        // Convert between different fixed sizes
                        let mut fixed_bytes = vec![0u8; 32];
                        let copy_len = bytes.len().min(target_size).min(32);
                        fixed_bytes[..copy_len].copy_from_slice(&bytes[..copy_len]);
                        Ok(DynSolValue::FixedBytes(B256::from_slice(&fixed_bytes), target_size))
                    }
                    DynSolValue::String(s) => {
                        // Convert string to fixed bytes
                        let bytes = s.as_bytes();
                        let mut fixed_bytes = vec![0u8; 32];
                        let copy_len = bytes.len().min(target_size);
                        fixed_bytes[..copy_len].copy_from_slice(&bytes[..copy_len]);
                        Ok(DynSolValue::FixedBytes(B256::from_slice(&fixed_bytes), target_size))
                    }
                    DynSolValue::Uint(val, _) => {
                        // Convert uint to fixed bytes (big-endian)
                        let mut fixed_bytes = vec![0u8; 32];
                        let be_bytes = val.to_be_bytes::<32>();
                        if target_size < 32 {
                            // Copy from the end of be_bytes (rightmost bytes for smaller sizes)
                            let src_start = 32 - target_size;
                            fixed_bytes[..target_size].copy_from_slice(&be_bytes[src_start..]);
                        } else {
                            fixed_bytes = be_bytes.to_vec();
                        }
                        Ok(DynSolValue::FixedBytes(B256::from_slice(&fixed_bytes), target_size))
                    }
                    _ => bail!("Cannot cast {:?} to bytes{}", value, target_size),
                }
            }

            // Dynamic bytes casting (simplified)
            Type::DynamicBytes => match value {
                DynSolValue::Bytes(bytes) => Ok(DynSolValue::Bytes(bytes)),
                DynSolValue::FixedBytes(bytes, _) => Ok(DynSolValue::Bytes(bytes.to_vec())),
                DynSolValue::String(s) => Ok(DynSolValue::Bytes(s.into_bytes())),
                DynSolValue::Uint(val, _) => Ok(DynSolValue::Bytes(val.to_be_bytes_vec())),
                _ => bail!("Cannot cast {:?} to bytes", value),
            },

            // String casting
            Type::String => match value {
                DynSolValue::String(s) => Ok(DynSolValue::String(s)),
                DynSolValue::Bytes(bytes) => {
                    let s =
                        String::from_utf8(bytes).map_err(|_| eyre::eyre!("Invalid UTF-8 bytes"))?;
                    Ok(DynSolValue::String(s))
                }
                DynSolValue::FixedBytes(bytes, _) => {
                    let s = String::from_utf8(bytes.to_vec())
                        .map_err(|_| eyre::eyre!("Invalid UTF-8 bytes"))?;
                    Ok(DynSolValue::String(s))
                }
                _ => bail!("Cannot cast {:?} to string", value),
            },

            // Unsupported types
            _ => bail!("Type casting to {:?} is not yet supported", target_type),
        }
    }

    // ========== Utility function stubs ==========
    // These will be implemented later by the user

    /// Get variable value from snapshot using handler
    fn get_variable_value(&self, name: &str, snapshot_id: usize) -> Result<DynSolValue> {
        match &self.handlers.variable_handler {
            Some(handler) => handler.get_variable_value(name, snapshot_id),
            None => bail!("No variable handler configured"),
        }
    }

    /// Get mapping or array value using multiple indices
    fn get_mapping_or_array_value(
        &self,
        root: DynSolValue,
        indices: Vec<DynSolValue>,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        match &self.handlers.mapping_array_handler {
            Some(handler) => handler.get_mapping_or_array_value(root, indices, snapshot_id),
            None => bail!("No mapping/array handler configured"),
        }
    }

    /// Call a function
    fn call_function(
        &self,
        name: &str,
        args: &[DynSolValue],
        callee: Option<&DynSolValue>,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        match &self.handlers.function_call_handler {
            Some(handler) => handler.call_function(name, args, callee, snapshot_id),
            None => bail!("No function call handler configured"),
        }
    }

    /// Access member of a value (for handler delegation)
    fn access_member(
        &self,
        value: DynSolValue,
        member: &str,
        snapshot_id: usize,
    ) -> Result<DynSolValue> {
        // This method is only called after built-in properties have been checked
        // in evaluate_member_access, so we delegate directly to handlers
        match &self.handlers.member_access_handler {
            Some(handler) => handler.access_member(value, member, snapshot_id),
            None => bail!("No member access handler configured"),
        }
    }

    /// Get msg.sender
    fn get_msg_sender(&self, snapshot_id: usize) -> Result<DynSolValue> {
        match &self.handlers.msg_handler {
            Some(handler) => handler.get_msg_sender(snapshot_id),
            None => bail!("No msg handler configured"),
        }
    }

    /// Get msg.value
    fn get_msg_value(&self, snapshot_id: usize) -> Result<DynSolValue> {
        match &self.handlers.msg_handler {
            Some(handler) => handler.get_msg_value(snapshot_id),
            None => bail!("No msg handler configured"),
        }
    }

    /// Get tx.origin
    fn get_tx_origin(&self, snapshot_id: usize) -> Result<DynSolValue> {
        match &self.handlers.tx_handler {
            Some(handler) => handler.get_tx_origin(snapshot_id),
            None => bail!("No tx handler configured"),
        }
    }

    /// Get block.number
    fn get_block_number(&self, snapshot_id: usize) -> Result<DynSolValue> {
        match &self.handlers.block_handler {
            Some(handler) => handler.get_block_number(snapshot_id),
            None => bail!("No block handler configured"),
        }
    }

    /// Get block.timestamp
    fn get_block_timestamp(&self, snapshot_id: usize) -> Result<DynSolValue> {
        match &self.handlers.block_handler {
            Some(handler) => handler.get_block_timestamp(snapshot_id),
            None => bail!("No block handler configured"),
        }
    }
}

// Operation enums for better code organization
#[derive(Debug)]
enum ArithmeticOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Power,
}

#[derive(Debug)]
enum BitwiseOp {
    And,
    Or,
    Xor,
    ShiftLeft,
    ShiftRight,
}

#[derive(Debug)]
enum ComparisonOp {
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
}

#[derive(Debug)]
enum LogicalOp {
    And,
    Or,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::handlers::{
        MappingArrayHandler, MemberAccessHandler,
        debug::{create_debug_handlers, create_simulation_debug_handlers},
    };
    use alloy_primitives::{U256, address};

    #[test]
    fn test_evaluate_number_literal() {
        // Just test that the structure exists and basic methods work
        // Since we don't have a real context, we can't fully test yet

        // Test parsing different number formats
        let evaluator = ExpressionEvaluator::new_default();

        // Test hex number
        let result = evaluator.evaluate_number_literal("0x1234");
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            assert_eq!(val, U256::from(0x1234));
            assert_eq!(bits, 256);
        }

        // Test decimal number
        let result = evaluator.evaluate_number_literal("42");
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            assert_eq!(val, U256::from(42));
            assert_eq!(bits, 256);
        }

        // Test number with underscores
        let result = evaluator.evaluate_number_literal("1_000_000");
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            assert_eq!(val, U256::from(1000000));
            assert_eq!(bits, 256);
        }
    }

    #[test]
    fn test_apply_arithmetic_ops() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test addition
        let left = DynSolValue::Uint(U256::from(10), 256);
        let right = DynSolValue::Uint(U256::from(20), 256);
        let result = evaluator.apply_arithmetic_op(left, right, ArithmeticOp::Add);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(30));
        }

        // Test subtraction
        let left = DynSolValue::Uint(U256::from(30), 256);
        let right = DynSolValue::Uint(U256::from(10), 256);
        let result = evaluator.apply_arithmetic_op(left, right, ArithmeticOp::Subtract);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(20));
        }

        // Test multiplication
        let left = DynSolValue::Uint(U256::from(5), 256);
        let right = DynSolValue::Uint(U256::from(6), 256);
        let result = evaluator.apply_arithmetic_op(left, right, ArithmeticOp::Multiply);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(30));
        }

        // Test division
        let left = DynSolValue::Uint(U256::from(100), 256);
        let right = DynSolValue::Uint(U256::from(5), 256);
        let result = evaluator.apply_arithmetic_op(left, right, ArithmeticOp::Divide);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(20));
        }

        // Test division by zero
        let left = DynSolValue::Uint(U256::from(100), 256);
        let right = DynSolValue::Uint(U256::from(0), 256);
        let result = evaluator.apply_arithmetic_op(left, right, ArithmeticOp::Divide);
        assert!(result.is_err());
    }

    #[test]
    fn test_comparison_ops() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test equality
        let left = DynSolValue::Uint(U256::from(10), 256);
        let right = DynSolValue::Uint(U256::from(10), 256);
        let result = evaluator.apply_comparison_op(left, right, ComparisonOp::Equal);
        assert!(result.unwrap());

        // Test inequality
        let left = DynSolValue::Uint(U256::from(10), 256);
        let right = DynSolValue::Uint(U256::from(20), 256);
        let result = evaluator.apply_comparison_op(left, right, ComparisonOp::NotEqual);
        assert!(result.unwrap());

        // Test less than
        let left = DynSolValue::Uint(U256::from(10), 256);
        let right = DynSolValue::Uint(U256::from(20), 256);
        let result = evaluator.apply_comparison_op(left, right, ComparisonOp::Less);
        assert!(result.unwrap());

        // Test greater than
        let left = DynSolValue::Uint(U256::from(30), 256);
        let right = DynSolValue::Uint(U256::from(20), 256);
        let result = evaluator.apply_comparison_op(left, right, ComparisonOp::Greater);
        assert!(result.unwrap());
    }

    #[test]
    fn test_to_bool() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test bool value
        assert!(evaluator.to_bool(&DynSolValue::Bool(true)).unwrap());
        assert!(!evaluator.to_bool(&DynSolValue::Bool(false)).unwrap());

        // Test uint values
        assert!(evaluator.to_bool(&DynSolValue::Uint(U256::from(1), 256)).unwrap());
        assert!(!evaluator.to_bool(&DynSolValue::Uint(U256::from(0), 256)).unwrap());
        assert!(evaluator.to_bool(&DynSolValue::Uint(U256::from(100), 256)).unwrap());

        // Test int values
        assert!(evaluator.to_bool(&DynSolValue::Int(I256::from_raw(U256::from(1)), 256)).unwrap());
        assert!(!evaluator.to_bool(&DynSolValue::Int(I256::from_raw(U256::from(0)), 256)).unwrap());
        assert!(
            evaluator
                .to_bool(&DynSolValue::Int(I256::from_raw(U256::from(1)).wrapping_neg(), 256))
                .unwrap()
        );
    }

    // ========== Direct eval() method tests ==========

    #[test]
    fn test_eval_number_literals() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test decimal numbers
        let result = evaluator.eval("42", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            assert_eq!(val, U256::from(42));
            assert_eq!(bits, 256);
        }

        // Test hex numbers
        let result = evaluator.eval("0x1234", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            assert_eq!(val, U256::from(0x1234));
            assert_eq!(bits, 256);
        }

        // Test numbers with underscores
        let result = evaluator.eval("1_000_000", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            assert_eq!(val, U256::from(1_000_000));
            assert_eq!(bits, 256);
        }

        // Test large hex numbers
        let result =
            evaluator.eval("0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            assert_eq!(val, U256::MAX);
            assert_eq!(bits, 256);
        }
    }

    #[test]
    fn test_eval_string_literals() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test simple string
        let result = evaluator.eval("\"hello world\"", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::String(s)) = result {
            assert_eq!(s, "hello world");
        }

        // Test empty string
        let result = evaluator.eval("\"\"", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::String(s)) = result {
            assert_eq!(s, "");
        }
    }

    #[test]
    fn test_eval_bool_literals() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test true
        let result = evaluator.eval("true", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        // Test false
        let result = evaluator.eval("false", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(!b);
        }
    }

    #[test]
    fn test_eval_arithmetic_expressions() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test simple addition
        let result = evaluator.eval("10 + 20", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(30));
        }

        // Test subtraction
        let result = evaluator.eval("50 - 20", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(30));
        }

        // Test multiplication
        let result = evaluator.eval("6 * 7", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(42));
        }

        // Test division
        let result = evaluator.eval("100 / 5", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(20));
        }

        // Test modulo
        let result = evaluator.eval("17 % 5", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(2));
        }

        // Test power
        let result = evaluator.eval("2 ** 8", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(256));
        }
    }

    #[test]
    fn test_eval_complex_arithmetic() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test precedence: 2 + 3 * 4 = 14
        let result = evaluator.eval("2 + 3 * 4", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(14));
        }

        // Test parentheses: (2 + 3) * 4 = 20
        let result = evaluator.eval("(2 + 3) * 4", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(20));
        }

        // Test complex expression: (10 + 5) * 2 - 8 / 4 = 28
        let result = evaluator.eval("(10 + 5) * 2 - 8 / 4", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(28));
        }
    }

    #[test]
    fn test_eval_unary_operations() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test unary plus
        let result = evaluator.eval("+42", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(42));
        }

        // Test unary minus (converts to signed)
        let result = evaluator.eval("-42", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Int(val, _)) = result {
            assert_eq!(val, I256::from_raw(U256::from(42)).wrapping_neg());
        }

        // Test logical not on boolean
        let result = evaluator.eval("!true", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(!b);
        }

        // Test logical not on number (non-zero becomes false)
        let result = evaluator.eval("!42", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(!b);
        }

        // Test logical not on zero (becomes true)
        let result = evaluator.eval("!0", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }
    }

    #[test]
    fn test_eval_comparison_operations() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test equality
        let result = evaluator.eval("10 == 10", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        let result = evaluator.eval("10 == 20", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(!b);
        }

        // Test inequality
        let result = evaluator.eval("10 != 20", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        // Test less than
        let result = evaluator.eval("10 < 20", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        // Test greater than
        let result = evaluator.eval("30 > 20", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        // Test less than or equal
        let result = evaluator.eval("10 <= 10", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        // Test greater than or equal
        let result = evaluator.eval("20 >= 10", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }
    }

    #[test]
    fn test_eval_logical_operations() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test logical AND
        let result = evaluator.eval("true && true", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        let result = evaluator.eval("true && false", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(!b);
        }

        // Test logical OR
        let result = evaluator.eval("true || false", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        let result = evaluator.eval("false || false", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(!b);
        }

        // Test short-circuit evaluation with numbers
        let result = evaluator.eval("0 && 42", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(!b);
        }

        let result = evaluator.eval("1 || 0", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }
    }

    #[test]
    fn test_eval_ternary_operator() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test true condition
        let result = evaluator.eval("true ? 42 : 99", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(42));
        }

        // Test false condition
        let result = evaluator.eval("false ? 42 : 99", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(99));
        }

        // Test with expression condition
        let result = evaluator.eval("(10 > 5) ? \"yes\" : \"no\"", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::String(s)) = result {
            assert_eq!(s, "yes");
        }

        // Test nested ternary
        let result = evaluator.eval("true ? (false ? 1 : 2) : 3", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(2));
        }
    }

    #[test]
    fn test_eval_error_cases() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test division by zero
        let result = evaluator.eval("10 / 0", 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Division by zero"));

        // Test modulo by zero
        let result = evaluator.eval("10 % 0", 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Modulo by zero"));

        // Test invalid syntax (this will be caught by the parser)
        let result = evaluator.eval("10 +", 0);
        assert!(result.is_err());

        // Test variables (should fail since no handler configured)
        let result = evaluator.eval("someVariable", 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No variable handler configured"));

        // Test msg.sender (should fail since no handler configured)
        let result = evaluator.eval("msg.sender", 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No msg handler configured"));
    }

    #[test]
    fn test_eval_complex_expressions() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test complex arithmetic with precedence
        let result = evaluator.eval("(2 + 3) * 4 - 1", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(19)); // (2+3)*4-1 = 5*4-1 = 20-1 = 19
        }

        // Test complex logical expressions
        let result = evaluator.eval("(5 > 3) && (2 == 2) || false", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        // Test nested conditional expressions
        let result = evaluator.eval("5 > 3 ? (2 + 2) : (3 * 3)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(4));
        }
    }

    #[test]
    fn test_eval_with_debug_handlers() {
        let handlers = create_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Test that debug handlers provide detailed error messages
        let result = evaluator.eval("someVariable", 0);
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("DebugHandler::get_variable_value"));
        assert!(error.contains("name='someVariable'"));
        assert!(error.contains("snapshot_id=0"));

        // Test debug handler for function calls
        let result = evaluator.eval("someFunction(1, 2)", 0);
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("DebugHandler::call_function"));
        assert!(error.contains("name='someFunction'"));
    }

    #[test]
    fn test_eval_type_casting_comprehensive() {
        let evaluator = ExpressionEvaluator::new_default();

        // ===== ADDRESS CASTING =====

        // Address from uint256 (zero address)
        let result = evaluator.eval("address(0)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Address(addr)) = result {
            assert_eq!(addr, Address::ZERO);
        }

        // Address from uint256 (non-zero address)
        let result = evaluator.eval("address(0x1234567890123456789012345678901234567890)", 0);
        assert!(result.is_ok());

        // Address from bytes20 (should work)
        // Note: This would require proper bytes literal parsing

        // ===== UINT CASTING =====

        // Uint256 from bool (true -> 1)
        let result = evaluator.eval("uint256(true)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            assert_eq!(val, U256::from(1));
            assert_eq!(bits, 256);
        }

        // Uint256 from bool (false -> 0)
        let result = evaluator.eval("uint256(false)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            assert_eq!(val, U256::ZERO);
            assert_eq!(bits, 256);
        }

        // Uint8 truncation test
        let result = evaluator.eval("uint8(257)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            assert_eq!(val, U256::from(1)); // 257 % 256 = 1
            assert_eq!(bits, 8);
        }

        // Uint16 truncation test
        let result = evaluator.eval("uint16(65537)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            assert_eq!(val, U256::from(1)); // 65537 % 65536 = 1
            assert_eq!(bits, 16);
        }

        // Uint32 from large number
        let result = evaluator.eval("uint32(0xFFFFFFFF)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            assert_eq!(val, U256::from(0xFFFFFFFF_u32));
            assert_eq!(bits, 32);
        }

        // ===== INT CASTING =====

        // Int256 from bool
        let result = evaluator.eval("int256(true)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Int(val, bits)) = result {
            assert_eq!(val, I256::from_raw(U256::from(1)));
            assert_eq!(bits, 256);
        }

        // Int8 from positive number
        let result = evaluator.eval("int8(127)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Int(val, bits)) = result {
            assert_eq!(val, I256::from_raw(U256::from(127)));
            assert_eq!(bits, 8);
        }

        // Int8 overflow/truncation (128 becomes -128 in two's complement)
        let result = evaluator.eval("int8(128)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Int(_val, bits)) = result {
            // In 8-bit signed, 128 (0x80) should be -128 due to sign extension
            assert_eq!(bits, 8);
            // The exact value depends on how sign extension is handled
        }

        // ===== BOOL CASTING =====

        // Bool from uint (non-zero -> true)
        let result = evaluator.eval("bool(1)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        // Bool from uint (zero -> false)
        let result = evaluator.eval("bool(0)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(!b);
        }

        // Bool from large uint (non-zero -> true)
        let result = evaluator.eval("bool(0xFFFFFFFFFFFFFFFF)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        // Bool from int (positive -> true)
        let result = evaluator.eval("bool(int256(1))", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        // ===== BYTES CASTING =====

        // Note: These would require proper bytes literal syntax support
        // bytes32 from string would be: bytes32("hello")
        // bytes from uint would be: bytes(uint256(0x1234))

        // ===== CHAINED CASTING =====

        // Multiple casts in one expression
        let result = evaluator.eval("bool(uint8(257))", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            // 257 -> uint8(1) -> bool(true)
            assert!(b);
        }

        // Cast in arithmetic expression
        let result = evaluator.eval("uint256(true) + uint256(false)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(1)); // 1 + 0 = 1
        }

        // Cast with comparison
        let result = evaluator.eval("uint8(300) == uint8(44)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            // 300 % 256 = 44, 44 % 256 = 44, so they're equal
            assert!(b);
        }

        // ===== ERROR CASES =====

        // Invalid cast combinations should fail gracefully
        // These might not parse correctly, but should not panic

        // ===== COMPLEX CASTING SCENARIOS =====

        // Cast in ternary operator
        let result = evaluator.eval("true ? uint256(1) : uint256(0)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(1));
        }

        // Nested casts with different bit sizes
        let result = evaluator.eval("uint256(uint8(uint16(65793)))", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, bits)) = result {
            // 65793 % 65536 = 257, 257 % 256 = 1
            assert_eq!(val, U256::from(1));
            assert_eq!(bits, 256);
        }

        // Address arithmetic with casting
        let result = evaluator.eval("uint256(address(0x1234)) + 1", 0);
        assert!(result.is_ok());

        // Bool logic with casting
        let result = evaluator.eval("bool(1) && bool(0)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(!b); // true && false = false
        }

        // Mixed type operations requiring implicit casting behavior
        let result = evaluator.eval("uint8(255) + 1", 0);
        assert!(result.is_ok());
        // This tests whether the cast happens before or after the addition
    }

    #[test]
    fn test_eval_error_propagation() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test division by zero (should be caught during evaluation)
        let result = evaluator.eval("5 / 0", 0);
        eprintln!("Error: {result:?}");
        assert!(result.unwrap_err().to_string().to_lowercase().contains("division by zero"));

        // Test invalid operations
        let result = evaluator.eval("5 + true", 0);
        assert!(result.is_err());

        // Test handler not configured errors
        let result = evaluator.eval("unknownVar", 0);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .to_lowercase()
                .contains("no variable handler configured")
        );
    }

    #[test]
    fn test_eval_edge_cases() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test large numbers
        let result = evaluator.eval(
            "115792089237316195423570985008687907853269984665640564039457584007913129639935",
            0,
        );
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::MAX);
        }

        // Test negative numbers (should convert to signed)
        let result = evaluator.eval("-42", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Int(val, _)) = result {
            assert!(val < I256::ZERO);
        }

        // Test string literals
        let result = evaluator.eval("\"hello world\"", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::String(s)) = result {
            assert_eq!(s, "hello world");
        }

        // Test address literals
        let result = evaluator.eval("0x1234567890123456789012345678901234567890", 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_eval_array_literals() {
        // Note: Since array literal syntax like [1, 2, 3] may not be directly supported
        // by the current parser, we test array-related functionality that is available

        // Test array length access (this uses member access)
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator_with_handlers = ExpressionEvaluator::new(handlers);

        // Test accessing array.length property
        let result = evaluator_with_handlers.eval("arrayVar.length", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(10)); // Default length from debug handler
        }

        // Test address balance property (common array-like access pattern)
        let result = evaluator_with_handlers.eval("someAddress.balance", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(1000000)); // Default balance from debug handler
        }

        // Test code property (bytes-like array access)
        let result = evaluator_with_handlers.eval("contractAddr.code", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bytes(_)) = result {
            // Should succeed in getting code bytes
        }

        // Check that array-related operations are logged
        let log = debug_handler.get_log();
        assert!(!log.is_empty());
        assert!(log.iter().any(|entry| entry.contains("access_member")));

        // Verify multiple member accesses were logged
        let member_accesses = log.iter().filter(|entry| entry.contains("access_member")).count();
        assert!(member_accesses >= 3, "Should have logged at least 3 member accesses");
    }

    #[test]
    fn test_eval_hex_literals() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test simple hex numbers (these are parsed as number literals with hex format)
        let result = evaluator.eval("0x42", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(0x42));
        }

        // Test larger hex number
        let result = evaluator.eval("0x1234ABCD", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(0x1234ABCD));
        }

        // Test max hex value for different bit sizes
        let result = evaluator.eval("0xFF", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(255));
        }

        // Test hex with all hex digits
        let result = evaluator.eval("0xDEADBEEF", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(0xDEADBEEFu32));
        }

        // Test arithmetic with hex literals
        let result = evaluator.eval("0x10 + 0x20", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(0x30)); // 16 + 32 = 48
        }

        // Test type casting with hex
        let result = evaluator.eval("address(0x0)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Address(addr)) = result {
            assert_eq!(addr, Address::ZERO);
        }

        // Test hex address
        let result = evaluator.eval("address(0x1234567890123456789012345678901234567890)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Address(_)) = result {
            // Should succeed in parsing as an address
        } else {
            panic!("Failed to cast hex literal to address");
        }
    }

    #[test]
    fn test_eval_list_tuples() {
        // Note: Since tuple syntax like (a, b, c) may not be directly supported
        // by the current parser, we test tuple-related functionality that is available

        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Test function calls that return multiple values (tuple-like behavior)
        let result = evaluator.eval("multiReturnFunc()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Tuple(values)) = result {
            assert!(!values.is_empty());
        }

        // Test variable assignment that could be tuple-like
        let result = evaluator.eval("tupleVar", 0);
        assert!(result.is_ok());
        // The debug handler should return a tuple for variables ending in "Tuple" or similar

        // Test struct-like access which is similar to tuple access
        let result = evaluator.eval("structVar.field1", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(42)); // Default value from debug handler for member access
        }

        // Test nested member access (tuple-like operations)
        let result = evaluator.eval("nestedStruct.inner.value", 0);
        assert!(result.is_ok());

        // Test tuple element access simulation via array-like access
        // Since actual tuple syntax might not be supported, we simulate it
        let result = evaluator.eval("pairData.first", 0);
        assert!(result.is_ok());

        let result = evaluator.eval("pairData.second", 0);
        assert!(result.is_ok());

        // Check that tuple-related operations are logged
        let log = debug_handler.get_log();
        assert!(!log.is_empty());

        // Verify that function calls and member accesses were logged
        let function_calls = log.iter().filter(|entry| entry.contains("call_function")).count();
        let member_accesses = log.iter().filter(|entry| entry.contains("access_member")).count();

        assert!(function_calls >= 1, "Should have logged at least 1 function call");
        assert!(member_accesses >= 4, "Should have logged at least 4 member accesses");
    }

    #[test]
    fn test_simulation_debug_handler_basic() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Test variable access with auto-generated values
        let result = evaluator.eval("balance", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(1000000)); // Expected balance default
        }

        // Test msg.sender access
        let result = evaluator.eval("msg.sender", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Address(addr)) = result {
            assert_eq!(addr, Address::from([0x42; 20])); // Expected mock address
        }

        // Check that operations were logged
        let log = debug_handler.get_log();
        assert!(!log.is_empty());
        assert!(log.iter().any(|entry| entry.contains("get_variable_value")));
        assert!(log.iter().any(|entry| entry.contains("get_msg_sender")));
    }

    #[test]
    fn test_simulation_debug_handler_custom_values() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Set custom variable values
        debug_handler.set_variable("myBalance", DynSolValue::Uint(U256::from(5000), 256));
        debug_handler.set_variable("myAddress", DynSolValue::Address(Address::from([0x99; 20])));

        // Test that custom values are returned
        let result = evaluator.eval("myBalance", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(5000));
        }

        let result = evaluator.eval("myAddress", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Address(addr)) = result {
            assert_eq!(addr, Address::from([0x99; 20]));
        }

        // Verify logging captured the custom values
        let log = debug_handler.get_log();
        assert!(log.iter().any(|entry| entry.contains("returning stored value")));
    }

    #[test]
    fn test_simulation_debug_handler_complex_expressions() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Set up some mock data
        debug_handler.set_variable("userBalance", DynSolValue::Uint(U256::from(2000), 256));
        debug_handler.set_function("balanceOf", DynSolValue::Uint(U256::from(1500), 256));

        debug_handler.clear_log(); // Clear previous logs

        // Test complex expression with variables and function calls
        let result = evaluator.eval("userBalance + balanceOf(msg.sender)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(3500)); // 2000 + 1500
        }

        // Check the execution log for detailed tracing
        let log = debug_handler.get_log();
        println!("Execution log:");
        for entry in &log {
            println!("  {entry}");
        }

        // Verify all operations were logged
        assert!(log.iter().any(|entry| entry.contains("get_variable_value: name='userBalance'")));
        assert!(log.iter().any(|entry| entry.contains("call_function: name='balanceOf'")));
        assert!(log.iter().any(|entry| entry.contains("get_msg_sender")));
    }

    #[test]
    fn test_simulation_debug_handler_mapping_access() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let _evaluator = ExpressionEvaluator::new(handlers);

        debug_handler.clear_log();

        // Test mapping access (this would require actual mapping syntax parsing)
        // For now we test the handler directly
        let mock_mapping = DynSolValue::Uint(U256::from(0), 256); // Mock mapping root
        let indices = vec![DynSolValue::Address(Address::from([0x11; 20]))]; // Mock address key

        let result = debug_handler.get_mapping_or_array_value(mock_mapping, indices, 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(1000000)); // Expected address mapping value
        }

        // Check logging
        let log = debug_handler.get_log();
        assert!(log.iter().any(|entry| entry.contains("get_mapping_or_array_value")));
    }

    #[test]
    fn test_simulation_debug_handler_member_access() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let _evaluator = ExpressionEvaluator::new(handlers);

        // Test member access (this would require actual member access syntax)
        // For now we test the handler directly
        let mock_object = DynSolValue::Address(Address::from([0x42; 20]));

        let result = debug_handler.access_member(mock_object, "balance", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(1000000)); // Expected balance value
        }

        let result = debug_handler.access_member(DynSolValue::Uint(U256::ZERO, 256), "length", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(10)); // Expected array length
        }
    }

    #[test]
    fn test_debug_handler_comparison() {
        // Compare error-only debug handler vs simulation debug handler

        // Test with error-only debug handler
        let error_handlers = create_debug_handlers();
        let error_evaluator = ExpressionEvaluator::new(error_handlers);

        let result = error_evaluator.eval("someVar", 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("DebugHandler::get_variable_value"));

        // Test with simulation debug handler
        let (sim_handlers, _sim_debug) = create_simulation_debug_handlers();
        let sim_evaluator = ExpressionEvaluator::new(sim_handlers);

        let result = sim_evaluator.eval("someVar", 0);
        assert!(result.is_ok()); // Should succeed with mock value
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(42)); // Expected default value
        }
    }

    #[test]
    fn test_eval_mixed_type_operations() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test mixing numbers in comparisons
        let result = evaluator.eval("(10 + 5) == 15", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        // Test boolean in arithmetic context (through logical operations)
        let result = evaluator.eval("!(10 == 5)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }

        // Test complex mixed expression
        let result = evaluator.eval("(2 * 3 > 5) && (10 / 2 == 5)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(b)) = result {
            assert!(b);
        }
    }

    #[test]
    fn test_eval_type_casting() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test mixing numbers in comparisons
        let result = evaluator.eval("address(0x0)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Address(addr)) = result {
            assert_eq!(addr, address!("0x0000000000000000000000000000000000000000"));
        }
    }

    #[test]
    fn test_eval_parentheses_precedence() {
        let evaluator = ExpressionEvaluator::new_default();

        // Test nested parentheses
        let result = evaluator.eval("((2 + 3) * (4 - 1))", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(15)); // (5) * (3) = 15
        }

        // Test multiple levels
        let result = evaluator.eval("(((10)))", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(10));
        }

        // Test mixing with unary operators
        let result = evaluator.eval("-(2 + 3)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Int(val, _)) = result {
            assert_eq!(val, I256::from_raw(U256::from(5)).wrapping_neg());
        }
    }

    #[test]
    fn test_advanced_simulation_scenarios() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Test ERC20 token simulation
        debug_handler.set_variable("token", DynSolValue::Address(Address::from([0xAB; 20])));
        debug_handler.set_function("balanceOf", DynSolValue::Uint(U256::from(50000), 256));
        debug_handler.set_function("totalSupply", DynSolValue::Uint(U256::from(1000000), 256));

        // Test token balance query
        let result = evaluator.eval("balanceOf()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(50000));
        }

        // Test complex DeFi calculations
        let result = evaluator.eval("(balanceOf() * 100) / totalSupply()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(5)); // 50000 * 100 / 1000000 = 5
        }

        // Verify comprehensive logging
        let log = debug_handler.get_log();
        assert!(log.iter().any(|entry| entry.contains("balanceOf")));
        assert!(log.iter().any(|entry| entry.contains("totalSupply")));
    }

    #[test]
    fn test_blockchain_context_simulation() {
        let (handlers, _debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Test blockchain context access
        let result = evaluator.eval("msg.sender", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Address(addr)) = result {
            assert_eq!(addr, Address::from([0x42; 20]));
        }

        let result = evaluator.eval("msg.value", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(1000000000000000000u64)); // 1 ETH
        }

        let result = evaluator.eval("block.number", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(18500000));
        }

        let result = evaluator.eval("block.timestamp", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(1700000000));
        }

        let result = evaluator.eval("tx.origin", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Address(addr)) = result {
            assert_eq!(addr, Address::from([0x11; 20]));
        }

        // Test time-based calculations
        let result = evaluator.eval("block.timestamp + 3600", 0); // Add 1 hour
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(1700003600));
        }
    }

    #[test]
    fn test_complex_mapping_and_array_operations() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Test array access with dynamic values
        debug_handler.set_variable("userIndex", DynSolValue::Uint(U256::from(5), 256));

        // Test nested data structure access
        let result = evaluator.eval("users[userIndex].balance", 0);
        assert!(result.is_ok());

        // Test mapping access with address keys
        let result = evaluator.eval("balances[msg.sender]", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(1000000)); // Default mapping value for address
        }

        // Test complex nested access
        let result = evaluator.eval("tokenData[token].holders[owner].amount", 0);
        assert!(result.is_ok());

        // Verify mapping/array operations are logged
        let log = debug_handler.get_log();
        assert!(log.iter().any(|entry| entry.contains("get_mapping_or_array_value")));
        assert!(log.iter().any(|entry| entry.contains("access_member")));
    }

    #[test]
    fn test_advanced_function_call_scenarios() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Set up complex function return values
        debug_handler.set_function(
            "getReserves",
            DynSolValue::Tuple(vec![
                DynSolValue::Uint(U256::from(1000000), 256),
                DynSolValue::Uint(U256::from(500000), 256),
                DynSolValue::Uint(U256::from(1700000000), 256),
            ]),
        );

        // Test function calls with different argument patterns
        let result = evaluator.eval("getReserves()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Tuple(values)) = result {
            assert_eq!(values.len(), 3);
        }

        // Test standard ERC20 functions
        let result = evaluator.eval("name()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::String(name)) = result {
            assert_eq!(name, "MockToken");
        }

        let result = evaluator.eval("symbol()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::String(symbol)) = result {
            assert_eq!(symbol, "MTK");
        }

        let result = evaluator.eval("decimals()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(decimals, _)) = result {
            assert_eq!(decimals, U256::from(18));
        }

        // Test boolean return functions
        let result = evaluator.eval("approve()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(success)) = result {
            assert!(success);
        }
    }

    #[test]
    fn test_cross_handler_interactions() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Test expression combining multiple handler types
        let result = evaluator.eval("(msg.value * balanceOf()) / totalSupply()", 0);
        assert!(result.is_ok());

        // Test complex conditional-like expressions
        let result = evaluator.eval("msg.value > 0 && balanceOf() > 1000", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_valid)) = result {
            assert!(is_valid);
        }

        // Test address comparisons
        let result = evaluator.eval("msg.sender == tx.origin", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_same)) = result {
            assert!(!is_same); // Different mock addresses
        }

        // Test numerical operations with blockchain context
        let result = evaluator.eval("block.number % 100", 0);
        assert!(result.is_ok());

        // Verify all handler types were used
        let log = debug_handler.get_log();
        assert!(log.iter().any(|entry| entry.contains("get_msg_")));
        assert!(log.iter().any(|entry| entry.contains("get_tx_")));
        assert!(log.iter().any(|entry| entry.contains("get_block_")));
        assert!(log.iter().any(|entry| entry.contains("call_function")));
    }

    #[test]
    fn test_error_handling_and_recovery() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Test invalid operations that should fail gracefully
        let result = evaluator.eval("1 / 0", 0);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(error_msg.contains("division") && error_msg.contains("zero"));

        // Test type mismatches
        let result = evaluator.eval("true + 5", 0);
        assert!(result.is_err());

        // Test successful operations after errors (handler state preserved)
        let result = evaluator.eval("msg.sender", 0);
        assert!(result.is_ok());

        // Verify error cases are logged appropriately
        let log = debug_handler.get_log();
        assert!(log.iter().any(|entry| entry.contains("get_msg_sender")));
    }

    #[test]
    fn test_performance_and_logging_metrics() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Clear initial logs
        debug_handler.clear_log();

        // Execute a complex expression
        let result = evaluator
            .eval("((balanceOf() * msg.value) / totalSupply()) > (block.timestamp % 1000)", 0);
        assert!(result.is_ok());

        // Analyze logging output
        let log = debug_handler.get_log();
        assert!(!log.is_empty());

        // Verify specific operations were logged
        let function_calls = log.iter().filter(|entry| entry.contains("call_function")).count();
        let msg_accesses = log.iter().filter(|entry| entry.contains("get_msg_")).count();
        let block_accesses = log.iter().filter(|entry| entry.contains("get_block_")).count();

        assert!(function_calls >= 2); // balanceOf, totalSupply
        assert!(msg_accesses >= 1); // msg.value
        assert!(block_accesses >= 1); // block.timestamp

        // Test log clearing functionality
        debug_handler.clear_log();
        let cleared_log = debug_handler.get_log();
        assert!(cleared_log.is_empty());
    }

    #[test]
    fn test_dynamic_value_simulation() {
        let (handlers, _debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Test different value types based on naming conventions that match the debug handler
        // patterns exactly The debug handler looks for exact word matches with .contains()

        // Test balance/amount/value types -> Uint (with specific values)
        let balance_result = evaluator.eval("balance", 0);
        if let Ok(DynSolValue::Uint(val, _)) = balance_result {
            assert_eq!(val, U256::from(1000000)); // 1M as default balance
        } else {
            panic!("Expected balance to return Uint(1000000, 256)");
        }

        let amount_result = evaluator.eval("amount", 0);
        if let Ok(DynSolValue::Uint(val, _)) = amount_result {
            assert_eq!(val, U256::from(1000000)); // Same as balance/amount/value
        } else {
            panic!("Expected amount to return Uint(1000000, 256)");
        }

        let value_result = evaluator.eval("value", 0);
        if let Ok(DynSolValue::Uint(val, _)) = value_result {
            assert_eq!(val, U256::from(1000000)); // Same as balance/amount/value
        } else {
            panic!("Expected value to return Uint(1000000, 256)");
        }

        // Test address/owner/sender types -> Address (using variable names that contain the
        // keywords)
        let address_result = evaluator.eval("myaddress", 0);
        assert!(matches!(address_result.unwrap(), DynSolValue::Address(_)));

        let owner_result = evaluator.eval("owner", 0);
        assert!(matches!(owner_result.unwrap(), DynSolValue::Address(_)));

        let sender_result = evaluator.eval("sender", 0);
        assert!(matches!(sender_result.unwrap(), DynSolValue::Address(_)));

        // Test count/length/index types -> Uint (with value 5)
        let count_result = evaluator.eval("count", 0);
        if let Ok(DynSolValue::Uint(val, _)) = count_result {
            assert_eq!(val, U256::from(5)); // Default count/length
        } else {
            panic!("Expected count to return Uint(5, 256)");
        }

        let length_result = evaluator.eval("length", 0);
        if let Ok(DynSolValue::Uint(val, _)) = length_result {
            assert_eq!(val, U256::from(5)); // Default count/length
        } else {
            panic!("Expected length to return Uint(5, 256)");
        }

        let index_result = evaluator.eval("index", 0);
        if let Ok(DynSolValue::Uint(val, _)) = index_result {
            assert_eq!(val, U256::from(5)); // Default count/length/index
        } else {
            panic!("Expected index to return Uint(5, 256)");
        }

        // Test enabled/active/flag types -> Bool
        let enabled_result = evaluator.eval("enabled", 0);
        if let Ok(DynSolValue::Bool(val)) = enabled_result {
            assert!(val); // Default boolean
        } else {
            panic!("Expected enabled to return Bool(true)");
        }

        let active_result = evaluator.eval("active", 0);
        if let Ok(DynSolValue::Bool(val)) = active_result {
            assert!(val); // Default boolean
        } else {
            panic!("Expected active to return Bool(true)");
        }

        let flag_result = evaluator.eval("flag", 0);
        if let Ok(DynSolValue::Bool(val)) = flag_result {
            assert!(val); // Default boolean
        } else {
            panic!("Expected flag to return Bool(true)");
        }

        // Test name/symbol/uri types -> String
        let name_result = evaluator.eval("name", 0);
        if let Ok(DynSolValue::String(val)) = name_result {
            assert_eq!(val, "Mock_name"); // Mock string format
        } else {
            panic!("Expected name to return String");
        }

        let symbol_result = evaluator.eval("symbol", 0);
        if let Ok(DynSolValue::String(val)) = symbol_result {
            assert_eq!(val, "Mock_symbol"); // Mock string format
        } else {
            panic!("Expected symbol to return String");
        }

        let uri_result = evaluator.eval("uri", 0);
        if let Ok(DynSolValue::String(val)) = uri_result {
            assert_eq!(val, "Mock_uri"); // Mock string format
        } else {
            panic!("Expected uri to return String");
        }

        // Test fallback case -> Uint(42, 256)
        let fallback_result = evaluator.eval("randomVariable", 0);
        if let Ok(DynSolValue::Uint(val, _)) = fallback_result {
            assert_eq!(val, U256::from(42));
        } else {
            panic!("Expected fallback to return Uint(42, 256)");
        }
    }

    #[test]
    fn test_enhanced_builtin_properties() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Set up test data
        debug_handler.set_variable("testString", DynSolValue::String("Hello".to_string()));
        debug_handler.set_variable("testBytes", DynSolValue::Bytes(vec![1, 2, 3]));
        debug_handler.set_variable("testAddress", DynSolValue::Address(Address::ZERO));
        debug_handler.set_variable(
            "testInt",
            DynSolValue::Int(I256::from_raw(U256::from(42).wrapping_neg()), 256),
        );
        debug_handler.set_variable("zeroInt", DynSolValue::Int(I256::ZERO, 256));
        debug_handler.set_variable("zeroUint", DynSolValue::Uint(U256::ZERO, 256));
        debug_handler.set_variable("nonZeroUint", DynSolValue::Uint(U256::from(123), 256));

        // ============ LENGTH PROPERTIES ============

        // Test string.length
        let result = evaluator.eval("testString.length", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(len, _)) = result {
            assert_eq!(len, U256::from(5)); // "Hello" length
        }

        // Test bytes.length
        let result = evaluator.eval("testBytes.length", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(len, _)) = result {
            assert_eq!(len, U256::from(3));
        }

        // ============ NUMERIC PROPERTIES ============

        // Test int.abs (negative value)
        let result = evaluator.eval("testInt.abs", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(val, _)) = result {
            assert_eq!(val, U256::from(42));
        }

        // ============ TYPE CHECKING PROPERTIES ============

        // Test isZero for zero values
        let result = evaluator.eval("zeroInt.isZero", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_zero)) = result {
            assert!(is_zero);
        }

        let result = evaluator.eval("zeroUint.isZero", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_zero)) = result {
            assert!(is_zero);
        }

        let result = evaluator.eval("testAddress.isZero", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_zero)) = result {
            assert!(is_zero); // Address::ZERO
        }

        // Test isZero for non-zero values
        let result = evaluator.eval("nonZeroUint.isZero", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_zero)) = result {
            assert!(!is_zero);
        }
    }

    #[test]
    fn test_builtin_member_functions() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Set up test data
        debug_handler.set_variable("testString", DynSolValue::String("Hello World".to_string()));
        debug_handler.set_variable("emptyString", DynSolValue::String(String::new()));
        debug_handler.set_variable("myBytes", DynSolValue::Bytes(vec![1, 2, 3, 4, 5]));
        debug_handler.set_variable("emptyBytes", DynSolValue::Bytes(vec![]));
        debug_handler.set_variable(
            "myArray",
            DynSolValue::Tuple(vec![
                DynSolValue::Uint(U256::from(10), 256),
                DynSolValue::Uint(U256::from(20), 256),
                DynSolValue::Uint(U256::from(30), 256),
            ]),
        );
        debug_handler.set_variable("emptyArray", DynSolValue::Tuple(vec![]));
        debug_handler.set_variable("testAddress", DynSolValue::Address(Address::from([0x42; 20])));
        debug_handler.set_variable("numberA", DynSolValue::Uint(U256::from(100), 256));
        debug_handler.set_variable("numberB", DynSolValue::Uint(U256::from(200), 256));

        // ============ LENGTH PROPERTIES ============

        // Test string.length
        let result = evaluator.eval("testString.length", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(len, _)) = result {
            assert_eq!(len, U256::from(11)); // "Hello World" length
        }

        // Test bytes.length
        let result = evaluator.eval("myBytes.length", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(len, _)) = result {
            assert_eq!(len, U256::from(5));
        }

        // Test array.length
        let result = evaluator.eval("myArray.length", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(len, _)) = result {
            assert_eq!(len, U256::from(3));
        }

        // ============ ARRAY/LIST METHODS ============

        // Test array.push() - empty push
        let result = evaluator.eval("myArray.push()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(new_len, _)) = result {
            assert_eq!(new_len, U256::from(4)); // Original 3 + 1
        }

        // Test array.push(element)
        debug_handler.set_variable("newElement", DynSolValue::Uint(U256::from(40), 256));
        let result = evaluator.eval("myArray.push(newElement)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(new_len, _)) = result {
            assert_eq!(new_len, U256::from(4)); // Original 3 + 1
        }

        // Test array.pop()
        let result = evaluator.eval("myArray.pop()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(popped_val, _)) = result {
            assert_eq!(popped_val, U256::from(30)); // Last element of myArray
        }

        // Test pop on empty array (should fail)
        let result = evaluator.eval("emptyArray.pop()", 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cannot pop from empty array"));

        // ============ STRING METHODS ============

        // Test string.concat()
        debug_handler.set_variable("suffix", DynSolValue::String(" Test".to_string()));
        let result = evaluator.eval("testString.concat(suffix)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::String(concatenated)) = result {
            assert_eq!(concatenated, "Hello World Test");
        }

        // Test string.slice(start)
        debug_handler.set_variable("startIndex", DynSolValue::Uint(U256::from(6), 256));
        let result = evaluator.eval("testString.slice(startIndex)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::String(sliced)) = result {
            assert_eq!(sliced, "World");
        }

        // Test string.slice(start, end)
        debug_handler.set_variable("endIndex", DynSolValue::Uint(U256::from(5), 256));
        let result = evaluator.eval("testString.slice(0, endIndex)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::String(sliced)) = result {
            assert_eq!(sliced, "Hello");
        }

        // ============ BYTES METHODS ============

        // Test bytes.concat()
        debug_handler.set_variable("moreBytes", DynSolValue::Bytes(vec![6, 7, 8]));
        let result = evaluator.eval("myBytes.concat(moreBytes)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bytes(concatenated)) = result {
            assert_eq!(concatenated, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        }

        // Test bytes.slice(start)
        let result = evaluator.eval("myBytes.slice(2)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bytes(sliced)) = result {
            assert_eq!(sliced, vec![3, 4, 5]);
        }

        // Test bytes.slice(start, end)
        let result = evaluator.eval("myBytes.slice(1, 4)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bytes(sliced)) = result {
            assert_eq!(sliced, vec![2, 3, 4]);
        }

        // ============ ADDRESS METHODS ============
        // Note: Address properties like balance, code, codehash, isContract should be
        // handled by handlers since they require blockchain state access

        // ============ MATH FUNCTIONS ============

        // Test number.min()
        let result = evaluator.eval("numberA.min(numberB)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(min_val, _)) = result {
            assert_eq!(min_val, U256::from(100));
        }

        // Test number.max()
        let result = evaluator.eval("numberA.max(numberB)", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Uint(max_val, _)) = result {
            assert_eq!(max_val, U256::from(200));
        }

        // ============ TYPE CHECKING FUNCTIONS ============

        // Test string.isEmpty()
        let result = evaluator.eval("testString.isEmpty()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_empty)) = result {
            assert!(!is_empty);
        }

        let result = evaluator.eval("emptyString.isEmpty()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_empty)) = result {
            assert!(is_empty);
        }

        // Test bytes.isEmpty()
        let result = evaluator.eval("myBytes.isEmpty()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_empty)) = result {
            assert!(!is_empty);
        }

        let result = evaluator.eval("emptyBytes.isEmpty()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_empty)) = result {
            assert!(is_empty);
        }

        // Test array.isEmpty()
        let result = evaluator.eval("myArray.isEmpty()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_empty)) = result {
            assert!(!is_empty);
        }

        let result = evaluator.eval("emptyArray.isEmpty()", 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_empty)) = result {
            assert!(is_empty);
        }
    }

    #[test]
    fn test_extremely_complex_expressions() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Set up complex mock data for realistic DeFi/protocol scenarios
        debug_handler.set_function(
            "getPoolData",
            DynSolValue::Tuple(vec![
                DynSolValue::Uint(U256::from(1000000), 256), // reserve0
                DynSolValue::Uint(U256::from(2000000), 256), // reserve1
                DynSolValue::Uint(U256::from(1700000000), 256), // lastUpdate
            ]),
        );

        debug_handler.set_function("calculateFee", DynSolValue::Uint(U256::from(3000), 256)); // 0.3% fee
        debug_handler.set_function("getPrice", DynSolValue::Uint(U256::from(2000), 256)); // Price in USD
        debug_handler.set_function("getUserBalance", DynSolValue::Uint(U256::from(50000), 256));
        debug_handler.set_function("getSlippageTolerance", DynSolValue::Uint(U256::from(100), 256)); // 1%

        // Complex Expression 1: Multi-step DeFi liquidity calculation with slippage protection
        let complex_expr1 = "((getUserBalance() * getPrice()) / 1000000) > ((calculateFee() * msg.value * getSlippageTolerance()) / (10000 * 100)) && (block.timestamp - 1700000000) < 3600 && msg.sender != tx.origin";

        let result = evaluator.eval(complex_expr1, 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(is_valid)) = result {
            // This complex condition should evaluate based on our mock values
            println!("Complex DeFi condition result: {is_valid}");
        }

        // Complex Expression 2: Nested conditional with multiple function calls and arithmetic
        let complex_expr2 = "(((balanceOf() + getUserBalance()) * (getPrice() - calculateFee())) / totalSupply()) > (msg.value * ((block.number % 100) + 1)) ? ((getSlippageTolerance() * 2) + (block.timestamp % 1000)) : ((calculateFee() * 3) - (msg.value / 1000))";

        let result = evaluator.eval(complex_expr2, 0);
        assert!(result.is_ok());
        println!("Complex ternary expression result: {result:?}");

        // Complex Expression 3: Advanced protocol governance voting calculation
        let complex_expr3 = "((balanceOf() * (block.number - 18500000)) + (getUserBalance() * getSlippageTolerance())) >= ((totalSupply() / 100) * ((msg.value > 1000000000000000000) ? (calculateFee() + 500) : (calculateFee() - 200))) && (block.timestamp > 1700000000) && ((tx.origin == msg.sender) || (getPrice() > 1800))";

        let result = evaluator.eval(complex_expr3, 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(governance_valid)) = result {
            println!("Complex governance voting condition: {governance_valid}");
        }

        // Verify all operations were logged
        let log = debug_handler.get_log();
        assert!(!log.is_empty());

        // Should contain multiple function calls and complex operations
        let function_calls = log.iter().filter(|entry| entry.contains("call_function")).count();
        assert!(function_calls >= 10, "Should have many function calls in complex expressions");
    }

    #[test]
    fn test_ultra_complex_nested_expressions() {
        let (handlers, debug_handler) = create_simulation_debug_handlers();
        let evaluator = ExpressionEvaluator::new(handlers);

        // Set up extremely complex scenario data
        debug_handler.set_function(
            "getLiquidityData",
            DynSolValue::Tuple(vec![
                DynSolValue::Uint(U256::from(5000000), 256),
                DynSolValue::Uint(U256::from(3000000), 256),
                DynSolValue::Uint(U256::from(8000000), 256),
            ]),
        );

        debug_handler.set_function("calculateRewards", DynSolValue::Uint(U256::from(12500), 256));
        debug_handler.set_function("getMultiplier", DynSolValue::Uint(U256::from(150), 256)); // 1.5x
        debug_handler.set_function("getRiskFactor", DynSolValue::Uint(U256::from(80), 256)); // 0.8x
        debug_handler.set_function("getTimeDecay", DynSolValue::Uint(U256::from(95), 256)); // 0.95x

        // Ultra Complex Expression: Multi-layered yield farming calculation with time decay and
        // risk adjustment
        let ultra_complex = r#"
            (
                (
                    (
                        (balanceOf() * getMultiplier() * getRiskFactor()) / (100 * 100)
                    ) +
                    (
                        (calculateRewards() * getTimeDecay() * ((block.timestamp - 1700000000) / 86400)) / (100 * 365)
                    )
                ) *
                (
                    (msg.value > (totalSupply() / 1000)) ?
                    (
                        ((getPrice() + calculateFee()) * (block.number % 1000)) / 500
                    ) :
                    (
                        ((getPrice() - calculateFee()) * (block.timestamp % 10000)) / 2000
                    )
                )
            ) >=
            (
                (
                    (getUserBalance() * (100 + getSlippageTolerance())) / 100
                ) +
                (
                    ((msg.sender == tx.origin) ? (calculateRewards() * 2) : (calculateRewards() / 2)) *
                    ((block.number > 18500000) ? getMultiplier() : (getMultiplier() / 2))
                ) / 100
            )
        "#.replace(['\n', ' '], "");

        let result = evaluator.eval(&ultra_complex, 0);
        assert!(result.is_ok());
        if let Ok(DynSolValue::Bool(result_bool)) = result {
            println!("Ultra complex yield farming condition: {result_bool}");
        }

        // Verify extensive logging occurred
        let log = debug_handler.get_log();
        let total_operations = log.len();
        assert!(
            total_operations >= 20,
            "Ultra complex expression should generate many log entries, got: {total_operations}"
        );

        println!("Total operations logged: {total_operations}");
        println!("Sample log entries: {:?}", log.iter().take(5).collect::<Vec<_>>());
    }
}
