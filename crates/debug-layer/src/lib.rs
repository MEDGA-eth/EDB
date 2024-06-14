//! # edb-debug-layer
//!
//! EDB's core debugging layer

mod artifact;
mod handler;
mod inspector;
mod utils;

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use alloy_primitives::Address;
pub use artifact::*;
pub use handler::DebugHanlder;
pub use inspector::DebugInspector;

#[derive(Debug, Default)]
pub struct DebugLayer {
    /// Identified contracts.
    pub identified_contracts: Rc<RefCell<HashMap<Address, String>>>,
    /// Map of source files. Note that each address will have a compilation artifact.
    pub compilation_artifacts: Rc<RefCell<HashMap<Address, Arc<CompilationArtifact>>>>,

    // Compilation artifact from local file system
    local_compilation_artifact: Option<Rc<RefCell<CompilationArtifact>>>,
}

impl DebugLayer {
    pub fn new<T>(local: Option<T>) -> Self
    where
        T: AsCompilationArtifact,
    {
        Self {
            local_compilation_artifact: local
                .map(|t| Rc::new(RefCell::new(t.as_compilation_artifact()))),
            ..Default::default()
        }
    }

    pub fn new_inspector(&self) -> DebugInspector {
        DebugInspector {
            identified_contracts: Rc::clone(&self.identified_contracts),
            compilation_artifacts: Rc::clone(&self.compilation_artifacts),
            local_compilation_artifact: self.local_compilation_artifact.as_ref().map(Rc::clone),
        }
    }

    pub fn new_handler(&self) -> DebugHanlder {
        DebugHanlder {
            identified_contracts: Rc::clone(&self.identified_contracts),
            compilation_artifacts: Rc::clone(&self.compilation_artifacts),
            local_compilation_artifact: self.local_compilation_artifact.as_ref().map(Rc::clone),
        }
    }
}
