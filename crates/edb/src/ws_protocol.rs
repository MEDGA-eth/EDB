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

//! WebSocket protocol definitions for EDB server mode
//!
//! This module defines the message protocol used for communication between
//! WebSocket clients and the EDB server. Clients can request transaction
//! replays or test debugging sessions, and the server responds with RPC
//! server connection information.

use serde::{Deserialize, Serialize};

// Re-export ProgressMessage from common crate
pub use edb_common::ProgressMessage;

/// Request sent from client to server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ClientRequest {
    /// Request to replay an existing transaction
    Replay {
        /// Transaction hash to replay (with or without 0x prefix)
        tx_hash: String,
    },
    /// Request to debug a Foundry test (not yet implemented)
    Test {
        /// Name of the test to debug
        test_name: String,
        /// Optional block number to fork at
        #[serde(skip_serializing_if = "Option::is_none")]
        block: Option<u64>,
    },
}

/// Response sent from server to client
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum ServerResponse {
    /// In-progress status update sent while the session is being prepared.
    Progress {
        /// Human-readable progress message.
        message: String,
        /// Optional current step index.
        #[serde(skip_serializing_if = "Option::is_none")]
        current_step: Option<usize>,
        /// Optional total number of steps.
        #[serde(skip_serializing_if = "Option::is_none")]
        total_steps: Option<usize>,
    },
    /// Successful response with RPC server information
    Success {
        /// Port of the RPC debug server
        rpc_port: u16,
        /// Transaction hash being debugged
        tx_hash: String,
        /// Whether this is a reused existing session
        reused: bool,
    },
    /// Error response
    Error {
        /// Error message
        message: String,
    },
}

impl ServerResponse {
    /// Create a success response
    pub fn success(rpc_port: u16, tx_hash: String, reused: bool) -> Self {
        Self::Success { rpc_port, tx_hash, reused }
    }

    /// Create an error response
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error { message: message.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_request_replay_serialization() {
        let request = ClientRequest::Replay { tx_hash: "0x1234567890abcdef".to_string() };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"type\":\"replay\""));
        assert!(json.contains("\"tx_hash\":\"0x1234567890abcdef\""));
    }

    #[test]
    fn test_client_request_test_serialization() {
        let request =
            ClientRequest::Test { test_name: "testTransfer".to_string(), block: Some(12345) };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"type\":\"test\""));
        assert!(json.contains("\"test_name\":\"testTransfer\""));
        assert!(json.contains("\"block\":12345"));
    }

    #[test]
    fn test_server_response_success_serialization() {
        let response = ServerResponse::success(3000, "0xabcd".to_string(), false);
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"status\":\"success\""));
        assert!(json.contains("\"rpc_port\":3000"));
        assert!(json.contains("\"reused\":false"));
    }

    #[test]
    fn test_server_response_error_serialization() {
        let response = ServerResponse::error("Invalid transaction hash");
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"status\":\"error\""));
        assert!(json.contains("\"message\":\"Invalid transaction hash\""));
    }

    #[test]
    fn test_server_response_progress_without_steps() {
        let response = ServerResponse::Progress {
            message: "Processing".to_string(),
            current_step: None,
            total_steps: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"status\":\"progress\""));
        assert!(json.contains("\"message\":\"Processing\""));
        assert!(!json.contains("current_step"));
        assert!(!json.contains("total_steps"));
    }

    #[test]
    fn test_server_response_progress_with_steps() {
        let response = ServerResponse::Progress {
            message: "Step 2 of 5".to_string(),
            current_step: Some(2),
            total_steps: Some(5),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"status\":\"progress\""));
        assert!(json.contains("\"message\":\"Step 2 of 5\""));
        assert!(json.contains("\"current_step\":2"));
        assert!(json.contains("\"total_steps\":5"));
    }

    #[test]
    fn test_client_request_deserialization() {
        let json = r#"{"type":"replay","tx_hash":"0x1234"}"#;
        let request: ClientRequest = serde_json::from_str(json).unwrap();
        match request {
            ClientRequest::Replay { tx_hash } => assert_eq!(tx_hash, "0x1234"),
            _ => panic!("Expected Replay variant"),
        }
    }
}
