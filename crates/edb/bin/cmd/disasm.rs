use std::{path::PathBuf, time::Duration};

use alloy_primitives::Address;
use alloy_provider::Provider;
use clap::{Parser, Subcommand};
use edb_backend::{
    analysis::source_map::SourceMapAnalysis,
    artifact::deploy::{DeployArtifact, DeployArtifactBuilder},
    utils::opcode,
    AnalyzedBytecode,
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
    pub async fn disasm(self) -> Result<()> {
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
        let bytecode = provider.get_code_at(address).await?;
        ensure!(!bytecode.is_empty(), "empty bytecode");
        let code = AnalyzedBytecode::new(&bytecode);

        // Step 4: fetch the source code if available.
        let compiler = OnchainCompiler::new(cache_path.compiler_chain_cache_dir(chain))?;
        if let Some((metadata, sources, compiler_output)) =
            compiler.compile(&etherscan, address).await?
        {
            let artifact = DeployArtifactBuilder {
                input_sources: sources,
                compilation_output: compiler_output,
                explorer_meta: metadata,
                onchain_bytecode: Bytecode::new_raw(bytecode),
                onchain_address: address,
            }
            .build()?;
            disasm_artifact(&artifact, Some(code))?;
        } else {
            // If the contract is not verified, we can't fetch the source code.
            // We can still disassemble the bytecode.
            disasm_bytecode(&code)?;
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
    pub async fn disasm(self) -> Result<()> {
        unimplemented!("Dump debug info from local project");
    }
}

impl DisasmArgs {
    pub async fn run(self) -> Result<()> {
        match self.mode {
            DisasmMode::OnChain(args) => args.disasm().await,
            DisasmMode::Local(args) => args.disasm().await,
        }
    }
}

fn disasm_artifact(artifact: &DeployArtifact, code: Option<AnalyzedBytecode>) -> Result<()> {
    // XXX (ZZ): For now, we only disassemble the deployed bytecode.
    let code = if let Some(code) = code {
        code
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

    let [_, deployed_source_map] = SourceMapAnalysis::analyze(artifact)?;
    debug_assert!(deployed_source_map.is_deployed());

    let source_map = &deployed_source_map.source_map;
    let labels = &deployed_source_map.labels;

    println!("{:5} ({:5}): {:<84} | {:<20} | Refined Label\n", "IC", "PC", "Opcode", "Source Map");

    for (ic, (src, label)) in source_map.iter().zip(labels.iter()).enumerate() {
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

        println!("{:05} ({:05}): {:<84} | {:<20} | {}", ic, pc, opcode_str, src.to_string(), label);
    }

    Ok(())
}

fn disasm_bytecode(code: &AnalyzedBytecode) -> Result<()> {
    println!("{:5} ({:5}): {:<84}\n", "IC", "PC", "Opcode",);

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

        println!("{ic:05} ({pc:05}): {opcode_str:<84}");
    }

    Ok(())
}
