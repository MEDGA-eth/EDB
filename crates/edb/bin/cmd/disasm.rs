use std::path::PathBuf;

use alloy_primitives::Address;
use alloy_provider::Provider;
use clap::{Parser, Subcommand};
use edb_backend::AnalyzedBytecode;
use edb_utils::{
    cache::{CachePath, EDBCache},
    onchain_compiler::OnchainCompiler,
};
use eyre::{ensure, Result};

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
        let OnChainArgs { address, cache, etherscan, rpc } = self;

        // Step 1: build the RPC provider.
        let provider = rpc.provider(true)?;
        ensure!(
            provider.get_chain_id().await? == etherscan.chain.unwrap_or_default().id(),
            "inconsistent chain id"
        );

        // Step 2: fetch the contract bytecode.
        let bytecode = provider.get_code_at(address).await?;
        ensure!(!bytecode.is_empty(), "empty bytecode");
        let code = AnalyzedBytecode::new(&bytecode);

        // Step 3: fetch the source code if available.
        let chain_id = etherscan.chain.unwrap_or_default().id();
        let cache_path = cache.cache_path();
        let compiler =
            OnchainCompiler::new(cache_path.and_then(|p| p.backend_chain_cache_dir(chain_id)))?;

        unimplemented!("Dump debug info from on-chain contract");
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
