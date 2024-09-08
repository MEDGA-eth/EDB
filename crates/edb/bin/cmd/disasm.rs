use std::{path::PathBuf, time::Duration};

use alloy_primitives::Address;
use alloy_provider::Provider;
use clap::{Parser, Subcommand};
use edb_backend::{
    analysis::source_map::{debug_unit::DebugUnit, source_label::SourceLabel, SourceMapAnalysis},
    artifact::{
        deploy::{DeployArtifact, DeployArtifactBuilder},
        onchain::AnalyzedBytecode,
    },
    utils::opcode,
};
use edb_utils::{
    cache::{CachePath, DEFAULT_ETHERSCAN_CACHE_TTL},
    onchain_compiler::OnchainCompiler,
};
use eyre::{ensure, OptionExt, Result};
use revm::{
    interpreter::{opcode::PUSH0, OpCode},
    primitives::Bytecode,
};

use crate::opts::{CacheOpts, EtherscanOpts, RpcOpts};

#[derive(Clone, Debug, Parser)]
pub struct DisasmArgs {
    /// Refine the source labels.
    #[clap(long)]
    refine: bool,

    /// Dedisassembly the constructor instead of the deployed bytecode.
    #[clap(long)]
    constructor: bool,

    #[command(subcommand)]
    mode: DisasmMode,
}

#[derive(Clone, Debug, Subcommand)]
enum DisasmMode {
    /// Dump the debug information from an on-chain contract.
    #[clap(name = "on-chain")]
    OnChain(OnChainArgs),

    /// Dump the debug information from the local project.
    #[clap(name = "local")]
    Local(LocalArgs),
}

#[derive(Clone, Debug, Parser)]
struct OnChainArgs {
    /// The address of the contract.
    #[clap(long, short)]
    address: Address,

    #[command(flatten)]
    pub cache: CacheOpts,

    #[command(flatten)]
    pub etherscan: EtherscanOpts,

    #[command(flatten)]
    pub rpc: RpcOpts,
}

impl OnChainArgs {
    pub async fn disasm(self, refine: bool, constructor: bool) -> Result<()> {
        let Self { address, cache, etherscan, rpc } = self;

        let cache_path = cache.cache_path();
        let chain = etherscan.chain();

        // Step 1: build the RPC provider.
        let provider = rpc.provider(true)?;
        ensure!(
            provider.get_chain_id().await? == etherscan.chain.unwrap_or_default().id(),
            "inconsistent chain id"
        );

        // Step 2: build the etherscan client.
        let mut etherscan = etherscan.client()?;
        if let Some(etherscan_cache) = cache_path.etherscan_chain_cache_dir(chain) {
            etherscan.set_cache(etherscan_cache, Duration::from_secs(DEFAULT_ETHERSCAN_CACHE_TTL));
        };

        // Step 3: fetch the contract bytecode.
        let deployed_bytecode = provider.get_code_at(address).await?;
        ensure!(!deployed_bytecode.is_empty(), "empty bytecode");
        let deployed_code = AnalyzedBytecode::new(&deployed_bytecode);

        // Step 4: fetch the source code if available.
        let compiler = OnchainCompiler::new(cache_path.compiler_chain_cache_dir(chain))?;
        if let Some((metadata, sources, compiler_output)) =
            compiler.compile(&etherscan, address).await?
        {
            let artifact = DeployArtifactBuilder {
                input_sources: sources,
                compilation_output: compiler_output,
                explorer_meta: metadata,
                onchain_bytecode: Bytecode::new_raw(deployed_bytecode),
                onchain_address: address,
            }
            .build()?;
            disasm_artifact(
                &artifact,
                // We do not fetch the constructor bytecode from etherscan.
                if constructor { None } else { Some(deployed_code) },
                refine,
                constructor,
            )?;
        } else {
            // If the contract is not verified, we can't fetch the source code.
            // We can still disassemble the bytecode.
            disasm_bytecode(&deployed_code)?;
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Parser)]
struct LocalArgs {
    /// The name of the contract.
    name: String,

