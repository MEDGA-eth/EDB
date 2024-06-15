use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use alloy_primitives::Address;

use crate::CompilationArtifact;

pub struct DebugInspector {
    pub(crate) identified_contracts: Rc<RefCell<HashMap<Address, String>>>,
    pub(crate) compilation_artifacts: Rc<RefCell<HashMap<Address, Arc<CompilationArtifact>>>>,
    pub(crate) local_compilation_artifact: Option<Rc<RefCell<CompilationArtifact>>>,
}

pub struct PreDebugInspector {
    pub(crate) creation_code: Rc<RefCell<HashMap<Address, Option<u64>>>>,
}
