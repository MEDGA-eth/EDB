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

use std::{collections::HashMap, sync::Arc};

use alloy_dyn_abi::DynSolType;
use foundry_compilers::artifacts::{
    ContractDefinition, EnumDefinition, Expression, StructDefinition, TypeName,
    UserDefinedValueTypeDefinition,
};
use once_cell::sync::OnceCell;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use serde::{Deserialize, Serialize};

use crate::analysis::{Analyzer, macros::universal_id};

universal_id! {
    /// A Universal Type Identifier (UTID) is a unique identifier for a type in a contract.
    UTID => 0
}

/// A reference-counted pointer to a UserDefinedType.
#[derive(Debug, Clone)]
pub struct UserDefinedTypeRef {
    inner: Arc<RwLock<UserDefinedType>>,
    /* cached readonly fields */
    utid: OnceCell<UTID>,
    /// The AST node ID of the type definition.
    ast_id: OnceCell<usize>,
    /// Variant of the user defined type.
    variant: OnceCell<UserDefinedTypeVariant>,
}

impl UserDefinedTypeRef {
    /// Creates a new UserDefinedTypeRef from a UserDefinedType.
    pub fn new(inner: UserDefinedType) -> Self {
        Self {
            inner: Arc::new(RwLock::new(inner)),
            utid: OnceCell::new(),
            ast_id: OnceCell::new(),
            variant: OnceCell::new(),
        }
    }
}

impl From<UserDefinedType> for UserDefinedTypeRef {
    fn from(value: UserDefinedType) -> Self {
        Self::new(value)
    }
}

#[allow(unused)]
impl UserDefinedTypeRef {
    pub(crate) fn read(&self) -> RwLockReadGuard<'_, UserDefinedType> {
        self.inner.read()
    }

    pub(crate) fn write(&self) -> RwLockWriteGuard<'_, UserDefinedType> {
        self.inner.write()
    }

    pub(crate) fn utid(&self) -> UTID {
        *self.utid.get_or_init(|| self.inner.read().utid)
    }

    pub(crate) fn ast_id(&self) -> usize {
        *self.ast_id.get_or_init(|| self.inner.read().variant.ast_id())
    }

    /// Returns true if this user defined type is a contract, library, or interface type.
    pub(crate) fn is_typed_address(&self) -> bool {
        matches!(self.inner.read().variant, UserDefinedTypeVariant::Contract(_))
    }

    pub(crate) fn variant(&self) -> &UserDefinedTypeVariant {
        self.variant.get_or_init(|| self.inner.read().variant.clone())
    }
}

impl Serialize for UserDefinedTypeRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.inner.read().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for UserDefinedTypeRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let user_defined_type = UserDefinedType::deserialize(deserializer)?;
        Ok(user_defined_type.into())
    }
}

/// A user-defined type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDefinedType {
    /// The unique type identifier.
    pub utid: UTID,
    /// The type definition.
    pub variant: UserDefinedTypeVariant,
    /// The ID of the source file that defines this type.
    pub source_id: u32,
}

impl UserDefinedType {
    /// Creates a new UserDefinedType from a UserDefinedValueTypeDefinition.
    pub fn new(source_id: u32, variant: UserDefinedTypeVariant) -> Self {
        Self { utid: UTID::next(), variant, source_id }
    }
}

/// Variant of the user defined type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum UserDefinedTypeVariant {
    /// A struct type.
    Struct(StructDefinition),
    /// An enum type.
    Enum(EnumDefinition),
    /// A user defined value type.
    UserDefinedValueType(UserDefinedValueTypeDefinition),
    /// A contract type.
    Contract(ContractDefinition),
}

impl UserDefinedTypeVariant {
    /// Returns the AST node ID of the type definition.
    pub fn ast_id(&self) -> usize {
        match self {
            Self::Struct(definition) => definition.id,
            Self::Enum(definition) => definition.id,
            Self::UserDefinedValueType(definition) => definition.id,
            Self::Contract(definition) => definition.id,
        }
    }
}

