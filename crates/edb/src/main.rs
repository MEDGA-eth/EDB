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

//! EDB - Ethereum Debugger
//!
//! A step-by-step debugger for Ethereum transactions.

use std::env;

use alloy_primitives::TxHash;
use clap::{Parser, Subcommand, ValueEnum};
use eyre::Result;

use crate::utils::TuiOptions;

mod cmd;
mod proxy;
mod utils;
mod ws_protocol;

/// Command-line interface for EDB
#[derive(Debug, Parser)]
#[command(name = "edb")]
#[command(
    about = "Ethereum Debugger - Source-level time-travel debugger for Ethereum smart contracts"
)]
#[command(version)]
pub struct Cli {
    /// Etherscan API key for source code download
    #[arg(long, env = "ETHERSCAN_API_KEY")]
    pub etherscan_api_key: Option<String>,

    /// Quick mode - skip replaying preceding transactions in the block
    #[arg(long)]
    pub quick: bool,

    /// Disable cache - do not use cached RPC responses
    #[arg(long)]
    pub disable_cache: bool,

    /// The cache directory
    #[arg(long, env = edb_common::env::EDB_CACHE_DIR)]
    pub cache_dir: Option<String>,

    /// TUI-specific options
    #[command(flatten)]
    pub tui_options: TuiOptions,

    /// User interface to launch after engine startup
    #[arg(long, value_enum, default_value = "web")]
    pub ui: Ui,

    /// Command to execute
    #[command(subcommand)]
    pub command: Commands,
}

/// Available commands
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Replay an existing transaction
    Replay {
        /// Transaction hash to replay
        tx_hash: String,
        /// Upstream RPC URLs (comma-separated)
        #[arg(long, required = true)]
        rpc_urls: String,
        /// Port for the RPC proxy server
        #[arg(long, default_value = "8546")]
        proxy_port: u16,
    },
    /// Debug a Foundry test case (CONTRACT::TEST_FN)
    Test {
        /// Qualified test identifier in the form `Contract::testFn`
        #[arg(value_name = "CONTRACT::TEST_FN")]
        target: String,
        /// Path to the foundry project root (defaults to walking up for foundry.toml)
        #[arg(long)]
        root: Option<String>,
        /// foundry.toml profile (defaults to FOUNDRY_PROFILE env or "default")
        #[arg(long)]
        profile: Option<String>,
        /// Upstream RPC URL for forking (defaults to foundry.toml eth_rpc_url)
        #[arg(long)]
        fork_url: Option<String>,
        /// Block number to fork at (defaults to `latest` when forking)
        #[arg(long)]
        fork_block_number: Option<u64>,
    },
    /// Start WebSocket server for remote debugging sessions
    Server {
        /// Port for the WebSocket server
        #[arg(long, default_value = "9001")]
        ws_port: u16,
        /// Upstream RPC URLs (comma-separated)
        #[arg(long, required = true)]
        rpc_urls: String,
        /// Port for the RPC proxy server
        #[arg(long, default_value = "8546")]
        proxy_port: u16,
    },
    /// Show RPC proxy provider status
    ProxyStatus {
        /// Port for the RPC proxy server to query
        #[arg(long, default_value = "8546")]
        proxy_port: u16,
    },
}

impl Commands {
    /// Whether the command launches a user interface (TUI or Web).
    pub fn runs_ui(&self) -> bool {
        matches!(self, Self::Replay { .. } | Self::Test { .. })
    }
}

/// Which user interface to use after engine startup
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Ui {
    /// Terminal UI
    Tui,
    /// Browser UI on the engine's port (default)
    Web,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenv::dotenv().ok();

    // Initialize logging
    edb_common::logging::init_logging("edb", true)?;

    // Parse CLI arguments
    let cli = Cli::parse();

    if let Some(cache_dir) = &cli.cache_dir {
        tracing::info!("Using cache directory: {cache_dir}");
        // SAFETY: edition 2024 marks env::set_var as unsafe (RFC 3445); this runs at startup
        // before any threads that read the env have been spawned.
        unsafe {
            env::set_var(edb_common::env::EDB_CACHE_DIR, cache_dir);
        }
    }

    match &cli.command {
        Commands::Replay { tx_hash, rpc_urls, proxy_port } => {
            tracing::info!("Replaying transaction: {}", tx_hash);
            let tx_hash: TxHash = tx_hash.parse()?;
            tracing::info!("Ensuring RPC proxy is running...");
            proxy::ensure_proxy_running(rpc_urls, *proxy_port, cli.disable_cache).await?;
            let effective_rpc_url = format!("http://127.0.0.1:{proxy_port}");
            cmd::replay_transaction(tx_hash, &cli, &effective_rpc_url).await
        }
        Commands::Test { target, root, profile, fork_url, fork_block_number } => {
            tracing::info!("Debugging test: {}", target);
            cmd::run_foundry_test(
                target,
                root.as_deref(),
                profile.as_deref(),
                fork_url.as_deref(),
                *fork_block_number,
                &cli,
            )
            .await
        }
        Commands::Server { ws_port, rpc_urls, proxy_port } => {
            tracing::info!("Starting WebSocket server on port {}", ws_port);
            tracing::info!("Ensuring RPC proxy is running...");
            proxy::ensure_proxy_running(rpc_urls, *proxy_port, cli.disable_cache).await?;
            let effective_rpc_url = format!("http://127.0.0.1:{proxy_port}");
            cmd::start_server(*ws_port, &cli, &effective_rpc_url).await
        }
        Commands::ProxyStatus { proxy_port } => cmd::show_proxy_status(*proxy_port).await,
    }
}
