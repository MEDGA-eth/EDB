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

//! RPC Proxy management for EDB

use eyre::{Result, eyre};
use serde_json::{Value, json};
use std::{
    process::{Command, Stdio},
    time::Duration,
};
use tokio::time::sleep;
use tracing::{debug, info, warn};

const PROXY_HEARTBEAT_INTERVAL: u64 = 10;
const PROXY_GRACE_PERIOD: u64 = 30;

pub async fn ensure_proxy_running(
    rpc_urls: &str,
    proxy_port: u16,
    disable_cache: bool,
) -> Result<()> {
    // Check if proxy already exists and is healthy
    match proxy_health_check(proxy_port).await {
        Ok(_) => {
            info!("Found healthy proxy at port {}", proxy_port);
            register_with_proxy(proxy_port).await?;
            start_heartbeat_task(proxy_port, PROXY_HEARTBEAT_INTERVAL);
            return Ok(());
        }
        Err(e) => {
            debug!("Proxy health check failed: {}", e);
        }
    }

    // Spawn new one
    info!("No healthy proxy found, spawning new instance");
    spawn_proxy(rpc_urls, proxy_port, disable_cache).await?;

    // Wait for proxy to be ready
    wait_for_proxy_ready(proxy_port).await?;

    // Register with the new proxy
    register_with_proxy(proxy_port).await?;

    // Start heartbeat task
    start_heartbeat_task(proxy_port, PROXY_HEARTBEAT_INTERVAL);

    Ok(())
}

async fn proxy_health_check(port: u16) -> Result<Value> {
    let client = reqwest::Client::new();
    let request = json!({
        "jsonrpc": "2.0",
        "method": "edb_ping",
        "params": [],
        "id": 1
    });

    let response = client
        .post(format!("http://127.0.0.1:{port}"))
        .json(&request)
        .timeout(Duration::from_secs(2))
        .send()
        .await?;

    let response_json: Value = response.json().await?;

    if response_json.get("error").is_some() {
        return Err(eyre!("Proxy returned error: {:?}", response_json["error"]));
    }

    Ok(response_json)
}

async fn register_with_proxy(port: u16) -> Result<()> {
    let client = reqwest::Client::new();
    let pid = std::process::id();
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();

    let request = json!({
        "jsonrpc": "2.0",
        "method": "edb_register",
        "params": [pid, timestamp],
        "id": 1
    });

    let response = client
        .post(format!("http://127.0.0.1:{port}"))
        .json(&request)
        .timeout(Duration::from_secs(5))
        .send()
        .await?;

    let response_json: Value = response.json().await?;

    if let Some(error) = response_json.get("error") {
        return Err(eyre!("Failed to register with proxy: {:?}", error));
    }

    info!("Successfully registered with proxy (PID: {})", pid);
    Ok(())
}

fn start_heartbeat_task(port: u16, interval: u64) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let pid = std::process::id();
        let mut interval_timer = tokio::time::interval(Duration::from_secs(interval));

        loop {
            interval_timer.tick().await;

            let request = json!({
                "jsonrpc": "2.0",
                "method": "edb_heartbeat",
                "params": [pid],
                "id": 1
            });

            if let Err(e) = client
                .post(format!("http://127.0.0.1:{port}"))
                .json(&request)
                .timeout(Duration::from_secs(5))
                .send()
                .await
            {
                warn!("Heartbeat failed: {}", e);
            } else {
                debug!("Sent heartbeat to proxy");
            }
        }
    });
}

async fn spawn_proxy(rpc_urls: &str, proxy_port: u16, disable_cache: bool) -> Result<()> {
    let proxy_binary = find_proxy_binary()?;

    info!("Spawning proxy binary: {:?}", proxy_binary);

    #[cfg(unix)]
    {
        use std::{env, os::unix::process::CommandExt};

        let mut args = vec![
            "server".to_string(), // Add the server subcommand
            "--port".to_string(),
            proxy_port.to_string(),
            "--grace-period".to_string(),
            PROXY_GRACE_PERIOD.to_string(),
            "--heartbeat-interval".to_string(),
            PROXY_HEARTBEAT_INTERVAL.to_string(),
        ];

        // Add RPC URLs
        args.push("--rpc-urls".to_string());
        args.push(rpc_urls.to_string());

        // If cache is disabled, set the max cache items to 0
        if disable_cache {
            args.push("--max-cache-items".to_string());
            args.push("0".to_string());
        }

        // If cache directory is specified, add it as well
        if let Ok(cache_dir) = env::var(edb_common::env::EDB_CACHE_DIR) {
            args.push("--cache-dir".to_string());
            args.push(cache_dir);
        }

        debug!(
            "Invoking proxy with command: {} {:?}",
            proxy_binary.as_os_str().to_string_lossy(),
            args
        );
        Command::new(&proxy_binary)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0) // Create new process group (detached)
            .spawn()
            .map_err(|e| eyre!("Failed to spawn proxy: {}", e))?;
    }

    #[cfg(windows)]
    {
        use std::{env, os::windows::process::CommandExt};

        let mut args = vec![
            "server".to_string(), // Add the server subcommand
            "--port".to_string(),
            proxy_port.to_string(),
            "--grace-period".to_string(),
            PROXY_GRACE_PERIOD.to_string(),
            "--heartbeat-interval".to_string(),
            PROXY_HEARTBEAT_INTERVAL.to_string(),
        ];

        // Add RPC URLs
        args.push("--rpc-urls".to_string());
        args.push(rpc_urls.to_string());

        // If cache is disabled, set the max cache items to 0
        if disable_cache {
            args.push("--max-cache-items".to_string());
            args.push("0".to_string());
        }

        // If cache directory is specified, add it as well
        if let Ok(cache_dir) = env::var(edb_common::env::EDB_CACHE_DIR) {
            args.push("--cache-dir".to_string());
            args.push(cache_dir);
        }

        Command::new(&proxy_binary)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x00000008) // DETACHED_PROCESS
            .spawn()
            .map_err(|e| eyre!("Failed to spawn proxy: {}", e))?;
    }

    info!("Proxy process spawned successfully");
    Ok(())
}

fn find_proxy_binary() -> Result<std::path::PathBuf> {
    crate::utils::find_proxy_binary()
}

async fn wait_for_proxy_ready(port: u16) -> Result<()> {
    let max_attempts = 120; // 120 seconds total

    for attempt in 1..=max_attempts {
        match proxy_health_check(port).await {
            Ok(_) => {
                info!("Proxy is ready on port {}", port);
                return Ok(());
            }
            Err(e) => {
                debug!("Proxy not ready (attempt {}/{}): {}", attempt, max_attempts, e);

                if attempt == max_attempts {
                    return Err(eyre!("Proxy failed to start within {} seconds", max_attempts));
                }

                sleep(Duration::from_secs(1)).await;
            }
        }
    }

    unreachable!()
}
