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
use edb_engine::EngineConfig;
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
    /// Upstream RPC URLs (comma-separated, overrides defaults if provided).
    ///
    /// Example: --rpc-urls "https://eth.llamarpc.com,https://rpc.ankr.com/eth"
    #[arg(long)]
    rpc_urls: Option<String>,

    /// Port for the RPC proxy server
    #[arg(long, default_value = "8546")]
    pub proxy_port: u16,

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

impl Cli {
    /// Validate CLI arguments and warn about misused options
    pub fn validate(&self) {
        // Warn if TUI-specific options are used with non-TUI configurations.
        // Note: --ui no longer triggers a warning when given to a non-UI
        // subcommand (server, proxy-status) — those subcommands ignore the
        // flag silently, and after the Web-default flip, the previous
        // warning would fire on every default invocation.
        let runs_ui = self.command.runs_ui();
        if self.tui_options.disable_mouse && !(runs_ui && self.ui == Ui::Tui) {
            tracing::warn!("--disable-mouse only applies to --ui=tui");
            eprintln!("Warning: --disable-mouse only applies to --ui=tui");
        }
    }

    /// Derive EDB engine configuration from CLI arguments
    pub fn to_engine_config(&self, rpc_url: &str) -> EngineConfig {
        let mut engine_config = EngineConfig::default()
            .with_quick_mode(self.quick)
            .with_rpc_proxy_url(rpc_url.to_string());
        if let Some(api_key) = &self.etherscan_api_key {
            engine_config = engine_config.with_etherscan_api_key(api_key.clone());
        }
        engine_config
    }
}

/// Available commands
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Replay an existing transaction
    Replay {
        /// Transaction hash to replay
        tx_hash: String,
    },
    /// Debug a Foundry test case
    Test {
        /// Test name to debug
        test_name: String,

        /// Block number to fork at (default: latest)
        block: Option<u64>,
    },
    /// Start WebSocket server for remote debugging sessions
    Server {
        /// Port for the WebSocket server
        #[arg(long, default_value = "9001")]
        ws_port: u16,
    },
    /// Show RPC proxy provider status
    ProxyStatus,
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

    // Validate CLI arguments
    cli.validate();

    if let Some(cache_dir) = &cli.cache_dir {
        tracing::info!("Using cache directory: {cache_dir}");
        // SAFETY: edition 2024 marks env::set_var as unsafe (RFC 3445); this runs at startup
        // before any threads that read the env have been spawned.
        unsafe {
            env::set_var(edb_common::env::EDB_CACHE_DIR, cache_dir);
        }
    }

    // Set up RPC endpoint (proxy or direct)
    let effective_rpc_url = {
        tracing::info!("Ensuring RPC proxy is running...");
        proxy::ensure_proxy_running(&cli).await?;
        format!("http://127.0.0.1:{}", cli.proxy_port)
    };

    tracing::info!("Using RPC endpoint: {}", effective_rpc_url);

    // Execute the command to get RPC server handle
    match &cli.command {
        Commands::Replay { tx_hash } => {
            tracing::info!("Replaying transaction: {}", tx_hash);
            let tx_hash: TxHash = tx_hash.parse()?;
            cmd::replay_transaction(tx_hash, &cli, &effective_rpc_url).await
        }
        Commands::Test { test_name, block } => {
            tracing::info!("Debugging test: {}", test_name);
            cmd::debug_foundry_test(test_name, *block, &cli, &effective_rpc_url).await
        }
        Commands::Server { ws_port } => {
            tracing::info!("Starting WebSocket server on port {}", ws_port);
            cmd::start_server(*ws_port, &cli, &effective_rpc_url).await
        }
        Commands::ProxyStatus => cmd::show_proxy_status(&cli).await,
    }
}