/// Converts a TypeName to a DynSolType.
///
/// # Arguments
/// * `all_user_defined_types` - A map of all user defined types.
/// * `type_name` - The type name to convert.
///
/// # Returns
/// The DynSolType.
pub fn dyn_sol_type(
    all_user_defined_types: &HashMap<usize, UserDefinedTypeRef>,
    type_name: &TypeName,
) -> Option<DynSolType> {
    match type_name {
        TypeName::ArrayTypeName(array_type_name) => {
            let base = dyn_sol_type(all_user_defined_types, &array_type_name.base_type)?;
            match array_type_name.length.as_ref() {
                Some(Expression::Literal(literal)) => {
                    let len = literal.value.as_ref()?;
                    let len = len.parse::<usize>().ok()?;
                    Some(DynSolType::FixedArray(Box::new(base), len))
                }
                Some(_) => None,
                None => Some(DynSolType::Array(Box::new(base))),
            }
        }
        TypeName::ElementaryTypeName(elementary_type_name) => {
            DynSolType::parse(&elementary_type_name.name).ok()
        }
        TypeName::FunctionTypeName(_) => Some(DynSolType::Function),
        TypeName::Mapping(_) => None,
        TypeName::UserDefinedTypeName(user_defined_type_name) => {
            if user_defined_type_name.referenced_declaration < 0 {
                return None;
            }

            let ty_def = all_user_defined_types
                .get(&(user_defined_type_name.referenced_declaration as usize))?;

            match ty_def.variant() {
                UserDefinedTypeVariant::Struct(definition) => {
                    let mut prop_names = Vec::with_capacity(definition.members.len());
                    let mut prop_types = Vec::with_capacity(definition.members.len());
                    for field in definition.members.iter() {
                        prop_names.push(field.name.clone());
                        // Propagate `None` from unresolvable field types (e.g. a
                        // struct member whose own type comes from a contract whose
                        // AST we didn't pull in, or a mapping-valued member). The
                        // prior `.unwrap()` panicked on those cases — solady's
                        // `ERC6551Test::testOnERC1155Received` hit this on a struct
                        // carrying a function-pointer member.
                        prop_types
                            .push(dyn_sol_type(all_user_defined_types, field.type_name.as_ref()?)?);
                    }
                    Some(DynSolType::CustomStruct {
                        name: definition.name.clone(),
                        prop_names,
                        tuple: prop_types,
                    })
                }
                UserDefinedTypeVariant::Enum(_) => Some(DynSolType::Uint(8)),
                UserDefinedTypeVariant::UserDefinedValueType(
                    user_defined_value_type_definition,
                ) => {
                    let underlying_type = &user_defined_value_type_definition.underlying_type;
                    dyn_sol_type(all_user_defined_types, underlying_type)
                }
                UserDefinedTypeVariant::Contract(_) => Some(DynSolType::Address),
            }
        }
    }
}

/* User defined type analysis */
impl Analyzer {
    pub(in crate::analysis) fn record_user_defined_value_type(
        &mut self,
        type_definition: &UserDefinedValueTypeDefinition,
    ) -> eyre::Result<()> {
        let user_defined_type = UserDefinedType::new(
            self.source_id,
            UserDefinedTypeVariant::UserDefinedValueType(type_definition.clone()),
        );
        self.user_defined_types.push(user_defined_type.into());
        Ok(())
    }

    pub(in crate::analysis) fn record_struct_type(
        &mut self,
        struct_definition: &StructDefinition,
    ) -> eyre::Result<()> {
        let user_defined_type = UserDefinedType::new(
            self.source_id,
            UserDefinedTypeVariant::Struct(struct_definition.clone()),
        );
        self.user_defined_types.push(user_defined_type.into());
        Ok(())
    }

    pub(in crate::analysis) fn record_enum_type(
        &mut self,
        enum_definition: &EnumDefinition,
    ) -> eyre::Result<()> {
        let user_defined_type = UserDefinedType::new(
            self.source_id,
            UserDefinedTypeVariant::Enum(enum_definition.clone()),
        );
        self.user_defined_types.push(user_defined_type.into());
        Ok(())
    }

    pub(in crate::analysis) fn record_contract_type(
        &mut self,
        contract_definition: &ContractDefinition,
    ) -> eyre::Result<()> {
        let user_defined_type = UserDefinedType::new(
            self.source_id,
            UserDefinedTypeVariant::Contract(contract_definition.clone()),
        );
        self.user_defined_types.push(user_defined_type.into());
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use alloy_dyn_abi::DynSolType;

    use crate::analysis::tests::compile_and_analyze;

    use super::*;

    #[test]
    fn test_parse_struct_as_dyn_sol_type() {
        let source = r#"
        contract C {
            struct MyStruct {
                uint256 a;
                uint256 b;
            }
            MyStruct internal myStruct;
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        let var = analysis.state_variables.first().unwrap();
        let ty = var.type_name().as_ref().unwrap();
        let dyn_ty = dyn_sol_type(&analysis.user_defined_types(), ty).unwrap();
        assert_eq!(
            dyn_ty,
            DynSolType::CustomStruct {
                name: "MyStruct".to_string(),
                prop_names: vec!["a".to_string(), "b".to_string()],
                tuple: vec![DynSolType::Uint(256), DynSolType::Uint(256)],
            }
        );
    }

    /// Regression: a struct member whose type cannot be lowered to a
    /// `DynSolType` (a mapping member, a function-pointer member, an
    /// unresolved user-defined member, etc.) used to panic at the unwrap
    /// in `dyn_sol_type`. Solady's `ERC1271Test::testSupportsERC7739`
    /// tripped this. The function must now propagate `None` instead.
    #[test]
    fn test_struct_with_mapping_member_returns_none() {
        let source = r#"
        contract C {
            struct WithMapping {
                uint256 a;
                mapping(uint256 => uint256) m;
            }
            WithMapping internal w;
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);
        let var = analysis.state_variables.first().unwrap();
        let ty = var.type_name().as_ref().unwrap();
        // Must not panic; should gracefully return None for an unrepresentable
        // (mapping-bearing) struct.
        assert!(dyn_sol_type(&analysis.user_defined_types(), ty).is_none());
    }
}
