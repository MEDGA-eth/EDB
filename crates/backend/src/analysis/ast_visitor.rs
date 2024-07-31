use eyre::Result;
use foundry_compilers::artifacts::*;
use paste::paste;

pub trait Visitor {
    fn visit_source_unit(&mut self, _source_unit: &SourceUnit) -> Result<()> {
        Ok(())
    }
    fn visit_import_directive(&mut self, _directive: &ImportDirective) -> Result<()> {
        Ok(())
    }
    fn visit_pragma_directive(&mut self, _directive: &PragmaDirective) -> Result<()> {
        Ok(())
    }
    fn visit_block(&mut self, _block: &Block) -> Result<()> {
        Ok(())
    }
    fn visit_statement(&mut self, _statement: &Statement) -> Result<()> {
        Ok(())
    }
    fn visit_expression(&mut self, _expression: &Expression) -> Result<()> {
        Ok(())
    }
    fn visit_function_call(&mut self, _function_call: &FunctionCall) -> Result<()> {
        Ok(())
    }
    fn visit_user_defined_type_name(&mut self, _type_name: &UserDefinedTypeName) -> Result<()> {
        Ok(())
    }
    fn visit_identifier_path(&mut self, _identifier_path: &IdentifierPath) -> Result<()> {
        Ok(())
    }
    fn visit_type_name(&mut self, _type_name: &TypeName) -> Result<()> {
        Ok(())
    }
    fn visit_parameter_list(&mut self, _parameter_list: &ParameterList) -> Result<()> {
        Ok(())
    }
    fn visit_function_definition(&mut self, _definition: &FunctionDefinition) -> Result<()> {
        Ok(())
    }
    fn visit_enum_definition(&mut self, _definition: &EnumDefinition) -> Result<()> {
        Ok(())
    }
    fn visit_error_definition(&mut self, _definition: &ErrorDefinition) -> Result<()> {
        Ok(())
    }
    fn visit_event_definition(&mut self, _definition: &EventDefinition) -> Result<()> {
        Ok(())
    }
    fn visit_struct_definition(&mut self, _definition: &StructDefinition) -> Result<()> {
        Ok(())
    }
    fn visit_modifier_definition(&mut self, _definition: &ModifierDefinition) -> Result<()> {
        Ok(())
    }
    fn visit_variable_declaration(&mut self, _declaration: &VariableDeclaration) -> Result<()> {
        Ok(())
    }
    fn visit_overrides(&mut self, _specifier: &OverrideSpecifier) -> Result<()> {
        Ok(())
    }
    fn visit_user_defined_value_type(
        &mut self,
        _value_type: &UserDefinedValueTypeDefinition,
    ) -> Result<()> {
        Ok(())
    }
    fn visit_contract_definition(&mut self, _definition: &ContractDefinition) -> Result<()> {
        Ok(())
    }
    fn visit_using_for(&mut self, _directive: &UsingForDirective) -> Result<()> {
        Ok(())
    }
    fn visit_unary_operation(&mut self, _unary_op: &UnaryOperation) -> Result<()> {
        Ok(())
    }
    fn visit_binary_operation(&mut self, _binary_op: &BinaryOperation) -> Result<()> {
        Ok(())
    }
    fn visit_conditional(&mut self, _conditional: &Conditional) -> Result<()> {
        Ok(())
    }
    fn visit_tuple_expression(&mut self, _tuple_expression: &TupleExpression) -> Result<()> {
        Ok(())
    }
    fn visit_new_expression(&mut self, _new_expression: &NewExpression) -> Result<()> {
        Ok(())
    }
    fn visit_assignment(&mut self, _assignment: &Assignment) -> Result<()> {
        Ok(())
    }
    fn visit_identifier(&mut self, _identifier: &Identifier) -> Result<()> {
        Ok(())
    }
    fn visit_index_access(&mut self, _index_access: &IndexAccess) -> Result<()> {
        Ok(())
    }
    fn visit_index_range_access(&mut self, _index_range_access: &IndexRangeAccess) -> Result<()> {
        Ok(())
    }
    fn visit_while_statement(&mut self, _while_statement: &WhileStatement) -> Result<()> {
        Ok(())
    }
    fn visit_for_statement(&mut self, _for_statement: &ForStatement) -> Result<()> {
        Ok(())
    }
    fn visit_if_statement(&mut self, _if_statement: &IfStatement) -> Result<()> {
        Ok(())
    }
    fn visit_do_while_statement(&mut self, _do_while_statement: &DoWhileStatement) -> Result<()> {
        Ok(())
    }
    fn visit_emit_statement(&mut self, _emit_statement: &EmitStatement) -> Result<()> {
        Ok(())
    }
    fn visit_unchecked_block(&mut self, _unchecked_block: &UncheckedBlock) -> Result<()> {
        Ok(())
    }
    fn visit_try_statement(&mut self, _try_statement: &TryStatement) -> Result<()> {
        Ok(())
    }
    fn visit_revert_statement(&mut self, _revert_statement: &RevertStatement) -> Result<()> {
        Ok(())
    }
    fn visit_member_access(&mut self, _member_access: &MemberAccess) -> Result<()> {
        Ok(())
    }
    fn visit_mapping(&mut self, _mapping: &Mapping) -> Result<()> {
        Ok(())
    }
    fn visit_elementary_type_name(
        &mut self,
        _elementary_type_name: &ElementaryTypeName,
    ) -> Result<()> {
        Ok(())
    }
    fn visit_literal(&mut self, _literal: &Literal) -> Result<()> {
        Ok(())
    }
    fn visit_function_type_name(&mut self, _function_type_name: &FunctionTypeName) -> Result<()> {
        Ok(())
    }
    fn visit_array_type_name(&mut self, _array_type_name: &ArrayTypeName) -> Result<()> {
        Ok(())
    }
    fn visit_function_call_options(&mut self, _function_call: &FunctionCallOptions) -> Result<()> {
        Ok(())
    }
    fn visit_return(&mut self, _return: &Return) -> Result<()> {
        Ok(())
    }
    fn visit_inheritance_specifier(&mut self, _specifier: &InheritanceSpecifier) -> Result<()> {
        Ok(())
    }
    fn visit_modifier_invocation(&mut self, _invocation: &ModifierInvocation) -> Result<()> {
        Ok(())
    }
    fn visit_inline_assembly(&mut self, _assembly: &InlineAssembly) -> Result<()> {
        Ok(())
    }
    fn visit_external_assembly_reference(
        &mut self,
        _ref: &ExternalInlineAssemblyReference,
    ) -> Result<()> {
        Ok(())
    }

