use std::{
    collections::BTreeMap,
    fmt::{self, Display},
};

use foundry_compilers::artifacts::{
    ContractDefinition, ContractKind, FunctionDefinition, InlineAssembly, ModifierDefinition,
    ParameterList, StateMutability,
};

use crate::RuntimeAddress;

use super::{ast_visitor::Walk, source_map::debug_unit::DebugUnit};

/// Metadata for Function Unit.
#[derive(Clone, Debug)]
pub struct FunctionInfo {
    pub is_modifier: bool,
    pub is_virtual: bool,
    pub name: String,
    pub state_mutability: Option<StateMutability>,
    pub parameters: ParameterList,
    pub return_parameters: Option<ParameterList>,
}

impl From<&FunctionDefinition> for FunctionInfo {
    fn from(func: &FunctionDefinition) -> Self {
        Self {
            is_modifier: false,
            is_virtual: func.is_virtual,
            name: func.name.clone(),
            state_mutability: func.state_mutability.clone(),
            parameters: func.parameters.clone(),
            return_parameters: Some(func.return_parameters.clone()),
        }
    }
}

impl From<&ModifierDefinition> for FunctionInfo {
    fn from(modifier: &ModifierDefinition) -> Self {
        Self {
            is_modifier: true,
            is_virtual: false,
            name: modifier.name.clone(),
            state_mutability: None,
            parameters: modifier.parameters.clone(),
            return_parameters: None,
        }
    }
}

impl FunctionInfo {
    pub fn is_pure(&self) -> bool {
        matches!(self.state_mutability, Some(StateMutability::Pure))
    }
}

impl Display for FunctionInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_modifier {
            write!(f, "modifier {}", self.name)
        } else {
            let mutability = match &self.state_mutability {
                Some(StateMutability::Pure) => "pure",
                Some(StateMutability::View) => "view",
                Some(StateMutability::Nonpayable) => "nonpayable",
                Some(StateMutability::Payable) => "payable",
                None => "",
            };
            if self.is_virtual {
                write!(f, "function {}(..) {} virtual", self.name, mutability)
            } else {
                write!(f, "function {}(..) {}", self.name, mutability)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ContractInfo {
    pub name: String,
    pub kind: ContractKind,
}

impl From<&ContractDefinition> for ContractInfo {
    fn from(contract: &ContractDefinition) -> Self {
        Self { name: contract.name.clone(), kind: contract.kind.clone() }
    }
}

impl Display for ContractInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            ContractKind::Contract => "contract",
            ContractKind::Library => "library",
            ContractKind::Interface => "interface",
        };

        write!(f, "{} {}", kind, self.name,)
    }
}

#[derive(Clone, Debug, Default)]
pub struct StatementInfo {
    pub inner_func_call: Vec<FunctionCallInfo>,
}

impl Display for StatementInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.inner_func_call.is_empty() {
            write!(f, "no function call")
        } else {
            // Print all the function calls in the statement, separated by comma.
            write!(
                f,
                "function calls: {}",
                self.inner_func_call
                    .iter()
                    .map(|meta| meta.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

// To avoid extensive memory usage, all metadata should be stored in Arc.
#[derive(Clone, Debug)]
pub struct FunctionCallInfo {
    pub is_constructor: bool,

    // The entire function call expression.
    pub expr: String,

    // The expression part of the function name.
    pub name: String,

    // The number of arguments in the function call.
    pub arg_n: usize,
}

impl Display for FunctionCallInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_constructor {
            write!(f, "new {}", self.name)
        } else {
            write!(f, "{}", self.name)
        }
    }
}

#[derive(Clone, Debug)]
pub struct InlineAssemblyInfo {}

impl Display for InlineAssemblyInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "inline assembly")
    }
}

#[derive(Debug, Clone, Default)]
pub struct CallGraph {
    pub addr: RuntimeAddress,
    pub functions: BTreeMap<DebugUnit, FunctionInfo>,
    pub contracts: BTreeMap<DebugUnit, ContractInfo>,
    pub statements: BTreeMap<DebugUnit, StatementInfo>,
    pub inline_assembly: BTreeMap<DebugUnit, InlineAssemblyInfo>,
}

impl CallGraph {
    pub fn get_function_info(&self, unit: &DebugUnit) -> Option<&FunctionInfo> {
        self.functions.get(unit)
    }

    pub fn get_contract_info(&self, unit: &DebugUnit) -> Option<&ContractInfo> {
        self.contracts.get(unit)
    }

    pub fn get_statement_info(&self, unit: &DebugUnit) -> Option<&StatementInfo> {
        self.statements.get(unit)
    }

    pub fn get_inline_assembly_info(&self, unit: &DebugUnit) -> Option<&InlineAssemblyInfo> {
        self.inline_assembly.get(unit)
    }
}

#[derive(Debug, Clone, Default)]
pub struct CallGraphAnalysis {
    pub addr: RuntimeAddress,
    pub functions: BTreeMap<DebugUnit, FunctionInfo>,
    pub contracts: BTreeMap<DebugUnit, ContractInfo>,
    pub statements: BTreeMap<DebugUnit, StatementInfo>,
    pub inline_assembly: BTreeMap<DebugUnit, InlineAssemblyInfo>,
}

impl CallGraphAnalysis {
    pub fn new(addr: RuntimeAddress) -> Self {
        Self { addr, ..Default::default() }
    }

    pub fn produce(self) -> CallGraph {
        CallGraph {
            addr: self.addr,
            functions: self.functions,
            contracts: self.contracts,
            statements: self.statements,
            inline_assembly: self.inline_assembly,
        }
    }

    pub fn register_contract(&mut self, unit: DebugUnit, def: &ContractDefinition) {
        todo!()
    }

