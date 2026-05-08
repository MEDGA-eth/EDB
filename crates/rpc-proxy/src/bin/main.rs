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

//! EDB RPC Proxy Server
//!
//! A caching RPC proxy server that sits between EDB components and real Ethereum RPC endpoints.
//! Provides intelligent caching of immutable RPC responses to improve performance and reduce
//! network overhead for multiple debugging sessions.

use clap::{Parser, Subcommand};
use edb_common::{init_file_only_logging, init_logging};
use eyre::Result;
use std::{
    net::{IpAddr, SocketAddr},
    str::FromStr,
};
use tracing::{info, warn};

use edb_rpc_proxy::{proxy, tui};

use proxy::ProxyServerBuilder;

/// EDB RPC Caching Proxy Server
#[derive(Parser, Debug)]
#[command(name = "edb-rpc-proxy")]
#[command(about = "EDB RPC Caching Proxy Server")]
#[command(version)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

/// Available commands
#[derive(Subcommand, Debug)]
enum Commands {
    /// Start RPC proxy server
    Server(ServerArgs),
    /// Monitor existing proxy via TUI
    Monitor(MonitorArgs),
}

/// Server mode arguments
#[derive(Parser, Debug)]
struct ServerArgs {
    // ========== General Configuration ==========
    /// Address to bind to
    /// Example: --host 0.0.0.0
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to listen on
    #[arg(long, default_value = "8546")]
    port: u16,

    /// Upstream RPC URLs (comma-separated, overrides defaults if provided)
    /// Example: --rpc-urls "https://eth.llamarpc.com,https://rpc.ankr.com/eth"
    #[arg(long)]
    rpc_urls: Option<String>,

    // ========== Cache Configuration ==========
    /// Maximum number of cached items
    #[arg(long, default_value = "1024000")]
    max_cache_items: u32,

    /// Cache directory (default: ~/.edb/cache/rpc/<chain_id>)
    #[arg(long)]
    cache_dir: Option<String>,

    /// Cache save interval in minutes (0 = save only on shutdown)
    #[arg(long, default_value = "15")]
    cache_save_interval: u64,

    // ========== Provider Health Check Configuration ==========
    /// Maximum consecutive failures before marking provider unhealthy
    #[arg(long, default_value = "3")]
    max_failures: u32,

    /// Provider health check interval in seconds
    #[arg(long, default_value = "60")]
    health_check_interval: u64,

    // ========== EDB Registry Configuration ==========
    /// Grace period in seconds before shutdown when no EDB instances (0 = no auto-shutdown)
    #[arg(long, default_value = "0")]
    grace_period: u64,

    /// Heartbeat check interval in seconds
    #[arg(long, default_value = "10")]
    heartbeat_interval: u64,

    // ========== UI Configuration ==========
    /// Enable TUI monitoring interface
    #[arg(long)]
    tui: bool,
}

/// Monitor mode arguments
#[derive(Parser, Debug)]
struct MonitorArgs {
    /// Proxy URL to monitor
    proxy_url: String,

    /// Refresh interval in milliseconds
    #[arg(long, default_value = "250")] // 4 FPS
    refresh_interval: u64,

    /// Connection timeout in seconds
    #[arg(long, default_value = "5")]
    timeout: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Commands::Server(server_args) => run_server(server_args).await,
        Commands::Monitor(monitor_args) => run_monitor(monitor_args).await,
    }
}

/// Run the RPC proxy server
async fn run_server(args: ServerArgs) -> Result<()> {
    if args.tui {
        // Initilize file-only logging
        let log_file_path = init_file_only_logging("edb-rpc-proxy")?;

        // Print log file location to stderr (so user knows where logs are)
        // Use stderr so it doesn't interfere with TUI if there are issues
        eprintln!("EDB RPC-PROXY TUI logs: {}", log_file_path.display());
    } else {
        // Initialize logging
        init_logging("edb-rpc-proxy", true)?;
    }

    // Create the proxy server using builder pattern
    let mut builder = ProxyServerBuilder::new()
        .max_cache_items(args.max_cache_items)
        .grace_period(args.grace_period)
        .heartbeat_interval(args.heartbeat_interval)
        .max_failures(args.max_failures)
        .health_check_interval(args.health_check_interval)
        .cache_save_interval(args.cache_save_interval);

    // Set RPC URLs if provided
    if let Some(urls) = args.rpc_urls {
        builder = builder.rpc_urls_str(&urls);
    }

    // Set cache directory if provided
    if let Some(cache_dir) = args.cache_dir {
        builder = builder.cache_dir(cache_dir);
    }

    let proxy = builder.build().await?;

    // Set up graceful shutdown
    let cache_manager = proxy.cache_manager().clone();

    // Start the server
    let ip = IpAddr::from_str(&args.host)?;
    let addr = SocketAddr::from((ip, args.port));

    if args.tui {
        // TUI mode - start TUI interface
        info!("Starting TUI monitoring interface...");

        // Start proxy server in background
        let proxy_clone = proxy.clone();
        let server_handle = tokio::spawn(async move { proxy_clone.serve(addr).await });

        // Wait for server to start
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Start TUI interface (now remote-based)
        let proxy_url = format!("http://{addr}");
        let tui_result = tui::run_tui(proxy_url, 250, 5).await;

        // Cleanup
        server_handle.abort();

        tui_result?;
    } else {
        // Standard mode - run as daemon
        info!("Starting EDB RPC Proxy on {}", addr);

        // Set up shutdown signal handling
        tokio::select! {
            result = proxy.serve(addr) => {
                result?;
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Received shutdown signal");
            }
        }
    }

    // Save cache to disk before exiting
    if let Err(e) = cache_manager.save_to_disk().await {
        warn!("Failed to save cache to disk: {}", e);
    }

    Ok(())
}

/// Run the TUI monitor for an existing proxy
async fn run_monitor(args: MonitorArgs) -> Result<()> {
    // Initilize file-only logging
    let log_file_path = init_file_only_logging("edb-rpc-proxy")?;

    // Print log file location to stderr (so user knows where logs are)
    // Use stderr so it doesn't interfere with TUI if there are issues
    eprintln!("EDB RPC-PROXY TUI logs: {}", log_file_path.display());

    info!("Starting TUI monitor for proxy at {}", args.proxy_url);

    // Create a remote TUI client and run it
    tui::run_tui(args.proxy_url, args.refresh_interval, args.timeout).await
}
