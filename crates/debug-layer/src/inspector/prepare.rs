use std::{cell::RefCell, collections::HashMap, rc::Rc};

use alloy_primitives::Address;

#[derive(Debug, Default)]
pub struct PrepareInspector {
    pub(crate) creation_code: Rc<RefCell<HashMap<Address, Option<u64>>>>,
}
