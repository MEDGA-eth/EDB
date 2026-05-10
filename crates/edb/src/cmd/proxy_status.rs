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

//! Show proxy status command

use eyre::Result;
use serde_json::json;
use std::time::Duration;

/// Show the status of RPC proxy providers
pub async fn show_proxy_status(proxy_port: u16) -> Result<()> {
    tracing::info!("Checking proxy status...");

    // Query provider status
    let client = reqwest::Client::new();
    let request = json!({
        "jsonrpc": "2.0",
        "method": "edb_providers",
        "params": [],
        "id": 1
    });

    let response = client
        .post(format!("http://127.0.0.1:{proxy_port}"))
        .json(&request)
        .timeout(Duration::from_secs(5))
        .send()
        .await?;

    let response_json: serde_json::Value = response.json().await?;

    if let Some(error) = response_json.get("error") {
        println!("❌ Error getting proxy status: {error}");
        return Ok(());
    }

    if let Some(result) = response_json.get("result") {
        let healthy_count = result["healthy_count"].as_u64().unwrap_or(0);
        let total_count = result["total_count"].as_u64().unwrap_or(0);
        let empty_providers = vec![];
        let providers = result["providers"].as_array().unwrap_or(&empty_providers);

        println!("🌐 EDB RPC Proxy Status");
        println!("=====================");
        println!("📊 Provider Summary: {healthy_count}/{total_count} healthy");
        println!();

        for (i, provider) in providers.iter().enumerate() {
            let url = provider["url"].as_str().unwrap_or("unknown");
            let is_healthy = provider["is_healthy"].as_bool().unwrap_or(false);
            let response_time = provider["response_time_ms"].as_u64();
            let failures = provider["consecutive_failures"].as_u64().unwrap_or(0);
            let last_check = provider["last_health_check_seconds_ago"].as_u64();

            let status_emoji = if is_healthy { "✅" } else { "❌" };
            let status_text = if is_healthy { "Healthy" } else { "Unhealthy" };

            println!("{}. {} {}", i + 1, status_emoji, status_text);
            println!("   URL: {url}");

            if let Some(rt) = response_time {
                println!("   Response Time: {rt}ms");
            }

            if failures > 0 {
                println!("   Failures: {failures}");
            }

            if let Some(last) = last_check {
                if last < 60 {
                    println!("   Last Check: {last}s ago");
                } else if last < 3600 {
                    println!("   Last Check: {}m ago", last / 60);
                } else {
                    println!("   Last Check: {}h ago", last / 3600);
                }
            }
            println!();
        }

        if healthy_count == 0 {
            println!("⚠️  Warning: No healthy providers available!");
            println!("   The proxy will attempt to health-check providers automatically.");
        } else if healthy_count < total_count {
            println!("⚠️  Some providers are unhealthy but {healthy_count} are still working.");
        } else {
            println!("✨ All providers are healthy!");
        }
    } else {
        println!("❌ Unexpected response format from proxy");
    }

    Ok(())
}
