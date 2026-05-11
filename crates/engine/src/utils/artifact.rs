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

//! Contract artifact management and metadata handling.
//!
//! This module provides utilities for managing compiled contract artifacts, including
//! metadata extraction, contract access, and constructor argument handling. The [`Artifact`]
//! struct consolidates all compilation-related data for contracts used in debugging.
//!
//! # Core Functionality
//!
//! ## Artifact Management
//! - **Metadata Storage**: Contract metadata including name, version, and constructor args
//! - **Compilation Data**: Full Solidity compiler input and output
//! - **Contract Access**: Easy access to compiled contract bytecode and ABI
//! - **Constructor Handling**: Support for constructor argument extraction and validation
//!
//! ## Integration Points
//! The artifact system integrates with:
//! - **Etherscan**: Loading verified contract source code and metadata
//! - **Foundry Compilers**: Handling Solidity compilation workflows
//! - **Code Tweaking**: Supporting bytecode replacement through recompilation
//! - **Analysis Engine**: Providing source code and ABI data for instrumentation

use alloy_primitives::Bytes;
use foundry_block_explorers::contract::{Metadata, SourceCodeEntry, SourceCodeMetadata};
use foundry_compilers::{
    ArtifactId,
    artifacts::{
        Bytecode, CompilerOutput, ConfigurableContractArtifact, Contract, ContractBytecode,
        DeployedBytecode, Evm, SolcInput, SolcLanguage, Source, SourceFile, Sources,
    },
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use tracing::error;

/// Compiled contract artifact with comprehensive metadata and compilation data.
///
/// This struct consolidates all information about a compiled contract, including
/// the original metadata from Etherscan, compiler input configuration, and the
/// complete compilation output. It serves as the primary container for contract
/// data throughout the debugging workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Metadata about the contract.
    pub meta: Metadata,
    /// Input for the Solidity compiler.
    pub input: SolcInput,
    /// Output from the Solidity compiler.
    pub output: CompilerOutput,
}

impl Artifact {
    /// Returns the contract name.
    pub fn contract_name(&self) -> &str {
        self.meta.contract_name.as_str()
    }

    /// Returns the compiler version.
    pub fn compiler_version(&self) -> &str {
        self.meta.compiler_version.as_str()
    }

    /// Returns the constructor arguments.
    pub fn constructor_arguments(&self) -> &Bytes {
        &self.meta.constructor_arguments
    }

    /// Subject contract
    pub fn contract(&self) -> Option<&Contract> {
        let contract_name = self.contract_name();

        self.output
            .contracts
            .values()
            .into_iter()
            .find(|c| c.contains_key(contract_name))
            .and_then(|contracts| contracts.get(contract_name))
    }

    /// Build an EDB [`Artifact`] from a foundry-compilers
    /// [`ConfigurableContractArtifact`] (and its [`ArtifactId`]).
    ///
    /// This is the local-source analogue of
    /// [`crate::orchestration::artifact::download_verified_source_code`]: it
    /// produces the same `Artifact { meta, input, output }` shape so the
    /// engine's downstream pipeline (analysis, instrumentation, recompilation,
    /// snapshots) can run against artifacts compiled locally by Foundry
    /// instead of fetched from Etherscan.
    ///
    /// Fields without an exact counterpart on the foundry side (chain-specific
    /// metadata such as `constructor_arguments`, `proxy`, etc.) are filled
    /// with sensible defaults; callers may overwrite them later if needed.
    pub fn from_foundry(id: &ArtifactId, art: &ConfigurableContractArtifact) -> eyre::Result<Self> {
        let meta = build_meta_from_foundry(id, art)?;
        let input = build_input_from_foundry(id, art)?;
        let output = build_output_from_foundry(id, art)?;
        Ok(Self { meta, input, output })
    }

    /// Find creation hooks (one-to-one mapping)
    pub fn find_creation_hooks<'a>(
        &'a self,
        recompiled: &'a Self,
    ) -> Vec<(&'a Contract, &'a Contract, &'a Bytes)> {
        let mut hooks = Vec::new();

        for (path, contracts) in &self.output.contracts {
            for (name, contract) in contracts {
                if let Some(recompiled_contract) =
                    recompiled.output.contracts.get(path).and_then(|c| c.get(name))
                {
                    hooks.push((contract, recompiled_contract, self.constructor_arguments()));
                } else {
                    error!("No recompiled contract found for {} in {}", name, path.display());
                }
            }
        }

        hooks
    }
}

