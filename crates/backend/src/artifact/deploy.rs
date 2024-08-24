use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
};

use alloy_json_abi::JsonAbi;
use alloy_primitives::{Address, Bytes};
use eyre::{eyre, OptionExt, Result};
use foundry_block_explorers::contract::Metadata;
use foundry_compilers::artifacts::{
    Bytecode, CompilerOutput, DeployedBytecode, Evm, SourceUnit, Sources,
};
use revm::primitives::Bytecode as RevmBytecode;
use serde::{Deserialize, Serialize};

use crate::{
    analysis::prune::ASTPruner,
    utils::compilation::{bytecode_align_similarity, link_contracts_fakely},
};

const SIMILARITY_THRESHOLD: f64 = 0.7;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: PathBuf,
    pub code: Arc<String>,
    pub ast: SourceUnit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeployArtifact {
    // The following fields exclusively belongs to a specific contract
    pub contract_name: String,
    pub file_id: u32, // the file id of the
    pub abi: JsonAbi,
    pub evm: Evm,
    pub constructor_arguments: Bytes,

    // Onchain Address. It is None if the deployed artifact is compiled from local source code.
    pub onchain_address: Option<Address>,

    // Other contract's source code may also get involved in the compilation process
    pub sources: BTreeMap<u32, SourceFile>,
}

impl DeployArtifact {
    pub fn deployed_bytecode(&self) -> Option<&Bytecode> {
        self.evm.deployed_bytecode.as_ref().and_then(|b| b.bytecode.as_ref())
    }

    pub fn constructor_bytecode(&self) -> Option<&Bytecode> {
        self.evm.bytecode.as_ref()
    }
}

/// This builder is used to build a deployment artifact.
///
/// The contract name is the one of the subject contract.
/// The source map is the source code of all contracts involved in the compilation process.
/// The compiler output is the output of the compiler.
/// The metadata is the metadata collected from the block explorer.
/// The bytecode is the on-chain bytecode of the subject contract.
#[derive(Debug)]
pub struct DeployArtifactBuilder {
    pub input_sources: Sources,
    pub compilation_output: CompilerOutput,
    pub explorer_meta: Metadata,
    pub onchain_bytecode: RevmBytecode,
    pub onchain_address: Address,
}

impl DeployArtifactBuilder {
    pub fn build(self) -> Result<DeployArtifact> {
        let Self {
            input_sources,
            compilation_output: mut output,
            explorer_meta: meta,
            onchain_bytecode: bytecode,
            onchain_address,
        } = self;

        let contract_name = meta.contract_name.to_string();

        trace!("building deployment artifact for {}", contract_name);
        let bytecode = bytecode.original_byte_slice();

        // Let first link the contracts, to have a more accurate similarity check.
        for (_, contracts) in output.contracts.iter_mut() {
            for (_, contract) in contracts.iter_mut() {
                // link deployed bytecode
                if let Some(Evm { deployed_bytecode: Some(ref mut deployed_bytecode), .. }) =
                    contract.evm
                {
                    link_contracts_fakely(
                        deployed_bytecode
                            .bytecode
                            .as_mut()
                            .ok_or_eyre("no deployed bytecode found")?,
                        None,
                    )?;
                }

                // link constructor bytecode
                if let Some(Evm { bytecode: Some(ref mut bytecode), .. }) = contract.evm {
                    link_contracts_fakely(bytecode, None)?;
                }
            }
        }

        // Let first find the correct compiler artifact for the specific contract.
        let mut selected = None;
        let mut max_similarity = 0.0;

        // Collect all contracts with the same name.
        let matched_contracts = output
            .contracts
            .iter()
            .flat_map(|(path, cs)| cs.iter().map(move |(name, contract)| (path, name, contract)))
            .filter_map(|(path, name, contract)| {
                if name.as_str() != contract_name {
                    return None;
                }

                if let Some(Evm {
                    deployed_bytecode: Some(DeployedBytecode { bytecode: Some(code), .. }),
                    ..
                }) = &contract.evm
                {
                    code.object.as_bytes().map(|c| (Vec::from(c.as_ref()), (path, contract)))
                } else {
                    None
                }
            })
            .collect::<HashMap<_, _>>();
        trace!(name=contract_name, addr=?onchain_address, n=matched_contracts.len(), "contracts with the same name");

        if matched_contracts.is_empty() {
            return Err(eyre!("no contract with the same name found"));
        } else {
            // If there are multiple contracts with the same name, then we need to find the most
            // similar one
            for (bytecode_to_check, (path_ref, contract_ref)) in matched_contracts.iter() {
                let similarity = bytecode_align_similarity(bytecode, bytecode_to_check);
                trace!("similarity of contracts with the same name: {}", similarity);

                if similarity > max_similarity {
                    max_similarity = similarity;
                    selected = Some((*contract_ref, *path_ref));
                }
            }
        }

        if max_similarity < SIMILARITY_THRESHOLD {
            return Err(eyre!(format!(
                "no similar contract found, with the max similarity of {max_similarity} for {onchain_address}",
            )));
        }

        let (compilation_ref, path_ref) = selected.ok_or_eyre("no compilation reference found")?;

        // get file id
        let file_id = output
            .sources
            .iter()
            .find_map(|(path, source)| if path == path_ref { Some(source.id) } else { None })
            .ok_or_eyre("no file id found")?;

        // collect all repated source
        let mut sources = BTreeMap::new();
        for (path, source) in output.sources.iter_mut() {
            let ast = ASTPruner::convert(source.ast.as_mut().ok_or_eyre("AST does not exist")?)?;
            let source_code = &input_sources.get(path).ok_or_eyre("missing source code")?.content;
            sources.insert(
                source.id,
                SourceFile { path: path.clone(), code: Arc::clone(source_code), ast: ast.clone() },
            );
        }

        Ok(DeployArtifact {
            contract_name: contract_name.to_string(),
            file_id,
            abi: compilation_ref.abi.as_ref().ok_or_eyre("missing abi")?.clone(),
            evm: compilation_ref.evm.as_ref().ok_or_eyre("missing evm")?.clone(),
            constructor_arguments: meta.constructor_arguments,
            sources,
            onchain_address: Some(onchain_address),
        })
    }
}
