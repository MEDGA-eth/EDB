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

//! Integration tests for the RPC proxy server

use edb_rpc_proxy::{
    cache::CacheEntry,
    proxy::{ProxyServer, ProxyServerBuilder},
};
use reqwest::Client;
use serde_json::{Value, json};
use std::{collections::HashMap, io::ErrorKind, net::SocketAddr, time::Duration};
use tempfile::TempDir;
use tokio::time::sleep;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

/// Helper to create a test proxy server
async fn skip_if_loopback_binds_restricted(test_name: &str) -> bool {
    match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => {
            drop(listener);
            false
        }
        Err(error)
            if matches!(error.kind(), ErrorKind::PermissionDenied)
                || error.raw_os_error() == Some(1) =>
        {
            eprintln!("Skipping {test_name} because loopback binds are restricted: {error}");
            true
        }
        Err(error) => panic!("Failed to probe loopback bind availability: {error}"),
    }
}

/// Helper to create a test proxy server
async fn create_test_proxy(
    test_name: &str,
    max_cache_items: u32,
) -> Option<(ProxyServer, MockServer, TempDir)> {
    if skip_if_loopback_binds_restricted(test_name).await {
        return None;
    }

    let mock_server = MockServer::start().await;
    let temp_dir = TempDir::new().unwrap();

    // Set up health check response that ProviderManager needs during initialization
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": "0x1"  // eth_blockNumber/eth_chainid response for health check
        })))
        .up_to_n_times(2) // Only for the initial health check
        .mount(&mock_server)
        .await;

    let proxy = ProxyServerBuilder::new()
        .rpc_urls(vec![mock_server.uri()])
        .max_cache_items(max_cache_items)
        .cache_dir(temp_dir.path())
        .grace_period(300) // 5 minutes
        .heartbeat_interval(10) // 10 seconds
        .max_failures(3)
        .health_check_interval(60) // 60 seconds
        .cache_save_interval(5) // 5 minutes
        .build()
        .await
        .unwrap();

    Some((proxy, mock_server, temp_dir))
}

/// Start proxy server on a random port and return the address
async fn start_proxy_server(test_name: &str, proxy: ProxyServer) -> Option<SocketAddr> {
    // Find an available port
    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(error)
            if matches!(error.kind(), ErrorKind::PermissionDenied)
                || error.raw_os_error() == Some(1) =>
        {
            eprintln!("Skipping {test_name} because loopback binds are restricted: {error}");
            return None;
        }
        Err(error) => panic!("Failed to bind proxy listener: {error}"),
    };
    let actual_addr = listener.local_addr().unwrap();
    drop(listener); // Release the listener so proxy.serve can bind to it

    tokio::spawn(async move {
        proxy.serve(actual_addr).await.unwrap();
    });

    // Give the server a moment to start
    sleep(Duration::from_millis(200)).await;
    Some(actual_addr)
}

/// Helper function to collect cache data from a proxy server
///
/// This function retrieves all cached entries from the proxy server's cache manager
/// for testing and verification purposes.
///
/// # Arguments
/// * `proxy` - Reference to the ProxyServer instance
///
/// # Returns
/// A HashMap containing all cache entries with their keys and values
pub async fn collect_cache_data(proxy: &ProxyServer) -> HashMap<String, CacheEntry> {
    proxy.cache_manager().get_all_entries().await
}

/// Helper function to get cache statistics from a proxy server
///
/// # Arguments  
/// * `proxy` - Reference to the ProxyServer instance
///
/// # Returns
/// A JSON Value containing detailed cache statistics
pub async fn get_cache_stats(proxy: &ProxyServer) -> Value {
    proxy.cache_manager().detailed_stats().await
}

#[tokio::test]
async fn test_proxy_health_endpoints() {
    let Some((proxy, _mock_server, _temp_dir)) =
        create_test_proxy("test_proxy_health_endpoints", 10).await
    else {
        return;
    };
    let Some(proxy_addr) = start_proxy_server("test_proxy_health_endpoints", proxy).await else {
        return;
    };

    let client = Client::new();
    let proxy_url = format!("http://{proxy_addr}");

    // Test edb_ping
    let ping_request = json!({
        "jsonrpc": "2.0",
        "method": "edb_ping",
        "id": 1
    });

    let response = client.post(&proxy_url).json(&ping_request).send().await.unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["result"]["status"], "ok");
    assert_eq!(body["result"]["service"], "edb-rpc-proxy");
    assert!(body["result"]["timestamp"].is_number());

    // Test edb_info
    let info_request = json!({
        "jsonrpc": "2.0",
        "method": "edb_info",
        "id": 2
    });

    let response = client.post(&proxy_url).json(&info_request).send().await.unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["result"]["service"], "edb-rpc-proxy");
}