#[cfg(test)]
impl Artifact {
    /// Construct a minimal stub suitable for unit tests. The artifact has empty
    /// source / compilation data; only `meta.contract_name` is set.
    pub fn test_stub(name: &str) -> Self {
        let meta = Metadata {
            source_code: SourceCodeMetadata::SourceCode(String::new()),
            abi: "[]".into(),
            contract_name: name.to_string(),
            compiler_version: "v0.8.0+commit.c7dfd78e".into(),
            optimization_used: 0,
            runs: 0,
            constructor_arguments: Bytes::default(),
            evm_version: "Default".into(),
            library: String::new(),
            license_type: String::new(),
            proxy: 0,
            implementation: None,
            swarm_source: String::new(),
        };
        Self { meta, input: SolcInput::default(), output: CompilerOutput::default() }
    }
}

/// Build `Metadata` (Etherscan-style) from a foundry artifact.
fn build_meta_from_foundry(
    id: &ArtifactId,
    art: &ConfigurableContractArtifact,
) -> eyre::Result<Metadata> {
    // Compiler version: prefix with `v` to match the Etherscan-style
    // (`v0.8.20+commit.xxx`) that `Metadata::compiler_version()` expects;
    // that helper tolerates a missing `v` prefix, so this is purely
    // cosmetic / forward-compatible.
    let compiler_version = art
        .metadata
        .as_ref()
        .map(|m| {
            let v = m.compiler.version.trim_start_matches('v');
            format!("v{v}")
        })
        .unwrap_or_else(|| format!("v{}", id.version));

    let (optimization_used, runs) = art
        .metadata
        .as_ref()
        .map(|m| {
            let enabled = m.settings.optimizer.enabled.unwrap_or(false);
            let runs = m.settings.optimizer.runs.unwrap_or(0) as u64;
            (u64::from(enabled), runs)
        })
        .unwrap_or((0, 0));

    let evm_version = art
        .metadata
        .as_ref()
        .and_then(|m| m.settings.evm_version)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Default".to_string());

    let abi_json = match &art.abi {
        Some(abi) => serde_json::to_string(abi).unwrap_or_else(|_| "[]".to_string()),
        None => "[]".to_string(),
    };

    // Source code: lift the metadata sources (path -> content) into the
    // Etherscan `Sources` variant. If sources are missing, fall back to a
    // single empty `Contract` entry so downstream consumers don't panic.
    let sources_map: std::collections::HashMap<String, SourceCodeEntry> = art
        .metadata
        .as_ref()
        .map(|m| {
            m.sources
                .inner
                .iter()
                .map(|(path, src)| {
                    let content = src.content.clone().unwrap_or_default();
                    (path.clone(), SourceCodeEntry::from(content))
                })
                .collect()
        })
        .unwrap_or_default();

    let source_code = if sources_map.is_empty() {
        SourceCodeMetadata::Sources(std::collections::HashMap::from([(
            id.name.clone(),
            SourceCodeEntry::from(String::new()),
        )]))
    } else {
        SourceCodeMetadata::Sources(sources_map)
    };

    Ok(Metadata {
        source_code,
        abi: abi_json,
        contract_name: id.name.clone(),
        compiler_version,
        optimization_used,
        runs,
        constructor_arguments: Bytes::default(),
        evm_version,
        library: String::new(),
        license_type: String::new(),
        proxy: 0,
        implementation: None,
        swarm_source: String::new(),
    })
}

