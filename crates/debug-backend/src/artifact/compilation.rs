use std::ops::{Deref, DerefMut};

use alloy_json_abi::JsonAbi;
use foundry_compilers::artifacts::{Ast, Bytecode, CompilerOutput, DeployedBytecode};

#[derive(Clone, Debug)]
pub struct CompilationUnit {
    pub id: u32,
    pub ast: Ast,
    pub abi: JsonAbi,
    pub bytecode: Bytecode,
    pub deployed_bytecode: DeployedBytecode,
}

/// Contract source code and bytecode data used for debugger.
#[derive(Clone, Debug, Default)]
pub struct CompilationArtifact(CompilerOutput);

impl CompilationArtifact {
    pub fn new(output: CompilerOutput) -> Self {
        Self(output)
    }
}

impl Deref for CompilationArtifact {
    type Target = CompilerOutput;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CompilationArtifact {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
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