#[tokio::test]
async fn test_proxy_registry_endpoints() {
    let Some((proxy, _mock_server, _temp_dir)) =
        create_test_proxy("test_proxy_registry_endpoints", 10).await
    else {
        return;
    };
    let Some(proxy_addr) = start_proxy_server("test_proxy_registry_endpoints", proxy).await else {
        return;
    };

    let client = Client::new();
    let proxy_url = format!("http://{proxy_addr}");

    // Test edb_register
    let register_request = json!({
        "jsonrpc": "2.0",
        "method": "edb_register",
        "params": [12345, 1234567890],
        "id": 1
    });

    let response = client.post(&proxy_url).json(&register_request).send().await.unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["result"]["status"], "registered");
    assert_eq!(body["result"]["pid"], 12345);

    // Test edb_heartbeat
    let heartbeat_request = json!({
        "jsonrpc": "2.0",
        "method": "edb_heartbeat",
        "params": [12345],
        "id": 2
    });

    let response = client.post(&proxy_url).json(&heartbeat_request).send().await.unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["result"]["status"], "ok");
    assert_eq!(body["result"]["pid"], 12345);
}

#[tokio::test]
async fn test_proxy_cache_stats_endpoint() {
    let Some((proxy, _mock_server, _temp_dir)) =
        create_test_proxy("test_proxy_cache_stats_endpoint", 100).await
    else {
        return;
    };
    let Some(proxy_addr) = start_proxy_server("test_proxy_cache_stats_endpoint", proxy).await
    else {
        return;
    };

    let client = Client::new();
    let proxy_url = format!("http://{proxy_addr}");

    // Test edb_cache_stats
    let stats_request = json!({
        "jsonrpc": "2.0",
        "method": "edb_cache_stats",
        "id": 1
    });

    let response = client.post(&proxy_url).json(&stats_request).send().await.unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();

    // Verify stats structure
    let stats = &body["result"];
    assert!(stats["total_entries"].is_number());
    assert_eq!(stats["max_entries"], 100);
    assert!(stats["utilization"].is_string());
    assert!(stats["cache_file_path"].is_string());
}

#[tokio::test]
async fn test_proxy_active_instances_endpoint() {
    let Some((proxy, _mock_server, _temp_dir)) =
        create_test_proxy("test_proxy_active_instances_endpoint", 10).await
    else {
        return;
    };
    let Some(proxy_addr) = start_proxy_server("test_proxy_active_instances_endpoint", proxy).await
    else {
        return;
    };

    let client = Client::new();
    let proxy_url = format!("http://{proxy_addr}");

    // First call to edb_active_instances should return empty list
    let active_instances_request = json!({
        "jsonrpc": "2.0",
        "method": "edb_active_instances",
        "id": 3
    });

    // Verify response structure
    let response = client.post(&proxy_url).json(&active_instances_request).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    let result = &body["result"];
    assert!(result["active_instances"].is_array());
    assert_eq!(result["count"], 0);

    // Then, we register an EDB instance
    let pid = std::process::id() as u64;
    let register_request1 = json!({
        "jsonrpc": "2.0",
        "method": "edb_register",
        "params": [pid, 1234567890],
        "id": 1
    });
    client.post(&proxy_url).json(&register_request1).send().await.unwrap();

    // Verify response structure
    let response = client.post(&proxy_url).json(&active_instances_request).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    let result = &body["result"];
    assert!(result["active_instances"].is_array());
    assert_eq!(result["count"], 1);

    // Then, we register an EDB instance with invalid PID
    let register_request2 = json!({
        "jsonrpc": "2.0",
        "method": "edb_register",
        "params": [1234567890, 1234567891],
        "id": 2
    });
    client.post(&proxy_url).json(&register_request2).send().await.unwrap();

    // Verify response structure
    let response = client.post(&proxy_url).json(&active_instances_request).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    let result = &body["result"];
    assert!(result["active_instances"].is_array());
    assert_eq!(result["count"], 1);

    let active_pids = result["active_instances"].as_array().unwrap();
    let pids: Vec<u64> = active_pids.iter().map(|v| v.as_u64().unwrap()).collect();

    assert!(pids.len() == 1);
    assert!(pids.contains(&pid));
}

