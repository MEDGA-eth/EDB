use std::{collections::HashMap, sync::Arc};

use foundry_compilers::artifacts::ContractBytecodeSome;
use rustc_hash::FxHashMap;

#[derive(Clone, Debug)]
pub struct BytecodeData {
    pub bytecode: ContractBytecodeSome,
    pub build_id: String,
    pub file_id: u32,
}

#[derive(Clone, Debug)]
pub struct SourceData {
    pub source: Arc<String>,
    pub name: String,
}

/// Contract source code and bytecode data used for debugger.
#[derive(Clone, Debug, Default)]
pub struct CompilationArtifact {
    /// Map over build_id -> file_id -> (source code, language)
    pub sources_by_id: HashMap<String, FxHashMap<u32, SourceData>>,
    /// Map over contract name -> Vec<(bytecode, build_id, file_id)>
    pub artifacts_by_name: HashMap<String, Vec<BytecodeData>>,
}

pub trait AsCompilationArtifact {
    fn as_compilation_artifact(&self) -> CompilationArtifact;
}

impl<T> From<T> for CompilationArtifact
where
    T: AsCompilationArtifact,
{
    fn from(t: T) -> Self {
        t.as_compilation_artifact()
    }
}