    fn post_visit_source_unit(&mut self, _source_unit: &SourceUnit) -> Result<()> {
        Ok(())
    }
    fn post_visit_import_directive(&mut self, _directive: &ImportDirective) -> Result<()> {
        Ok(())
    }
    fn post_visit_pragma_directive(&mut self, _directive: &PragmaDirective) -> Result<()> {
        Ok(())
    }
    fn post_visit_block(&mut self, _block: &Block) -> Result<()> {
        Ok(())
    }
    fn post_visit_statement(&mut self, _statement: &Statement) -> Result<()> {
        Ok(())
    }
    fn post_visit_expression(&mut self, _expression: &Expression) -> Result<()> {
        Ok(())
    }
    fn post_visit_function_call(&mut self, _function_call: &FunctionCall) -> Result<()> {
        Ok(())
    }
    fn post_visit_user_defined_type_name(
        &mut self,
        _type_name: &UserDefinedTypeName,
    ) -> Result<()> {
        Ok(())
    }
    fn post_visit_identifier_path(&mut self, _identifier_path: &IdentifierPath) -> Result<()> {
        Ok(())
    }
    fn post_visit_type_name(&mut self, _type_name: &TypeName) -> Result<()> {
        Ok(())
    }
    fn post_visit_parameter_list(&mut self, _parameter_list: &ParameterList) -> Result<()> {
        Ok(())
    }
    fn post_visit_function_definition(&mut self, _definition: &FunctionDefinition) -> Result<()> {
        Ok(())
    }
    fn post_visit_enum_definition(&mut self, _definition: &EnumDefinition) -> Result<()> {
        Ok(())
    }
    fn post_visit_error_definition(&mut self, _definition: &ErrorDefinition) -> Result<()> {
        Ok(())
    }
    fn post_visit_event_definition(&mut self, _definition: &EventDefinition) -> Result<()> {
        Ok(())
    }
    fn post_visit_struct_definition(&mut self, _definition: &StructDefinition) -> Result<()> {
        Ok(())
    }
    fn post_visit_modifier_definition(&mut self, _definition: &ModifierDefinition) -> Result<()> {
        Ok(())
    }
    fn post_visit_variable_declaration(
        &mut self,
        _declaration: &VariableDeclaration,
    ) -> Result<()> {
        Ok(())
    }
    fn post_visit_overrides(&mut self, _specifier: &OverrideSpecifier) -> Result<()> {
        Ok(())
    }
    fn post_visit_user_defined_value_type(
        &mut self,
        _value_type: &UserDefinedValueTypeDefinition,
    ) -> Result<()> {
        Ok(())
    }
    fn post_visit_contract_definition(&mut self, _definition: &ContractDefinition) -> Result<()> {
        Ok(())
    }
    fn post_visit_using_for(&mut self, _directive: &UsingForDirective) -> Result<()> {
        Ok(())
    }
    fn post_visit_unary_operation(&mut self, _unary_op: &UnaryOperation) -> Result<()> {
        Ok(())
    }
    fn post_visit_binary_operation(&mut self, _binary_op: &BinaryOperation) -> Result<()> {
        Ok(())
    }
    fn post_visit_conditional(&mut self, _conditional: &Conditional) -> Result<()> {
        Ok(())
    }
    fn post_visit_tuple_expression(&mut self, _tuple_expression: &TupleExpression) -> Result<()> {
        Ok(())
    }
    fn post_visit_new_expression(&mut self, _new_expression: &NewExpression) -> Result<()> {
        Ok(())
    }
    fn post_visit_assignment(&mut self, _assignment: &Assignment) -> Result<()> {
        Ok(())
    }
    fn post_visit_identifier(&mut self, _identifier: &Identifier) -> Result<()> {
        Ok(())
    }
    fn post_visit_index_access(&mut self, _index_access: &IndexAccess) -> Result<()> {
        Ok(())
    }
    fn post_visit_index_range_access(
        &mut self,
        _index_range_access: &IndexRangeAccess,
    ) -> Result<()> {
        Ok(())
    }
    fn post_visit_while_statement(&mut self, _while_statement: &WhileStatement) -> Result<()> {
        Ok(())
    }
    fn post_visit_for_statement(&mut self, _for_statement: &ForStatement) -> Result<()> {
        Ok(())
    }
    fn post_visit_if_statement(&mut self, _if_statement: &IfStatement) -> Result<()> {
        Ok(())
    }
    fn post_visit_do_while_statement(
        &mut self,
        _do_while_statement: &DoWhileStatement,
    ) -> Result<()> {
        Ok(())
    }
    fn post_visit_emit_statement(&mut self, _emit_statement: &EmitStatement) -> Result<()> {
        Ok(())
    }
    fn post_visit_unchecked_block(&mut self, _unchecked_block: &UncheckedBlock) -> Result<()> {
        Ok(())
    }
    fn post_visit_try_statement(&mut self, _try_statement: &TryStatement) -> Result<()> {
        Ok(())
    }
    fn post_visit_revert_statement(&mut self, _revert_statement: &RevertStatement) -> Result<()> {
        Ok(())
    }
    fn post_visit_member_access(&mut self, _member_access: &MemberAccess) -> Result<()> {
        Ok(())
    }
    fn post_visit_mapping(&mut self, _mapping: &Mapping) -> Result<()> {
        Ok(())
    }
    fn post_visit_elementary_type_name(
        &mut self,
        _elementary_type_name: &ElementaryTypeName,
    ) -> Result<()> {
        Ok(())
    }
    fn post_visit_literal(&mut self, _literal: &Literal) -> Result<()> {
        Ok(())
    }
    fn post_visit_function_type_name(
        &mut self,
        _function_type_name: &FunctionTypeName,
    ) -> Result<()> {
        Ok(())
    }
    fn post_visit_array_type_name(&mut self, _array_type_name: &ArrayTypeName) -> Result<()> {
        Ok(())
    }
    fn post_visit_function_call_options(
        &mut self,
        _function_call: &FunctionCallOptions,
    ) -> Result<()> {
        Ok(())
    }
    fn post_visit_return(&mut self, _return: &Return) -> Result<()> {
        Ok(())
    }
    fn post_visit_inheritance_specifier(
        &mut self,
        _specifier: &InheritanceSpecifier,
    ) -> Result<()> {
        Ok(())
    }
    fn post_visit_modifier_invocation(&mut self, _invocation: &ModifierInvocation) -> Result<()> {
        Ok(())
    }
    fn post_visit_inline_assembly(&mut self, _assembly: &InlineAssembly) -> Result<()> {
        Ok(())
    }
    fn post_visit_external_assembly_reference(
        &mut self,
        _ref: &ExternalInlineAssemblyReference,
    ) -> Result<()> {
        Ok(())
    }
}

