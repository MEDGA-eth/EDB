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

//! Utility functions for RPC server operations.
//!
//! This module provides common utilities needed by the RPC server including:
//! - Port discovery and availability checking
//! - Socket address parsing with sensible defaults
//! - Error handling helpers for JSON-RPC responses
//! - Convenience functions for common error types
//!
//! # Port Management
//!
//! The module includes functions to find available ports automatically,
//! starting from a preferred port (3000) and searching upward if needed.
//!
//! # Error Handling
//!
//! Provides helper functions to create properly formatted JSON-RPC error
//! responses with standard error codes and descriptive messages.

use eyre::{Result, eyre};
use std::net::{SocketAddr, TcpListener};
use tracing::{debug, info};

const PORT_SCAN_WINDOW: u16 = 1024;

/// Find an available port starting from a base port
pub fn find_available_port(start_port: u16) -> Result<u16> {
    let end_port = start_port.saturating_add(PORT_SCAN_WINDOW);

    for port in start_port..=end_port {
        if is_port_available(port) {
            info!("Found available port: {}", port);
            return Ok(port);
        }
    }

    // Limit the linear scan to a small window so we do not burn through thousands of bind
    // attempts under test load before asking the OS for an ephemeral port.
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| {
        eyre!("No available port found in {}-{} or via OS fallback: {}", start_port, end_port, e)
    })?;
    let port = listener.local_addr()?.port();
    info!("Falling back to OS-assigned ephemeral port: {}", port);
    Ok(port)
}

/// Check if a port is available on localhost
pub fn is_port_available(port: u16) -> bool {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => {
            debug!("Port {} is available", port);
            true
        }
        Err(_) => {
            debug!("Port {} is not available", port);
            false
        }
    }
}

/// Get default RPC server port (tries 3000 first, then searches)
pub fn get_default_rpc_port() -> Result<u16> {
    if is_port_available(3000) { Ok(3000) } else { find_available_port(3001) }
}

/// Parse a socket address, with sensible defaults
pub fn parse_socket_addr(addr_str: Option<&str>, default_port: u16) -> Result<SocketAddr> {
    match addr_str {
        Some(addr) => addr.parse().map_err(|e| eyre!("Invalid socket address '{}': {}", addr, e)),
        None => Ok(SocketAddr::from(([127, 0, 0, 1], default_port))),
    }
}

/// Convert error to RPC error format
pub fn to_rpc_error(
    code: i32,
    message: &str,
    data: Option<serde_json::Value>,
) -> crate::rpc::types::RpcError {
    crate::rpc::types::RpcError { code, message: message.to_string(), data }
}

/// Helper to create internal error responses
pub fn internal_error(message: &str) -> crate::rpc::types::RpcError {
    to_rpc_error(crate::rpc::types::error_codes::INTERNAL_ERROR, message, None)
}

/// Helper to create method not found error
pub fn method_not_found(method: &str) -> crate::rpc::types::RpcError {
    to_rpc_error(
        crate::rpc::types::error_codes::METHOD_NOT_FOUND,
        &format!("Method '{method}' not found"),
        None,
    )
}

/// Helper to create invalid params error
pub fn invalid_params(message: &str) -> crate::rpc::types::RpcError {
    to_rpc_error(crate::rpc::types::error_codes::INVALID_PARAMS, message, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;

    fn local_bind_is_restricted(error: &std::io::Error) -> bool {
        matches!(error.kind(), ErrorKind::PermissionDenied) || error.raw_os_error() == Some(1)
    }

    #[test]
    fn test_port_availability() {
        let listener = match TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            Err(error) if local_bind_is_restricted(&error) => return,
            Err(error) => panic!("Should be able to probe local port binding: {error}"),
        };
        let start_port = listener.local_addr().unwrap().port();

        assert!(!is_port_available(start_port));
        drop(listener);

        let port = find_available_port(start_port).expect("Should find an available port");
        assert!(is_port_available(port));
    }

    #[test]
    fn test_socket_addr_parsing() {
        let addr = parse_socket_addr(Some("127.0.0.1:8080"), 3000).unwrap();
        assert_eq!(addr.port(), 8080);

        let addr = parse_socket_addr(None, 3000).unwrap();
        assert_eq!(addr.port(), 3000);
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
    }

    #[test]
    fn test_error_helpers() {
        let err = internal_error("test message");
        assert_eq!(err.code, -32603);
        assert_eq!(err.message, "test message");

        let err = method_not_found("test_method");
        assert_eq!(err.code, -32601);
        assert!(err.message.contains("test_method"));
    }
}
