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

//! Health check service for proxy status

use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

/// Health check service for monitoring proxy status
///
/// Provides health check endpoints that return information about
/// the proxy server's status, uptime, and configuration.
pub struct HealthService {
    start_time: u64,
}

impl Default for HealthService {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthService {
    /// Creates a new health service instance
    ///
    /// Records the current time as the service start time for uptime calculations.
    ///
    /// # Returns
    /// A new HealthService instance
    pub fn new() -> Self {
        let start_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        Self { start_time }
    }

    /// Returns a simple ping response with current status
    ///
    /// # Returns
    /// JSON-RPC response with status and timestamp
    pub async fn ping(&self) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "status": "ok",
                "service": "edb-rpc-proxy",
                "timestamp": SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            }
        })
    }

    /// Returns detailed information about the proxy service
    ///
    /// # Returns
    /// JSON-RPC response with service version, uptime, and process information
    pub async fn info(&self) -> Value {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "service": "edb-rpc-proxy",
                "version": env!("CARGO_PKG_VERSION"),
                "uptime": now - self.start_time,
                "started_at": self.start_time,
                "pid": std::process::id()
            }
        })
    }
}
