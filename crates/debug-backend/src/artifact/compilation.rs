use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use alloy_json_abi::JsonAbi;
use eyre::{eyre, Result};
use foundry_compilers::artifacts::{
    CompilerOutput, DeployedBytecode, Evm, Source, SourceUnit, Sources,
};
use revm::primitives::Bytecode as RevmBytecode;

use crate::{
    analysis::prune::ASTPruner,
    utils::compilation::{bytecode_similarity, link_contracts_fakely},
};

const SIMILARITY_THRESHOLD: f64 = 0.7;

#[derive(Clone, Debug)]
pub struct SourceFile {
    pub path: PathBuf,
    pub code: Arc<String>,
    pub ast: SourceUnit,
}

#[derive(Clone, Debug)]
pub struct CompilationArtifact {
    // The following fields exclusively belongs to a specific contract
    pub contract_name: String,
    pub file_id: u32, // the file id of the
    pub abi: JsonAbi,
    pub evm: Evm,

    // Other contract's source code may also get involved in the compilation process
    pub sources: BTreeMap<u32, SourceFile>,
}

pub trait AsCompilationArtifact {
    fn as_artifact(self) -> Result<CompilationArtifact>;
}

/// This trait is used to convert a tuple of contract name, bytecode, sources and compiler output
/// into a CompilationArtifact.
///
/// The contract name is the one of the subject contract.
/// The bytecode is the on-chain bytecode of the subject contract.
/// The source map is the source code of all contracts involved in the compilation process.
/// The compiler output is the output of the compiler.
impl AsCompilationArtifact for (&str, RevmBytecode, &Sources, CompilerOutput) {
    fn as_artifact(self) -> Result<CompilationArtifact> {
        let (contract_name, bytecode, input_sources, mut output) = self;
        let bytecode = bytecode.original_byte_slice();

        // let first link the contracts, to have a more accurate similarity check
        for (_, contracts) in output.contracts.iter_mut() {
            for (_, contract) in contracts.iter_mut() {
                if let Some(Evm { deployed_bytecode: Some(ref mut deployed_bytecode), .. }) =
                    contract.evm
                {
                    link_contracts_fakely(deployed_bytecode, None)?;
                }
            }
        }

        // let first find the correct compiler artifact for the specific contract
        let mut selected = None;
        let mut max_similarity = 0.0;
        for (path, contracts) in output.contracts.iter() {
            for (_, contract) in contracts.iter().filter(|(name, _)| name.as_str() == contract_name)
            {
                if let Some(Evm {
                    deployed_bytecode:
                        Some(DeployedBytecode { bytecode: Some(bytecode_to_check), .. }),
                    ..
                }) = &contract.evm
                {
                    let bytecod_to_check = bytecode_to_check
                        .object
                        .as_bytes()
                        .ok_or(eyre!("missing bytecode object"))?
                        .as_ref();

                    let similarity = bytecode_similarity(bytecode, bytecod_to_check);
                    println!("similarity: {}", similarity);

                    if similarity > max_similarity {
                        max_similarity = similarity;
                        selected = Some((contract, path));
                    }
                }
            }
        }

        if max_similarity < SIMILARITY_THRESHOLD {
            return Err(eyre!("no similar contract found"));
        }

        let (compilation_ref, path_ref) =
            selected.ok_or(eyre!("no compilation reference found"))?;

        // get file id
        let file_id = output
            .sources
            .iter()
            .find_map(|(path, source)| if path == path_ref { Some(source.id) } else { None })
            .ok_or(eyre!("no file id found"))?;

        // collect all repated source
        let mut sources = BTreeMap::new();
        for (path, source) in output.sources.iter_mut() {
            let ast = ASTPruner::convert(source.ast.as_mut().ok_or(eyre!("AST does not exist"))?)?;
            let source_code = &input_sources.get(path).ok_or(eyre!("missing source code"))?.content;
            sources.insert(
                source.id,
                SourceFile { path: path.clone(), code: Arc::clone(&source_code), ast: ast.clone() },
            );
        }

        Ok(CompilationArtifact {
            contract_name: contract_name.to_string(),
            file_id,
            abi: compilation_ref.abi.as_ref().ok_or(eyre!("missing abi"))?.clone(),
            evm: compilation_ref.evm.as_ref().ok_or(eyre!("missing evm"))?.clone(),
            sources,
        })
    }
}
