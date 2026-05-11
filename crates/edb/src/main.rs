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
use clap::Parser;
use eyre::Result;

use edb::{Cli, Commands, cmd, proxy};

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