/// Build the `SolcInput` that EDB's instrumenter expects.
///
/// We synthesise a standard-json input from whatever sources the foundry
/// artifact carries in its `metadata.sources`. Settings are carried over
/// where the shapes match (optimizer, viaIR, evm_version, remappings); the
/// rest fall back to `Default`.
fn build_input_from_foundry(
    id: &ArtifactId,
    art: &ConfigurableContractArtifact,
) -> eyre::Result<SolcInput> {
    use foundry_compilers::artifacts::Settings;

    let mut sources = Sources::new();
    if let Some(meta) = art.metadata.as_ref() {
        for (path_str, src) in &meta.sources.inner {
            let content = src.content.clone().unwrap_or_default();
            sources.insert(PathBuf::from(path_str), Source::new(content));
        }
    }
    // If the metadata didn't carry any sources at all, fall back to a single
    // empty entry under the artifact's source path so the input is at least
    // structurally well-formed.
    if sources.is_empty() {
        sources.insert(id.source.clone(), Source::new(String::new()));
    }

    let mut settings = Settings::default();
    if let Some(meta) = art.metadata.as_ref() {
        let m = &meta.settings;
        settings.optimizer = m.optimizer.clone();
        settings.evm_version = m.evm_version;
        settings.via_ir = m.via_ir;
        settings.remappings = m.remappings.clone();
        settings.metadata = m.metadata.clone();
        // m.libraries is `BTreeMap<String, String>`, whereas Settings::libraries
        // is the richer `Libraries` map type — we leave it at Default since
        // local artifacts already have their library links baked into the
        // deployed bytecode.
    }
    // Always emit the complete output selection so downstream recompilation
    // gets the AST, sourceMap, bytecode, etc.
    settings.output_selection =
        foundry_compilers::artifacts::output_selection::OutputSelection::complete_output_selection(
        );

    Ok(SolcInput::new(SolcLanguage::Solidity, sources, settings))
}

/// Build the `CompilerOutput` (contracts + sources) for the lifted artifact.
fn build_output_from_foundry(
    id: &ArtifactId,
    art: &ConfigurableContractArtifact,
) -> eyre::Result<CompilerOutput> {
    // Convert the compact bytecode pair into the non-compact forms that
    // `Contract { evm: Some(Evm { bytecode, deployed_bytecode, .. }), .. }`
    // wraps. The `From` impls already exist in foundry-compilers.
    let cb = ContractBytecode {
        abi: art.abi.clone(),
        bytecode: art.bytecode.clone().map(Bytecode::from),
        deployed_bytecode: art.deployed_bytecode.clone().map(DeployedBytecode::from),
    };

    let evm = Evm {
        assembly: art.assembly.clone(),
        legacy_assembly: art.legacy_assembly.clone(),
        bytecode: cb.bytecode,
        deployed_bytecode: cb.deployed_bytecode,
        method_identifiers: art.method_identifiers.clone().unwrap_or_default(),
        gas_estimates: art.gas_estimates.clone(),
    };

    let contract = Contract {
        abi: cb.abi,
        metadata: None,
        userdoc: art.userdoc.clone().unwrap_or_default(),
        devdoc: art.devdoc.clone().unwrap_or_default(),
        ir: art.ir.clone(),
        storage_layout: art.storage_layout.clone().unwrap_or_default(),
        transient_storage_layout: art.transient_storage_layout.clone().unwrap_or_default(),
        evm: Some(evm),
        ewasm: art.ewasm.clone(),
        ir_optimized: art.ir_optimized.clone(),
        ir_optimized_ast: art.ir_optimized_ast.clone(),
    };

    let mut contracts: BTreeMap<PathBuf, BTreeMap<String, Contract>> = BTreeMap::new();
    let mut by_name = BTreeMap::new();
    by_name.insert(id.name.clone(), contract);
    contracts.insert(id.source.clone(), by_name);

    let mut sources: BTreeMap<PathBuf, SourceFile> = BTreeMap::new();
    sources.insert(id.source.clone(), SourceFile { id: art.id.unwrap_or(0), ast: art.ast.clone() });

    Ok(CompilerOutput { errors: Vec::new(), sources, contracts })
}

#[cfg(test)]
mod from_foundry_tests {
    use super::*;
    use foundry_compilers::artifacts::{
        BytecodeObject, CompactBytecode, CompactDeployedBytecode, Compiler,
        Metadata as FoundryMeta, MetadataSettings, MetadataSource, MetadataSources, Optimizer,
        Output,
    };
    use semver::Version;
    use std::collections::BTreeMap;