#[tokio::test]
async fn test_proxy_rpc_forwarding_and_caching() {
    // Create proxy pointing to mock server
    let Some((proxy, mock_server, _temp_dir)) =
        create_test_proxy("test_proxy_rpc_forwarding_and_caching", 10).await
    else {
        return;
    };
    let Some(proxy_addr) = start_proxy_server("test_proxy_rpc_forwarding_and_caching", proxy).await
    else {
        return;
    };
    let client = Client::new();
    let proxy_url = format!("http://{proxy_addr}");

    let response_data = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "number": "0x1000000",
            "hash": "0x1234567890abcdef"
        }
    });

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_data))
        .expect(1) // Should only be called once due to caching
        .mount(&mock_server)
        .await;

    let rpc_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBlockByNumber",
        "params": ["0x1000000", false],
        "id": 1
    });

    // First request - should hit upstream
    let response1 = client.post(&proxy_url).json(&rpc_request).send().await.unwrap();

    assert_eq!(response1.status(), 200);
    let body1: Value = response1.json().await.unwrap();
    assert_eq!(body1, response_data);

    // Second request - should hit cache
    let response2 = client.post(&proxy_url).json(&rpc_request).send().await.unwrap();

    assert_eq!(response2.status(), 200);
    let body2: Value = response2.json().await.unwrap();
    assert_eq!(body2, response_data);

    // Mock server expectations should be met (only 1 call)
}

#[tokio::test]
async fn test_proxy_non_cacheable_forwarding() {
    // Create proxy pointing to mock server
    let Some((proxy, mock_server, _temp_dir)) =
        create_test_proxy("test_proxy_non_cacheable_forwarding", 10).await
    else {
        return;
    };
    let Some(proxy_addr) = start_proxy_server("test_proxy_non_cacheable_forwarding", proxy).await
    else {
        return;
    };
    let client = Client::new();
    let proxy_url = format!("http://{proxy_addr}");

    let response_data = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": "0x1000000"
    });

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_data))
        .expect(2) // Should be called twice since not cacheable
        .mount(&mock_server)
        .await;

    let rpc_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber", // Non-cacheable method
        "params": [],
        "id": 1
    });

    // Both requests should hit upstream
    let response1 = client.post(&proxy_url).json(&rpc_request).send().await.unwrap();

    assert_eq!(response1.status(), 200);
    let body1: Value = response1.json().await.unwrap();
    assert_eq!(body1, response_data);

    let response2 = client.post(&proxy_url).json(&rpc_request).send().await.unwrap();

    assert_eq!(response2.status(), 200);
    let body2: Value = response2.json().await.unwrap();
    assert_eq!(body2, response_data);

    // Mock server expectations should be met (2 calls)
}

#[tokio::test]
async fn test_proxy_non_deterministic_params_bypass_cache() {
    // Create proxy pointing to mock server
    let Some((proxy, mock_server, _temp_dir)) =
        create_test_proxy("test_proxy_non_deterministic_params_bypass_cache", 10).await
    else {
        return;
    };
    let Some(proxy_addr) =
        start_proxy_server("test_proxy_non_deterministic_params_bypass_cache", proxy).await
    else {
        return;
    };

    let response_data = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "number": "0x1000001", // Different block number to show it's not cached
            "hash": "0xabcdef1234567890"
        }
    });

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_data))
        .expect(2) // Should be called twice since "latest" bypasses cache
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let proxy_url = format!("http://{proxy_addr}");

    let rpc_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBlockByNumber",
        "params": ["latest", false], // "latest" should bypass cache
        "id": 1
    });

    // Both requests should hit upstream due to "latest" parameter
    let response1 = client.post(&proxy_url).json(&rpc_request).send().await.unwrap();

    assert_eq!(response1.status(), 200);
    let body1: Value = response1.json().await.unwrap();
    assert_eq!(body1, response_data);

    let response2 = client.post(&proxy_url).json(&rpc_request).send().await.unwrap();

    assert_eq!(response2.status(), 200);
    let body2: Value = response2.json().await.unwrap();
    assert_eq!(body2, response_data);

    // Mock server expectations should be met (2 calls)
}