    pub fn register_function(&mut self, unit: DebugUnit, def: &dyn Walk) {
        todo!()
    }

    pub fn register_statement(&mut self, unit: DebugUnit, def: &dyn Walk) {
        todo!()
    }

    pub fn register_primitive_statement(&mut self, unit: DebugUnit, def: &dyn Walk) {
        todo!()
    }

    pub fn register_inline_assembly(&mut self, unit: DebugUnit, def: &InlineAssembly) {
        todo!()
    }
}

// let meta = if let Some(node) = node {
//     let mut visitor = StatementVisitor::new(self);
//     node.walk(&mut visitor)?;
//     visitor.produce()
// } else {
//     StatementMeta::default()
// };

// #[derive(Clone, Debug)]
// struct StatementVisitor<'a, 'b> {
//     func_calls: Vec<FunctionCallMeta>,
//     debug_unit_visitor: &'a DebugUnitVisitor<'b>,
// }
//
// impl<'a, 'b> Visitor for StatementVisitor<'a, 'b> {
//     fn visit_statement(&mut self, _statement: &Statement) -> Result<()> {
//         ensure!(self.func_calls.is_empty(), "statement debug units should not nested");
//
//         Ok(())
//     }
//
//     fn visit_function_call(&mut self, function_call: &FunctionCall) -> Result<()> {
//         if function_call.kind != FunctionCallKind::FunctionCall {
//             return Ok(());
//         }
//
//         if let Some(mut meta) = self.collect_function_call(&function_call.expression)? {
//             trace!(arg_n = function_call.arguments.len(), "find a function call: {meta}");
//             meta.arg_n = function_call.arguments.len();
//             self.func_calls.push(meta);
//         }
//
//         Ok(())
//     }
// }
//
// impl<'a, 'b> StatementVisitor<'a, 'b>
// where
//     'b: 'a,
// {
//     pub fn new(debug_unit_visitor: &'a DebugUnitVisitor<'b>) -> Self {
//         Self { func_calls: Vec::new(), debug_unit_visitor }
//     }
//
//     pub fn produce(self) -> StatementMeta {
//         StatementMeta { inner_func_call: self.func_calls }
//     }
//
//     fn collect_function_call(
//         &mut self,
//         call_expr: &Expression,
//     ) -> Result<Option<FunctionCallMeta>> {
//         let unit = self
//             .debug_unit_visitor
//             .get_unit_location(get_source_location_for_expression(call_expr))?;
//         let unit_s = unit.as_str();
//
//         // We will ignore the function call to the ABI or the new operator, since they are not
//         // actual function calls.
//         if unit_s.starts_with("abi.") {
//             return Ok(None);
//         }
//
//         match call_expr {
//             Expression::Identifier(ref ident) => {
//                 if ident.name.as_str() == "require" || ident.name.as_str() == "keccak256" {
//                     Ok(None)
//                 } else {
//                     Ok(Some(FunctionCallMeta {
//                         is_constructor: false,
//                         name: ident.name.clone(),
//                         expr: unit.as_str().to_string(),
//                         arg_n: 0, // Placeholder for the number of arguments.
//                     }))
//                 }
//             }
//             Expression::MemberAccess(ref member) => {
//                 if unit_s.starts_with("super") {
//                     debug!("WTF {:?} {}", member.expression, unit_s);
//                 }
//                 Ok(Some(FunctionCallMeta {
//                     is_constructor: false,
//                     name: member.member_name.clone(),
//                     expr: unit.as_str().to_string(),
//                     arg_n: 0, // Placeholder for the number of arguments.
//                 }))
//             }
//             Expression::FunctionCallOptions(ref opts) => {
//                 if let Some(mut meta) = self.collect_function_call(&opts.expression)? {
//                     meta.expr = unit.as_str().to_string();
//                     Ok(Some(meta))
//                 } else {
//                     Ok(None)
//                 }
//             }
//             Expression::FunctionCall(ref call) => {
//                 // It is possible for `stakingRouter.deposit.value(depositsValue)(...)`
//                 match &call.expression {
//                     Expression::MemberAccess(ref member) => {
//                         self.collect_function_call(&member.expression)
//                     }
//                     _ => {
//                         bail!("invalid Expression::FunctionCall {unit_s} {:?}", call.expression);
//                     }
//                 }
//             }
//             Expression::NewExpression(ref new) => match &new.type_name {
//                 TypeName::UserDefinedTypeName(ref user) => {
//                     let type_str = user.type_descriptions.type_string.as_ref().ok_or_eyre(
//                         format!("invalid Expression::NewExpression {unit_s} {:?}",
// new.type_name),                     )?;
//                     if let Some(contract_str) = type_str.strip_prefix("contract ") {
//                         Ok(Some(FunctionCallMeta {
//                             is_constructor: true,
//                             name: contract_str.to_string(),
//                             expr: unit.as_str().to_string(),
//                             arg_n: 0, // Placeholder for the number of arguments.
//                         }))
//                     } else {
//                         bail!("invalid Expression::NewExpression {unit_s} {:?}", new.type_name);
//                     }
//                 }
//                 _ => Ok(None),
//             },
//             Expression::TupleExpression(ref tuple) => {
//                 // e.g., ProxyAdmin adminInstance = (new ProxyAdmin){salt: adminSalt}()
//                 match &tuple.components[..] {
//                     [Some(Expression::NewExpression(_))] => self.collect_function_call(
//                         tuple.components[0].as_ref().expect("this should not happen"),
//                     ),
//                     _ => {
//                         bail!(
//                             "invalid Expression::TupleExpression {unit_s} {:?}",
//                             tuple.components
//                         );
//                     }
//                 }
//             }
//             _ => {
//                 bail!("invalid function call expression type {unit_s} {:?}", call_expr);
//             }
//         }
//     }
// }