pub trait Walk {
    fn walk(&self, visitor: &mut dyn Visitor) -> Result<()>;
}

macro_rules! impl_walk {
    // Implement `Walk` for a type, calling the given function.
    ($ty:ty, | $val:ident, $visitor:ident | $e:expr) => {
        impl Walk for $ty {
            fn walk(&self, visitor: &mut dyn Visitor) -> Result<()> {
                let $val = self;
                let $visitor = visitor;
                $e
            }
        }
    };
    ($ty:ty, $func:ident) => {
        impl_walk!($ty, |obj, visitor| {
            visitor.$func(obj)?;
            paste! { visitor.[<post_ $func>](obj)?; }
            Ok(())
        });
    };
    ($ty:ty, $func:ident, | $val:ident, $visitor:ident | $e:expr) => {
        impl_walk!($ty, |$val, $visitor| {
            $visitor.$func($val)?;
            let r = $e;
            if r.is_err() {
                return r;
            }
            paste! { $visitor.[<post_ $func>]($val)?; }
            Ok(())
        });
    };
}

impl_walk!(SourceUnit, visit_source_unit, |source_unit, visitor| {
    for node in &source_unit.nodes {
        node.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(SourceUnitPart, |part, visitor| {
    match part {
        SourceUnitPart::ContractDefinition(contract) => contract.walk(visitor),
        SourceUnitPart::UsingForDirective(directive) => directive.walk(visitor),
        SourceUnitPart::ErrorDefinition(error) => error.walk(visitor),
        SourceUnitPart::StructDefinition(struct_) => struct_.walk(visitor),
        SourceUnitPart::VariableDeclaration(declaration) => declaration.walk(visitor),
        SourceUnitPart::FunctionDefinition(function) => function.walk(visitor),
        SourceUnitPart::UserDefinedValueTypeDefinition(value_type) => value_type.walk(visitor),
        SourceUnitPart::ImportDirective(directive) => directive.walk(visitor),
        SourceUnitPart::EnumDefinition(enum_) => enum_.walk(visitor),
        SourceUnitPart::PragmaDirective(directive) => directive.walk(visitor),
    }
});

impl_walk!(ContractDefinition, visit_contract_definition, |contract, visitor| {
    for base_contract in &contract.base_contracts {
        base_contract.walk(visitor)?;
    }

    for part in &contract.nodes {
        match part {
            ContractDefinitionPart::FunctionDefinition(function) => function.walk(visitor),
            ContractDefinitionPart::ErrorDefinition(error) => error.walk(visitor),
            ContractDefinitionPart::EventDefinition(event) => event.walk(visitor),
            ContractDefinitionPart::StructDefinition(struct_) => struct_.walk(visitor),
            ContractDefinitionPart::VariableDeclaration(declaration) => declaration.walk(visitor),
            ContractDefinitionPart::ModifierDefinition(modifier) => modifier.walk(visitor),
            ContractDefinitionPart::UserDefinedValueTypeDefinition(definition) => {
                definition.walk(visitor)
            }
            ContractDefinitionPart::UsingForDirective(directive) => directive.walk(visitor),
            ContractDefinitionPart::EnumDefinition(enum_) => enum_.walk(visitor),
        }?;
    }
    Ok(())
});

impl_walk!(Expression, visit_expression, |expr, visitor| {
    match expr {
        Expression::FunctionCall(expression) => expression.walk(visitor),
        Expression::MemberAccess(member_access) => member_access.walk(visitor),
        Expression::IndexAccess(index_access) => index_access.walk(visitor),
        Expression::UnaryOperation(unary_op) => unary_op.walk(visitor),
        Expression::BinaryOperation(expression) => expression.walk(visitor),
        Expression::Conditional(expression) => expression.walk(visitor),
        Expression::TupleExpression(tuple) => tuple.walk(visitor),
        Expression::NewExpression(expression) => expression.walk(visitor),
        Expression::Assignment(expression) => expression.walk(visitor),
        Expression::Identifier(identifier) => identifier.walk(visitor),
        Expression::FunctionCallOptions(function_call) => function_call.walk(visitor),
        Expression::IndexRangeAccess(range_access) => range_access.walk(visitor),
        Expression::Literal(literal) => literal.walk(visitor),
        Expression::ElementaryTypeNameExpression(type_name) => type_name.walk(visitor),
    }
});

impl_walk!(Statement, visit_statement, |statement, visitor| {
    match statement {
        Statement::Block(block) => block.walk(visitor),
        Statement::WhileStatement(statement) => statement.walk(visitor),
        Statement::ForStatement(statement) => statement.walk(visitor),
        Statement::IfStatement(statement) => statement.walk(visitor),
        Statement::DoWhileStatement(statement) => statement.walk(visitor),
        Statement::EmitStatement(statement) => statement.walk(visitor),
        Statement::VariableDeclarationStatement(statement) => statement.walk(visitor),
        Statement::ExpressionStatement(statement) => statement.walk(visitor),
        Statement::UncheckedBlock(statement) => statement.walk(visitor),
        Statement::TryStatement(statement) => statement.walk(visitor),
        Statement::RevertStatement(statement) => statement.walk(visitor),
        Statement::Return(statement) => statement.walk(visitor),
        Statement::InlineAssembly(assembly) => assembly.walk(visitor),
        Statement::Break(_) | Statement::Continue(_) | Statement::PlaceholderStatement(_) => Ok(()),
    }
});

impl_walk!(FunctionDefinition, visit_function_definition, |function, visitor| {
    function.parameters.walk(visitor)?;
    function.return_parameters.walk(visitor)?;

    if let Some(overrides) = &function.overrides {
        overrides.walk(visitor)?;
    }

    if let Some(body) = &function.body {
        body.walk(visitor)?;
    }

    for m in &function.modifiers {
        m.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(ErrorDefinition, visit_error_definition, |error, visitor| {
    error.parameters.walk(visitor)
});

impl_walk!(EventDefinition, visit_event_definition, |event, visitor| {
    event.parameters.walk(visitor)
});

impl_walk!(StructDefinition, visit_struct_definition, |struct_, visitor| {
    for member in &struct_.members {
        member.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(ModifierDefinition, visit_modifier_definition, |modifier, visitor| {
    if let Some(body) = modifier.body.as_ref() {
        body.walk(visitor)?;
    }
    if let Some(override_) = &modifier.overrides {
        override_.walk(visitor)?;
    }
    modifier.parameters.walk(visitor)?;
    Ok(())
});

impl_walk!(VariableDeclaration, visit_variable_declaration, |declaration, visitor| {
    if let Some(value) = &declaration.value {
        value.walk(visitor)?;
    }

    if let Some(type_name) = &declaration.type_name {
        type_name.walk(visitor)?;
    }

    Ok(())
});

impl_walk!(OverrideSpecifier, visit_overrides, |override_, visitor| {
    for type_name in &override_.overrides {
        type_name.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(UserDefinedValueTypeDefinition, visit_user_defined_value_type, |value_type, visitor| {
    value_type.underlying_type.walk(visitor)
});

impl_walk!(FunctionCallOptions, visit_function_call_options, |function_call, visitor| {
    function_call.expression.walk(visitor)?;
    for option in &function_call.options {
        option.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(Return, visit_return, |return_, visitor| {
    if let Some(expr) = return_.expression.as_ref() {
        expr.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(UsingForDirective, visit_using_for, |directive, visitor| {
    if let Some(type_name) = &directive.type_name {
        type_name.walk(visitor)?;
    }
    if let Some(library_name) = &directive.library_name {
        library_name.walk(visitor)?;
    }
    for function in &directive.function_list {
        function.walk(visitor)?;
    }

    Ok(())
});

impl_walk!(UnaryOperation, visit_unary_operation, |unary_op, visitor| {
    unary_op.sub_expression.walk(visitor)
});

impl_walk!(BinaryOperation, visit_binary_operation, |binary_op, visitor| {
    binary_op.lhs.walk(visitor)?;
    binary_op.rhs.walk(visitor)?;
    Ok(())
});

impl_walk!(Conditional, visit_conditional, |conditional, visitor| {
    conditional.condition.walk(visitor)?;
    conditional.true_expression.walk(visitor)?;
    conditional.false_expression.walk(visitor)?;
    Ok(())
});

impl_walk!(TupleExpression, visit_tuple_expression, |tuple_expression, visitor| {
    for component in tuple_expression.components.iter().filter_map(|component| component.as_ref()) {
        component.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(NewExpression, visit_new_expression, |new_expression, visitor| {
    new_expression.type_name.walk(visitor)
});

impl_walk!(Assignment, visit_assignment, |assignment, visitor| {
    assignment.lhs.walk(visitor)?;
    assignment.rhs.walk(visitor)?;
    Ok(())
});

impl_walk!(IfStatement, visit_if_statement, |if_statement, visitor| {
    if_statement.condition.walk(visitor)?;
    if_statement.true_body.walk(visitor)?;

    if let Some(false_body) = &if_statement.false_body {
        false_body.walk(visitor)?;
    }

    Ok(())
});

impl_walk!(IndexAccess, visit_index_access, |index_access, visitor| {
    index_access.base_expression.walk(visitor)?;
    if let Some(index_expression) = &index_access.index_expression {
        index_expression.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(IndexRangeAccess, visit_index_range_access, |index_range_access, visitor| {
    index_range_access.base_expression.walk(visitor)?;
    if let Some(start_expression) = &index_range_access.start_expression {
        start_expression.walk(visitor)?;
    }
    if let Some(end_expression) = &index_range_access.end_expression {
        end_expression.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(WhileStatement, visit_while_statement, |while_statement, visitor| {
    while_statement.condition.walk(visitor)?;
    while_statement.body.walk(visitor)?;
    Ok(())
});

impl_walk!(ForStatement, visit_for_statement, |for_statement, visitor| {
    for_statement.body.walk(visitor)?;
    if let Some(condition) = &for_statement.condition {
        condition.walk(visitor)?;
    }

    if let Some(loop_expression) = &for_statement.loop_expression {
        loop_expression.walk(visitor)?;
    }

    if let Some(initialization_expr) = &for_statement.initialization_expression {
        initialization_expr.walk(visitor)?;
    }

    Ok(())
});

impl_walk!(DoWhileStatement, visit_do_while_statement, |do_while_statement, visitor| {
    do_while_statement.body.walk(visitor)?;
    do_while_statement.condition.walk(visitor)?;
    Ok(())
});

impl_walk!(EmitStatement, visit_emit_statement, |emit_statement, visitor| {
    emit_statement.event_call.walk(visitor)
});

impl_walk!(VariableDeclarationStatement, |stmt, visitor| {
    for declaration in stmt.declarations.iter().filter_map(|d| d.as_ref()) {
        declaration.walk(visitor)?;
    }
    if let Some(initial_value) = &stmt.initial_value {
        initial_value.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(UncheckedBlock, visit_unchecked_block, |unchecked_block, visitor| {
    for statement in &unchecked_block.statements {
        statement.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(TryStatement, visit_try_statement, |try_statement, visitor| {
    for clause in &try_statement.clauses {
        clause.block.walk(visitor)?;

        if let Some(parameter_list) = &clause.parameters {
            parameter_list.walk(visitor)?;
        }
    }

    try_statement.external_call.walk(visitor)
});

impl_walk!(RevertStatement, visit_revert_statement, |revert_statement, visitor| {
    revert_statement.error_call.walk(visitor)
});

impl_walk!(MemberAccess, visit_member_access, |member_access, visitor| {
    member_access.expression.walk(visitor)
});

impl_walk!(FunctionCall, visit_function_call, |function_call, visitor| {
    function_call.expression.walk(visitor)?;
    for argument in &function_call.arguments {
        argument.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(Block, visit_block, |block, visitor| {
    for statement in &block.statements {
        statement.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(UserDefinedTypeName, visit_user_defined_type_name, |type_name, visitor| {
    if let Some(path_node) = &type_name.path_node {
        path_node.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(TypeName, visit_type_name, |type_name, visitor| {
    match type_name {
        TypeName::ElementaryTypeName(type_name) => type_name.walk(visitor),
        TypeName::UserDefinedTypeName(type_name) => type_name.walk(visitor),
        TypeName::Mapping(mapping) => mapping.walk(visitor),
        TypeName::ArrayTypeName(array) => array.walk(visitor),
        TypeName::FunctionTypeName(function) => function.walk(visitor),
    }
});

impl_walk!(FunctionTypeName, visit_function_type_name, |function, visitor| {
    function.parameter_types.walk(visitor)?;
    function.return_parameter_types.walk(visitor)?;
    Ok(())
});

impl_walk!(ParameterList, visit_parameter_list, |parameter_list, visitor| {
    for parameter in &parameter_list.parameters {
        parameter.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(Mapping, visit_mapping, |mapping, visitor| {
    mapping.key_type.walk(visitor)?;
    mapping.value_type.walk(visitor)?;
    Ok(())
});

impl_walk!(ArrayTypeName, visit_array_type_name, |array, visitor| {
    array.base_type.walk(visitor)?;
    if let Some(length) = &array.length {
        length.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(InheritanceSpecifier, visit_inheritance_specifier, |specifier, visitor| {
    specifier.base_name.walk(visitor)?;
    for arg in &specifier.arguments {
        arg.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(ModifierInvocation, visit_modifier_invocation, |invocation, visitor| {
    for arg in &invocation.arguments {
        arg.walk(visitor)?;
    }
    invocation.modifier_name.walk(visitor)?;
    Ok(())
});

impl_walk!(InlineAssembly, visit_inline_assembly, |assembly, visitor| {
    for reference in &assembly.external_references {
        reference.walk(visitor)?;
    }
    Ok(())
});

impl_walk!(ExternalInlineAssemblyReference, visit_external_assembly_reference);

impl_walk!(ElementaryTypeName, visit_elementary_type_name);
impl_walk!(Literal, visit_literal);
impl_walk!(ImportDirective, visit_import_directive);
impl_walk!(PragmaDirective, visit_pragma_directive);
impl_walk!(IdentifierPath, visit_identifier_path);
impl_walk!(EnumDefinition, visit_enum_definition);
impl_walk!(Identifier, visit_identifier);

impl_walk!(UserDefinedTypeNameOrIdentifierPath, |type_name, visitor| {
    match type_name {
        UserDefinedTypeNameOrIdentifierPath::UserDefinedTypeName(type_name) => {
            type_name.walk(visitor)
        }
        UserDefinedTypeNameOrIdentifierPath::IdentifierPath(identifier_path) => {
            identifier_path.walk(visitor)
        }
    }
});

impl_walk!(BlockOrStatement, |block_or_statement, visitor| {
    match block_or_statement {
        BlockOrStatement::Block(block) => block.walk(visitor),
        BlockOrStatement::Statement(statement) => statement.walk(visitor),
    }
});

impl_walk!(ExpressionOrVariableDeclarationStatement, |val, visitor| {
    match val {
        ExpressionOrVariableDeclarationStatement::ExpressionStatement(expression) => {
            expression.walk(visitor)
        }
        ExpressionOrVariableDeclarationStatement::VariableDeclarationStatement(stmt) => {
            stmt.walk(visitor)
        }
    }
});

impl_walk!(IdentifierOrIdentifierPath, |val, visitor| {
    match val {
        IdentifierOrIdentifierPath::Identifier(ident) => ident.walk(visitor),
        IdentifierOrIdentifierPath::IdentifierPath(path) => path.walk(visitor),
    }
});

impl_walk!(ExpressionStatement, |expression_statement, visitor| {
    expression_statement.expression.walk(visitor)
});

impl_walk!(ElementaryTypeNameExpression, |type_name, visitor| {
    type_name.type_name.walk(visitor)
});

impl_walk!(ElementaryOrRawTypeName, |type_name, visitor| {
    match type_name {
        ElementaryOrRawTypeName::ElementaryTypeName(type_name) => type_name.walk(visitor),
        ElementaryOrRawTypeName::Raw(_) => Ok(()),
    }
});

impl_walk!(UsingForFunctionItem, |item, visitor| {
    match item {
        UsingForFunctionItem::Function(func) => func.function.walk(visitor),
        UsingForFunctionItem::OverloadedOperator(operator) => operator.walk(visitor),
    }
});

impl_walk!(OverloadedOperator, |operator, visitor| operator.definition.walk(visitor));