#[tokio::test]
async fn test_proxy_cache_data_collection() {
    // Create proxy pointing to mock server
    let Some((proxy, mock_server, _temp_dir)) =
        create_test_proxy("test_proxy_cache_data_collection", 10).await
    else {
        return;
    };
    let Some(proxy_addr) =
        start_proxy_server("test_proxy_cache_data_collection", proxy.clone()).await
    else {
        return;
    };
    let client = Client::new();
    let proxy_url = format!("http://{proxy_addr}");

    let response_data = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "number": "0x1000000",
            "hash": "0x1234567890abcdef"
        }
    });

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_data))
        .expect(1) // Should only be called once due to caching
        .mount(&mock_server)
        .await;

    // Initially cache should be empty
    let initial_cache = collect_cache_data(&proxy).await;
    assert_eq!(initial_cache.len(), 0);

    let initial_stats = get_cache_stats(&proxy).await;
    assert_eq!(initial_stats["total_entries"], 0);

    let rpc_request = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBlockByNumber",
        "params": ["0x1000000", false],
        "id": 1
    });

    // Make a cacheable request
    let response = client.post(&proxy_url).json(&rpc_request).send().await.unwrap();
    assert_eq!(response.status(), 200);

    // Now cache should contain one entry
    let cache_after_request = collect_cache_data(&proxy).await;
    assert_eq!(cache_after_request.len(), 1);

    let stats_after_request = get_cache_stats(&proxy).await;
    assert_eq!(stats_after_request["total_entries"], 1);

    // Verify the cache contains the expected data
    // Since cache keys are now hash-based, we'll verify that there's exactly one key starting with
    // the method name
    let method_keys: Vec<_> =
        cache_after_request.keys().filter(|k| k.starts_with("eth_getBlockByNumber:")).collect();
    assert_eq!(
        method_keys.len(),
        1,
        "Should have exactly one cache entry for eth_getBlockByNumber"
    );

    let actual_cache_key = method_keys[0];
    assert_eq!(cache_after_request[actual_cache_key].data, response_data);
}

#[tokio::test]
async fn test_proxy_shutdown_endpoint() {
    let Some((proxy, _mock_server, _temp_dir)) =
        create_test_proxy("test_proxy_shutdown_endpoint", 10).await
    else {
        return;
    };
    let Some(proxy_addr) = start_proxy_server("test_proxy_shutdown_endpoint", proxy).await else {
        return;
    };

    let client = Client::new();
    let proxy_url = format!("http://{proxy_addr}");

    // Test edb_shutdown
    let shutdown_request = json!({
        "jsonrpc": "2.0",
        "method": "edb_shutdown",
        "params": [],
        "id": 1
    });

    let response = client.post(&proxy_url).json(&shutdown_request).send().await.unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["result"]["status"], "shutting_down");
    assert_eq!(body["result"]["message"], "Server shutdown initiated");

    // Give the server a moment to shut down
    sleep(Duration::from_millis(500)).await;

    // Subsequent requests should fail as server is shut down
    let ping_request = json!({
        "jsonrpc": "2.0",
        "method": "edb_ping",
        "params": [],
        "id": 2
    });

    let result = client.post(&proxy_url).json(&ping_request).send().await;
    // This should fail because the server has shut down
    assert!(result.is_err());
}

#[tokio::test]
async fn test_proxy_invalid_request_handling() {
    let Some((proxy, _mock_server, _temp_dir)) =
        create_test_proxy("test_proxy_invalid_request_handling", 10).await
    else {
        return;
    };
    let Some(proxy_addr) = start_proxy_server("test_proxy_invalid_request_handling", proxy).await
    else {
        return;
    };

    let client = Client::new();
    let proxy_url = format!("http://{proxy_addr}");

    // Test request without method
    let invalid_request = json!({
        "jsonrpc": "2.0",
        "id": 1
    });

    let response = client.post(&proxy_url).json(&invalid_request).send().await.unwrap();

    assert_eq!(response.status(), 400); // Bad Request

    // Test malformed edb_register request (missing params)
    let malformed_request = json!({
        "jsonrpc": "2.0",
        "method": "edb_register",
        "id": 1
    });

    let response = client.post(&proxy_url).json(&malformed_request).send().await.unwrap();

    assert_eq!(response.status(), 400); // Bad Request
}