    fn sample_foundry_artifact() -> ConfigurableContractArtifact {
        let bytecode = CompactBytecode {
            object: BytecodeObject::Bytecode(Bytes::from_static(&[0x60, 0x80, 0x60, 0x40])),
            source_map: None,
            link_references: BTreeMap::new(),
        };
        let deployed_bytecode = CompactDeployedBytecode {
            bytecode: Some(CompactBytecode {
                object: BytecodeObject::Bytecode(Bytes::from_static(&[0xfe])),
                source_map: None,
                link_references: BTreeMap::new(),
            }),
            immutable_references: BTreeMap::new(),
        };

        let mut sources = BTreeMap::new();
        sources.insert(
            "Foo.sol".to_string(),
            MetadataSource {
                keccak256: String::new(),
                urls: vec![],
                content: Some("contract Foo {}".to_string()),
                license: None,
            },
        );

        let metadata = FoundryMeta {
            compiler: Compiler { version: "0.8.20+commit.a1b79de6".to_string() },
            language: "Solidity".to_string(),
            output: Output { abi: vec![], devdoc: None, userdoc: None },
            settings: MetadataSettings {
                remappings: vec![],
                optimizer: Optimizer { enabled: Some(true), runs: Some(200), details: None },
                metadata: None,
                compilation_target: BTreeMap::new(),
                evm_version: None,
                libraries: BTreeMap::new(),
                via_ir: None,
            },
            sources: MetadataSources { inner: sources },
            version: 1,
        };

        ConfigurableContractArtifact {
            abi: Some(alloy_json_abi::JsonAbi::default()),
            bytecode: Some(bytecode),
            deployed_bytecode: Some(deployed_bytecode),
            metadata: Some(metadata),
            id: Some(7),
            ..Default::default()
        }
    }

    fn sample_artifact_id() -> ArtifactId {
        ArtifactId {
            path: PathBuf::from("out/Foo.sol/Foo.json"),
            name: "Foo".to_string(),
            source: PathBuf::from("src/Foo.sol"),
            version: Version::new(0, 8, 20),
            build_id: "stub".to_string(),
            profile: "default".to_string(),
        }
    }

    #[test]
    fn lifts_preserves_contract_name_and_version() {
        let art = sample_foundry_artifact();
        let id = sample_artifact_id();
        let lifted = Artifact::from_foundry(&id, &art).unwrap();
        assert_eq!(lifted.contract_name(), id.name);
        assert!(
            lifted.compiler_version().contains("0.8.20"),
            "expected compiler_version to contain 0.8.20, got {:?}",
            lifted.compiler_version()
        );
    }

    #[test]
    fn lifts_preserves_abi_when_present() {
        let art = sample_foundry_artifact();
        let id = sample_artifact_id();
        let lifted = Artifact::from_foundry(&id, &art).unwrap();
        // The contract entry should carry the ABI through to output.contracts.
        let contract = lifted.contract().expect("contract entry should exist");
        assert!(contract.abi.is_some(), "abi should be carried through");
        // And the Etherscan-style meta.abi should be a JSON-encoded array.
        assert!(
            lifted.meta.abi.starts_with('['),
            "meta.abi should be a JSON-encoded array, got {:?}",
            lifted.meta.abi
        );
    }

    #[test]
    fn lifts_carries_sources_and_bytecode_into_output() {
        let art = sample_foundry_artifact();
        let id = sample_artifact_id();
        let lifted = Artifact::from_foundry(&id, &art).unwrap();

        // SolcInput.sources should contain the lifted "Foo.sol" entry.
        let has_foo = lifted.input.sources.keys().any(|p| p.ends_with("Foo.sol"));
        assert!(
            has_foo,
            "input sources should contain Foo.sol, got {:?}",
            lifted.input.sources.keys().collect::<Vec<_>>()
        );

        // CompilerOutput.contracts[id.source][id.name] should exist with evm.bytecode populated.
        let by_file =
            lifted.output.contracts.get(&id.source).expect("output.contracts keyed by id.source");
        let contract = by_file.get(&id.name).expect("contract under id.name");
        let evm = contract.evm.as_ref().expect("evm should be Some");
        assert!(evm.bytecode.is_some(), "creation bytecode should be lifted");
        assert!(evm.deployed_bytecode.is_some(), "deployed bytecode should be lifted");

        // output.sources should have the SourceFile under id.source.
        assert!(lifted.output.sources.contains_key(&id.source));
    }
}