    /// The path to the project. If not provided, the current directory is used.
    #[clap(long)]
    path: Option<PathBuf>,
}

impl LocalArgs {
    pub async fn disasm(self, _refine: bool, _constructor: bool) -> Result<()> {
        unimplemented!("Dump debug info from local project");
    }
}

impl DisasmArgs {
    pub async fn run(self) -> Result<()> {
        match self.mode {
            DisasmMode::OnChain(args) => args.disasm(self.refine, self.constructor).await,
            DisasmMode::Local(args) => args.disasm(self.refine, self.constructor).await,
        }
    }
}

fn disasm_artifact(
    artifact: &DeployArtifact,
    onchain_code: Option<AnalyzedBytecode>,
    refine: bool,
    constructor: bool,
) -> Result<()> {
    let code = if let Some(code) = onchain_code {
        code
    } else if constructor {
        AnalyzedBytecode::new(
            artifact
                .constructor_bytecode()
                .ok_or_eyre("invalid compiled bytecode")?
                .bytes()
                .ok_or_eyre("cannot convert compiled bytecode into bytes")?
                .as_ref(),
        )
    } else {
        AnalyzedBytecode::new(
            artifact
                .deployed_bytecode()
                .ok_or_eyre("invalid compiled bytecode")?
                .bytes()
                .ok_or_eyre("cannot convert compiled bytecode into bytes")?
                .as_ref(),
        )
    };

    let [mut constructor_source_map, mut deployed_source_map] =
        SourceMapAnalysis::analyze(artifact)?;
    debug_assert!(constructor_source_map.is_constructor());
    debug_assert!(deployed_source_map.is_deployed());

    let analyze_source_map =
        if constructor { &mut constructor_source_map } else { &mut deployed_source_map };

    let source_map = &analyze_source_map.source_map;
    let labels = &mut analyze_source_map.labels;
    if refine {
        labels.refine(&code)?;
    }

    println!(
        "{:5} ({:5}): {:<85} | {:<72} | Comments\n",
        "IC", "PC", "Opcode", "Refined Label"
    );

    for (ic, (_, label)) in source_map.iter().zip(labels.iter()).enumerate() {
        let pc = code.ic_pc_map.get(ic).ok_or_eyre(format!("no pc found at {ic}"))?;
        let opcode =
            OpCode::new(code.code[pc]).ok_or_eyre(format!("invalid opcode: {}", code.code[pc]))?;

        let opcode_str = if opcode.is_push() {
            format!(
                "PUSH{} {}",
                code.code[pc] - PUSH0,
                opcode::get_push_value(code.code.as_ref(), pc)?
            )
        } else {
            format!("{opcode}")
        };

        let comments = match label {
            SourceLabel::PrimitiveStmt { stmt: DebugUnit::Primitive(_, meta), .. } => {
                meta.to_string()
            }
            SourceLabel::Tag { tag, .. } => match tag {
                DebugUnit::Function(_, meta) => meta.to_string(),
                DebugUnit::Contract(_, meta) => meta.to_string(),
                DebugUnit::Primitive(_, meta) => meta.to_string(),
                _ => String::new(),
            },
            _ => String::new(),
        };

        println!(
            "{:05} ({:05}): {:<85} | {:<72} | {}",
            ic,
            pc,
            opcode_str,
            label.to_string(),
            comments
        );
    }

    Ok(())
}

fn disasm_bytecode(code: &AnalyzedBytecode) -> Result<()> {
    println!("{:5} ({:5}): {:<85}\n", "IC", "PC", "Opcode",);

    for (pc, value) in code.code.iter().enumerate() {
        let Some(ic) = code.pc_ic_map.get(pc) else {
            continue;
        };

        let opcode = OpCode::new(*value).ok_or_eyre(format!("invalid opcode: {value}"))?;

        let opcode_str = if opcode.is_push() {
            format!("PUSH{} {}", *value - PUSH0, opcode::get_push_value(code.code.as_ref(), pc)?)
        } else {
            format!("{opcode}")
        };

        println!("{ic:05} ({pc:05}): {opcode_str:<85}");
    }

    Ok(())
}
