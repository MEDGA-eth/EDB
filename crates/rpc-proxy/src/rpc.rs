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

//! RPC request handling and caching logic

use crate::{
    cache::CacheManager,
    metrics::{ErrorType, MetricsCollector},
    providers::ProviderManager,
};
use eyre::Result;
use reqwest::StatusCode;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::Arc,
    time::Instant,
};
use tracing::{debug, warn};

/// RPC methods that can be cached (when using deterministic block parameters)
const CACHEABLE_METHODS: &[&str] = &[
    // State queries (require deterministic block param)
    "eth_getCode",
    "eth_getBalance",
    "eth_getStorageAt",
    "eth_getProof",
    "eth_getTransactionCount",
    "eth_call",
    // Transaction data (always immutable)
    "eth_getTransactionByHash",
    "eth_getRawTransactionByHash",
    "eth_getTransactionReceipt",
    "eth_getTransactionByBlockHashAndIndex",
    "eth_getTransactionByBlockNumberAndIndex",
    // Block data (immutable once finalized)
    "eth_getBlockByNumber",
    "eth_getBlockByHash",
    "eth_getBlockReceipts",
    "eth_getBlockTransactionCountByHash",
    "eth_getBlockTransactionCountByNumber",
    "eth_getUncleByBlockHashAndIndex",
    "eth_getUncleByBlockNumberAndIndex",
    "eth_getUncleCountByBlockHash",
    "eth_getUncleCountByBlockNumber",
    // Logs (require deterministic block range)
    "eth_getLogs",
    // Chain constants
    "eth_chainId",
    "net_version",
    // Debug/trace methods (immutable traces, require deterministic blocks)
    "debug_traceTransaction",
    "debug_traceBlockByNumber",
    "debug_traceBlockByHash",
    "debug_traceCall",
    "trace_transaction",
    "trace_block",
    "trace_replayTransaction",
    "trace_replayBlockTransactions",
    "trace_call",
];

/// Common rate limit error patterns found in various RPC providers
const RATE_LIMIT_PATTERNS: &[&str] = &[
    "rate limit",
    "rate-limit",
    "ratelimit",
    "too many requests",
    "cu limit exceeded",
    "compute units exceeded",
    "quota exceeded",
    "throttled",
    "exceeded the allowed rps",
    "request limit",
    "max requests",
];

/// Common user error patterns that indicate invalid requests
const USER_ERROR_PATTERNS: &[&str] = &[
    "invalid method",
    "method not found",
    "invalid params",
    "missing param",
    "invalid argument",
    "parse error",
    "invalid request",
    "unsupported method",
];

/// Patterns indicating that debug/trace methods are not supported by provider
const UNSUPPORTED_METHOD_PATTERNS: &[&str] =
    &["not supported", "not available", "method not found", "unsupported", "not implemented"];

/// Fields that indicate valid trace/debug response structure
const TRACE_RESPONSE_FIELDS: &[&str] =
    &["output", "trace", "stateDiff", "result", "structLogs", "gas", "returnValue"];

/// RPC request handler with intelligent caching capabilities
///
/// Handles incoming RPC requests by either serving from cache or forwarding to
/// upstream RPC servers. Implements smart caching logic that avoids caching
/// non-deterministic requests (e.g., "latest" block parameters).
pub struct RpcHandler {
    upstream_client: reqwest::Client,
    provider_manager: Arc<ProviderManager>,
    cache_manager: Arc<CacheManager>,
    metrics_collector: Arc<MetricsCollector>,
}

impl RpcHandler {
    /// Creates a new RPC handler with multiple providers and cache manager
    ///
    /// # Arguments
    /// * `provider_manager` - Manager for multiple RPC providers with load balancing
    /// * `cache_manager` - Shared cache manager for storing responses
    /// * `metrics_collector` - Metrics collector for performance tracking
    ///
    /// # Returns
    /// A new RpcHandler instance ready to process requests
    pub fn new(
        provider_manager: Arc<ProviderManager>,
        cache_manager: Arc<CacheManager>,
        metrics_collector: Arc<MetricsCollector>,
    ) -> Result<Self> {
        let upstream_client =
            reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build()?;

        Ok(Self { upstream_client, provider_manager, cache_manager, metrics_collector })
    }

    /// Returns a reference to the cache manager
    ///
    /// # Returns
    /// Reference to the underlying cache manager
    pub fn cache_manager(&self) -> &Arc<CacheManager> {
        &self.cache_manager
    }

    /// Returns a reference to the provider manager
    pub fn provider_manager(&self) -> &Arc<ProviderManager> {
        &self.provider_manager
    }

    /// Returns a reference to the metrics collector
    #[allow(dead_code)]
    pub fn metrics_collector(&self) -> &Arc<MetricsCollector> {
        &self.metrics_collector
    }

    /// Handles an RPC request with intelligent caching
    ///
    /// Determines whether to serve from cache or forward to upstream based on:
    /// - Whether the method is cacheable (see CACHEABLE_METHODS)
    /// - Whether the request contains non-deterministic block parameters
    /// - Whether a cached response already exists
    /// - Whether the response is valid (for debug/trace methods)
    ///
    /// # Arguments
    /// * `request` - The JSON-RPC request to handle
    ///
    /// # Returns
    /// The JSON-RPC response, either from cache or upstream
    pub async fn handle_request(&self, request: Value) -> Result<Value> {
        let start_time = Instant::now();
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

        debug!("Handling RPC request: {}", method);

        // Check if this method is cacheable
        if CACHEABLE_METHODS.contains(&method) {
            // ALL cacheable methods must pass the deterministic block check
            // This applies uniformly to eth_call, eth_getBalance, etc.
            if self.has_non_deterministic_block_params(&request) {
                debug!("Non-deterministic block params for {}, bypassing cache", method);
                return self.forward_request(&request).await;
            }

            // Generate cache key from request
            let cache_key = self.generate_cache_key(&request);

            // Try to get from cache first
            if let Some(cached_response) = self.cache_manager.get(&cache_key).await {
                let response_time = start_time.elapsed().as_millis() as u64;
                self.metrics_collector.record_cache_hit(method, response_time);
                debug!("Cache hit for {}: {}", method, cache_key);
                return Ok(cached_response);
            }

            debug!("Cache miss for {}: {}", method, cache_key);

            // Forward to upstream
            let response = self.forward_request(&request).await;
            self.metrics_collector.record_cache_miss();

            if let Ok(resp) = &response {
                let success = resp.get("error").is_none();

                // Only cache successful responses
                if success {
                    // Additional validation for debug/trace methods
                    if (method.starts_with("debug_") || method.starts_with("trace_"))
                        && !self.is_valid_debug_trace_response(resp)
                    {
                        debug!("Invalid debug/trace response for {}, not caching", method);
                        return Ok(resp.clone());
                    }

                    self.cache_manager.set(cache_key, resp.clone()).await;
                    debug!("Cached response for {}", method);
                } else {
                    debug!("Error response for {}, not caching", method);
                    // Classify error type for metrics
                    if let Some(error_obj) = resp.get("error")
                        && let Some(error_msg) = error_obj.get("message").and_then(|m| m.as_str())
                    {
                        let error_msg_lower = error_msg.to_lowercase();
                        if RATE_LIMIT_PATTERNS
                            .iter()
                            .any(|pattern| error_msg_lower.contains(pattern))
                        {
                            self.metrics_collector.record_error(ErrorType::RateLimit);
                        } else if USER_ERROR_PATTERNS
                            .iter()
                            .any(|pattern| error_msg_lower.contains(pattern))
                        {
                            self.metrics_collector.record_error(ErrorType::UserError);
                        } else {
                            self.metrics_collector.record_error(ErrorType::Other);
                        }
                    }
                }
            }

            response
        } else {
            // Non-cacheable request - forward directly
            debug!("Non-cacheable request: {}", method);
            self.forward_request(&request).await
        }
    }

    /// Detects if a response indicates rate limiting
    ///
    /// Checks HTTP status codes, response text patterns, and JSON error messages
    /// to identify rate limiting from various providers
    fn is_rate_limit_response(
        &self,
        status: StatusCode,
        response_text: &str,
        json_response: Option<&Value>,
    ) -> bool {
        // Check HTTP status code
        if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE {
            debug!("Rate limit response detected by status: {}", status);
            return true;
        }

        // Check response text for rate limit patterns
        let text_lower = response_text.to_lowercase();
        if RATE_LIMIT_PATTERNS.iter().any(|pattern| text_lower.contains(pattern)) {
            debug!(
                "Rate limit response detected by response text: {}",
                &text_lower.chars().take(200).collect::<String>()
            );
            return true;
        }

        // Check JSON error response
        if let Some(json) = json_response
            && let Some(error) = json.get("error")
        {
            // Check error message
            if let Some(message) = error.get("message").and_then(|m| m.as_str()) {
                let msg_lower = message.to_lowercase();
                if RATE_LIMIT_PATTERNS.iter().any(|pattern| msg_lower.contains(pattern)) {
                    debug!(
                        "Rate limit response detected by error message: {}",
                        &msg_lower.chars().take(200).collect::<String>()
                    );
                    return true;
                }
            }

            // Check error code (some providers use specific codes for rate limiting)
            if let Some(code) = error.get("code").and_then(|c| c.as_i64()) {
                match code {
                    429 | -32005 | -32098 | -32099 => {
                        debug!("Rate limit response detected by error code: {}", code);
                        return true;
                    }
                    _ => {}
                }
            }
        }

        false
    }

    /// Detects if an error is due to user's invalid request
    ///
    /// These errors should be returned immediately without trying other providers
    fn is_user_error(&self, json_response: &Value) -> bool {
        if let Some(error) = json_response.get("error") {
            if let Some(code) = error.get("code").and_then(|c| c.as_i64()) {
                match code {
                    -32600 | -32601 | -32602 | -32700 => {
                        debug!("Detected user error with code: {}", code);
                        return true;
                    }
                    _ => {}
                }
            }

            // Also check for specific error messages that indicate user error
            if let Some(message) = error.get("message").and_then(|m| m.as_str()) {
                let msg_lower = message.to_lowercase();

                if USER_ERROR_PATTERNS.iter().any(|pattern| msg_lower.contains(pattern)) {
                    debug!("Detected user error from message: {}", message);
                    return true;
                }
            }
        }

        false
    }

    /// Creates a hash-based signature for error deduplication
    ///
    /// Uses a stable hash of the error object to identify similar errors
    fn create_error_signature(&self, error: &Value) -> u64 {
        let mut hasher = DefaultHasher::new();

        // Hash the error code if present
        if let Some(code) = error.get("code").and_then(|c| c.as_i64()) {
            code.hash(&mut hasher);
        }

        // Hash the error message if present (normalized to lowercase)
        if let Some(message) = error.get("message").and_then(|m| m.as_str()) {
            message.to_lowercase().hash(&mut hasher);
        }

        // Optionally hash error data if it's a simple type
        if let Some(data) = error.get("data") {
            match data {
                Value::String(s) => s.hash(&mut hasher),
                Value::Number(n) => n.to_string().hash(&mut hasher),
                Value::Bool(b) => b.hash(&mut hasher),
                _ => {} // Skip complex data structures
            }
        }

        hasher.finish()
    }

    /// Validate debug/trace responses to ensure provider actually supports them
    ///
    /// Some providers don't support debug/trace methods and return errors
    /// or invalid responses that shouldn't be cached
    fn is_valid_debug_trace_response(&self, response: &Value) -> bool {
        if let Some(result) = response.get("result") {
            // Check for common "not supported" patterns in string responses
            if let Value::String(s) = result {
                let s_lower = s.to_lowercase();
                if UNSUPPORTED_METHOD_PATTERNS.iter().any(|pattern| s_lower.contains(pattern)) {
                    return false;
                }
            }

            // For trace methods, ensure we have actual trace data
            if let Value::Object(obj) = result {
                // Should have some recognizable trace structure
                let has_trace_fields =
                    TRACE_RESPONSE_FIELDS.iter().any(|field| obj.contains_key(*field));

                if !has_trace_fields {
                    // Empty object or unrecognized structure
                    return false;
                }
            }

            // Arrays are valid (list of traces)
            if let Value::Array(arr) = result {
                // Empty arrays might indicate unsupported method
                return !arr.is_empty();
            }

            // Null results indicate unsupported
            if result.is_null() {
                return false;
            }

            return true;
        }

        false
    }

    async fn forward_request(&self, request: &Value) -> Result<Value> {
        const MAX_RETRIES: usize = 5;
        const MAX_MULTIPLE_SAME_ERROR: usize = 3;

        // Track providers we've already tried for this request
        let mut tried_providers = HashSet::new();
        // Track errors from different providers using hash as key
        let mut error_responses: HashMap<u64, (Value, usize)> = HashMap::new();
        let mut last_network_error: Option<eyre::Error> = None;
        let mut providers_tried = 0;

        for retry in 0..MAX_RETRIES {
            // Get a provider we haven't tried yet
            let provider_url =
                match self.provider_manager.get_weighted_provider_excluding(&tried_providers).await
                {
                    Some(url) => url,
                    None => {
                        debug!(
                            "All available providers have been tried for this request (attempt {})",
                            retry + 1
                        );

                        // If no untried providers, trigger health check and try again
                        if retry < MAX_RETRIES - 1 {
                            self.provider_manager.health_check_all().await;
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            // Clear tried providers to allow retry of previously failed ones after
                            // health check
                            tried_providers.clear();
                            continue;
                        }

                        // If we have any error responses, return the most common one
                        if let Some((error_response, _)) =
                            error_responses.values().max_by_key(|(_, count)| *count)
                        {
                            debug!("All providers exhausted, returning most common error");
                            return Ok(error_response.clone());
                        }

                        return Err(last_network_error
                            .unwrap_or_else(|| eyre::eyre!("No available RPC providers")));
                    }
                };

            // Mark this provider as tried
            tried_providers.insert(provider_url.clone());

            providers_tried += 1;
            debug!(
                "Forwarding request to provider: {} (attempt {}/{}, {} providers tried)",
                provider_url,
                retry + 1,
                MAX_RETRIES,
                tried_providers.len()
            );

            let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("unknown_method");

            let start = Instant::now();

            match self
                .upstream_client
                .post(&provider_url)
                .header("Content-Type", "application/json")
                .json(request)
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    let response_time = start.elapsed().as_millis() as u64;

                    match response.text().await {
                        Ok(response_text) => {
                            // Try to parse as JSON
                            let json_result = serde_json::from_str::<Value>(&response_text);

                            // Check if it's a rate limit error
                            if self.is_rate_limit_response(
                                status,
                                &response_text,
                                json_result.as_ref().ok(),
                            ) {
                                debug!(
                                    "Provider {} is rate limited (response: {}...)",
                                    provider_url,
                                    &response_text.chars().take(200).collect::<String>()
                                );

                                self.metrics_collector.record_request(
                                    method,
                                    &provider_url,
                                    response_time,
                                    false,
                                );
                                self.provider_manager.mark_provider_failed(&provider_url).await;

                                // Continue to next provider without counting as error
                                continue;
                            }

                            // Handle valid JSON response
                            match json_result {
                                Ok(response_json) => {
                                    // Check if response contains an error
                                    if let Some(error) = response_json.get("error") {
                                        self.metrics_collector.record_request(
                                            method,
                                            &provider_url,
                                            response_time,
                                            false,
                                        );

                                        // Check if it's a user error (invalid request)
                                        if self.is_user_error(&response_json) {
                                            debug!("Detected user error, returning immediately");

                                            // Don't mark provider as failed - it's user's fault
                                            self.provider_manager
                                                .mark_provider_success(&provider_url, response_time)
                                                .await;
                                            return Ok(response_json);
                                        }

                                        // It's a provider/blockchain error
                                        let error_hash = self.create_error_signature(error);

                                        // Track this error
                                        error_responses
                                            .entry(error_hash)
                                            .and_modify(|(_, count)| *count += 1)
                                            .or_insert((response_json.clone(), 1));

                                        debug!(
                                            "Provider {} returned error (hash: {})",
                                            provider_url, error_hash
                                        );

                                        // If multiple providers return the same error, it's likely
                                        // legitimate
                                        if let Some((_, count)) = error_responses.get(&error_hash)
                                            && *count >= MAX_MULTIPLE_SAME_ERROR
                                        {
                                            debug!(
                                                "Multiple providers ({}) returned same error, likely legitimate",
                                                count
                                            );
                                            return Ok(response_json);
                                        }

                                        // Mark provider as failed and continue
                                        self.provider_manager
                                            .mark_provider_failed(&provider_url)
                                            .await;
                                        continue;
                                    }

                                    // Success! No error in response
                                    self.metrics_collector.record_request(
                                        method,
                                        &provider_url,
                                        response_time,
                                        true,
                                    );

                                    self.provider_manager
                                        .mark_provider_success(&provider_url, response_time)
                                        .await;

                                    debug!(
                                        "Request successful via {} ({}ms)",
                                        provider_url, response_time
                                    );
                                    return Ok(response_json);
                                }
                                Err(parse_error) => {
                                    // Response is not valid JSON
                                    warn!(
                                        "Invalid JSON response from {} (first 200 chars): {}...",
                                        provider_url,
                                        &response_text.chars().take(200).collect::<String>()
                                    );

                                    self.metrics_collector.record_request(
                                        method,
                                        &provider_url,
                                        response_time,
                                        false,
                                    );
                                    self.provider_manager.mark_provider_failed(&provider_url).await;
                                    last_network_error = Some(eyre::eyre!(
                                        "Invalid JSON from provider: {}",
                                        parse_error
                                    ));
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to read response body from {}: {}", provider_url, e);
                            self.metrics_collector.record_request(
                                method,
                                &provider_url,
                                response_time,
                                false,
                            );
                            self.provider_manager.mark_provider_failed(&provider_url).await;
                            last_network_error = Some(e.into());
                            continue;
                        }
                    }
                }
                Err(e) => {
                    warn!("Request failed to {}: {}", provider_url, e);

                    let response_time = start.elapsed().as_millis() as u64;
                    self.metrics_collector.record_request(
                        method,
                        &provider_url,
                        response_time,
                        false,
                    );

                    self.provider_manager.mark_provider_failed(&provider_url).await;
                    last_network_error = Some(e.into());
                    continue;
                }
            }
        }

        // All retries exhausted
        warn!(
            "All {} retries exhausted after trying {} providers for request: {}",
            MAX_RETRIES, providers_tried, request
        );

        // Return the most common error if we have any
        if !error_responses.is_empty() {
            let (most_common_error, count) =
                error_responses.values().max_by_key(|(_, count)| *count).unwrap();

            debug!("Returning most common error (seen {} times)", count);
            return Ok(most_common_error.clone());
        }

        // Otherwise return the last network error
        Err(last_network_error.unwrap_or_else(|| eyre::eyre!("429 Too Many Requests")))
    }

    fn has_non_deterministic_block_params(&self, request: &Value) -> bool {
        let params = request.get("params").and_then(|p| p.as_array());

        if let Some(params) = params {
            for param in params {
                if let Some("latest" | "pending" | "earliest" | "safe" | "finalized") =
                    param.as_str()
                {
                    return true;
                }
                // Also check object parameters for block identifiers
                if let Some(param_obj) = param.as_object()
                    && let Some(block_value) = param_obj
                        .get("blockNumber")
                        .or_else(|| param_obj.get("toBlock"))
                        .or_else(|| param_obj.get("fromBlock"))
                    && let Some("latest" | "pending" | "earliest" | "safe" | "finalized") =
                        block_value.as_str()
                {
                    return true;
                }
            }
        }

        false
    }

    /// Generate a unique cache key for the given RPC request based on method and parameters
    pub fn generate_cache_key(&self, request: &Value) -> String {
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = request.get("params").unwrap_or(&Value::Null);

        // Create a hasher for consistent key generation
        let mut hasher = DefaultHasher::new();
        method.hash(&mut hasher);

        // Hash parameters in a consistent way
        Self::hash_json_value(&mut hasher, params);

        format!("{}:{:x}", method, hasher.finish())
    }

    /// Hash a JSON value consistently for cache key generation
    fn hash_json_value(hasher: &mut DefaultHasher, value: &Value) {
        use std::collections::BTreeMap;

        match value {
            Value::Null => "null".hash(hasher),
            Value::Bool(b) => b.hash(hasher),
            Value::Number(n) => n.to_string().hash(hasher),
            Value::String(s) => s.hash(hasher),
            Value::Array(arr) => {
                "array".hash(hasher);
                arr.len().hash(hasher);
                for elem in arr {
                    Self::hash_json_value(hasher, elem);
                }
            }
            Value::Object(obj) => {
                "object".hash(hasher);
                // Sort keys for consistent hashing
                let sorted: BTreeMap<_, _> = obj.iter().collect();
                sorted.len().hash(hasher);
                for (key, val) in sorted {
                    key.hash(hasher);
                    Self::hash_json_value(hasher, val);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheManager;
    use edb_common::logging;
    use std::io::ErrorKind;
    use tempfile::TempDir;
    use tracing::{debug, info};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

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
                info!("Skipping {test_name} because loopback binds are restricted: {error}");
                true
            }
            Err(error) => panic!("Failed to probe loopback bind availability: {error}"),
        }
    }

    async fn create_test_rpc_handler(test_name: &str) -> Option<(RpcHandler, MockServer, TempDir)> {
        if skip_if_loopback_binds_restricted(test_name).await {
            return None;
        }

        let mock_server = MockServer::start().await;
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("test_cache.json");

        // Set up health check response that ProviderManager needs during initialization
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "0x1"  // eth_blockNumber/eth_chainid response for health check
            })))
            .up_to_n_times(1) // Only for the initial health check
            .mount(&mock_server)
            .await;

        let cache_manager = Arc::new(CacheManager::new(100, cache_path).unwrap());
        let metrics_collector = Arc::new(MetricsCollector::new());

        // Now create provider manager with mock server URL
        let provider_manager = Arc::new(
            ProviderManager::new(vec![mock_server.uri()], 3)
                .await
                .expect("Failed to create provider manager"),
        );

        let handler = RpcHandler::new(provider_manager, cache_manager, metrics_collector).unwrap();

        Some((handler, mock_server, temp_dir))
    }

    #[tokio::test]
    async fn test_cacheable_method_caching() {
        let Some((handler, mock_server, _temp_dir)) =
            create_test_rpc_handler("test_cacheable_method_caching").await
        else {
            return;
        };

        let response_data = serde_json::json!({
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

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": ["0x1000000", false],
            "id": 1
        });

        // First request - should hit upstream
        let result1 = handler.handle_request(request.clone()).await.unwrap();
        assert_eq!(result1, response_data);

        // Second request - should hit cache
        let result2 = handler.handle_request(request).await.unwrap();
        assert_eq!(result2, response_data);

        // Mock server expectations should be met (only 1 call)
    }

    #[tokio::test]
    async fn test_non_cacheable_method_passthrough() {
        let Some((handler, mock_server, _temp_dir)) =
            create_test_rpc_handler("test_non_cacheable_method_passthrough").await
        else {
            return;
        };

        let response_data = serde_json::json!({
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

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });

        // Both requests should hit upstream
        let result1 = handler.handle_request(request.clone()).await.unwrap();
        assert_eq!(result1, response_data);

        let result2 = handler.handle_request(request).await.unwrap();
        assert_eq!(result2, response_data);
    }

    #[tokio::test]
    async fn test_non_deterministic_block_params_bypass_cache() {
        let Some((handler, mock_server, _temp_dir)) =
            create_test_rpc_handler("test_non_deterministic_block_params_bypass_cache").await
        else {
            return;
        };

        let response_data = serde_json::json!({
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
            .expect(2) // Should be called twice since "latest" bypasses cache
            .mount(&mock_server)
            .await;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": ["latest", false],
            "id": 1
        });

        // Both requests should hit upstream due to "latest" parameter
        let result1 = handler.handle_request(request.clone()).await.unwrap();
        assert_eq!(result1, response_data);

        let result2 = handler.handle_request(request).await.unwrap();
        assert_eq!(result2, response_data);
    }

    #[tokio::test]
    async fn test_deterministic_vs_non_deterministic_params() {
        let Some((handler, mock_server, _temp_dir)) =
            create_test_rpc_handler("test_deterministic_vs_non_deterministic_params").await
        else {
            return;
        };

        let response_data = serde_json::json!({
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
            .expect(3) // Latest call twice + one specific block call
            .mount(&mock_server)
            .await;

        // Non-deterministic request with "latest"
        let latest_request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": ["latest", false],
            "id": 1
        });

        // Deterministic request with specific block number
        let specific_request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": ["0x1000000", false],
            "id": 1
        });

        // Latest requests should both hit upstream
        handler.handle_request(latest_request.clone()).await.unwrap();
        handler.handle_request(latest_request).await.unwrap();

        // Specific block request should hit upstream once
        handler.handle_request(specific_request.clone()).await.unwrap();
        // Second specific block request should hit cache (no additional upstream call)
        handler.handle_request(specific_request).await.unwrap();
    }

    #[tokio::test]
    async fn test_has_non_deterministic_block_params() {
        let Some((handler, _mock_server, _temp_dir)) =
            create_test_rpc_handler("test_has_non_deterministic_block_params").await
        else {
            return;
        };

        // Test various non-deterministic block parameters
        let test_cases = vec![
            (
                serde_json::json!({
                    "method": "eth_getBlockByNumber",
                    "params": ["latest", false]
                }),
                true,
            ),
            (
                serde_json::json!({
                    "method": "eth_getBlockByNumber",
                    "params": ["pending", false]
                }),
                true,
            ),
            (
                serde_json::json!({
                    "method": "eth_getBlockByNumber",
                    "params": ["0x1000000", false]
                }),
                false,
            ),
            (
                serde_json::json!({
                    "method": "eth_getLogs",
                    "params": [{
                        "fromBlock": "latest",
                        "toBlock": "latest"
                    }]
                }),
                true,
            ),
            (
                serde_json::json!({
                    "method": "eth_getLogs",
                    "params": [{
                        "fromBlock": "0x1000000",
                        "toBlock": "0x1000010"
                    }]
                }),
                false,
            ),
        ];

        for (request, expected) in test_cases {
            let result = handler.has_non_deterministic_block_params(&request);
            assert_eq!(result, expected, "Failed for request: {request:?}");
        }
    }

    #[tokio::test]
    async fn test_error_response_not_cached() {
        let Some((handler, mock_server, _temp_dir)) =
            create_test_rpc_handler("test_error_response_not_cached").await
        else {
            return;
        };

        let error_response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32602,
                "message": "Invalid params"
            }
        });

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&error_response))
            .expect(2) // Should be called twice since errors aren't cached
            .mount(&mock_server)
            .await;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": ["0x1000000", false],
            "id": 1
        });

        // Both requests should hit upstream since error responses aren't cached
        let result1 = handler.handle_request(request.clone()).await.unwrap();
        assert_eq!(result1, error_response);

        let result2 = handler.handle_request(request).await.unwrap();
        assert_eq!(result2, error_response);
    }

    #[tokio::test]
    async fn test_multi_provider_weighted_distribution() {
        if skip_if_loopback_binds_restricted("test_multi_provider_weighted_distribution").await {
            return;
        }
        // Create 3 mock servers for weighted distribution testing
        let mock1 = MockServer::start().await;
        let mock2 = MockServer::start().await;
        let mock3 = MockServer::start().await;

        // Setup health check responses for all providers
        for mock_server in &[&mock1, &mock2, &mock3] {
            Mock::given(method("POST"))
                .and(path("/"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": "0x1234567"
                })))
                .up_to_n_times(1) // Health check
                .mount(mock_server)
                .await;
        }

        // Create cache and provider manager with all 3 mock servers
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("test_cache.json");
        let cache_manager = Arc::new(CacheManager::new(100, cache_path).unwrap());

        let provider_manager = Arc::new(
            ProviderManager::new(vec![mock1.uri(), mock2.uri(), mock3.uri()], 3)
                .await
                .expect("Failed to create provider manager"),
        );

        let metrics_collector = Arc::new(MetricsCollector::new());
        let handler = RpcHandler::new(provider_manager, cache_manager, metrics_collector).unwrap();

        // Setup different responses for each mock to verify weighted distribution
        let response1 = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": "response_from_server_1"
        });

        let response2 = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": "response_from_server_2"
        });

        let response3 = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": "response_from_server_3"
        });

        // Setup expectations for non-cacheable method (eth_blockNumber) to test weighted
        // distribution Each server should get roughly equal requests for single request
        // calls due to weighted selection
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response1))
            .expect(0..=9) // Variable count due to weighted selection
            .mount(&mock1)
            .await;

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response2))
            .expect(0..=9)
            .mount(&mock2)
            .await;

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response3))
            .expect(0..=9)
            .mount(&mock3)
            .await;

        // Send 9 requests for non-cacheable method to verify weighted distribution
        let mut responses = Vec::new();
        for i in 0..9 {
            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_blockNumber", // Non-cacheable method
                "params": [],
                "id": i + 1
            });

            let response = handler.handle_request(request).await.unwrap();
            responses.push(response["result"].as_str().unwrap().to_string());
        }

        // Verify that all servers are used (distribution may vary due to weighted selection)
        let server1_count = responses.iter().filter(|r| *r == "response_from_server_1").count();
        let server2_count = responses.iter().filter(|r| *r == "response_from_server_2").count();
        let server3_count = responses.iter().filter(|r| *r == "response_from_server_3").count();

        // Verify that all requests were completed and distributed among servers
        assert_eq!(
            server1_count + server2_count + server3_count,
            9,
            "All 9 requests should be completed"
        );

        // With weighted selection, each server should get at least some requests over 9 attempts
        // (though the exact distribution varies due to randomness)
        debug!(
            "Server distribution - Server1: {}, Server2: {}, Server3: {}",
            server1_count, server2_count, server3_count
        );
    }

    #[tokio::test]
    async fn test_provider_tried_once_per_request() {
        if skip_if_loopback_binds_restricted("test_provider_tried_once_per_request").await {
            return;
        }
        edb_common::logging::ensure_test_logging(None);
        info!("Testing that each provider is only tried once per request with Option 1");

        // Create 3 mock servers that will all return errors
        let mock1 = MockServer::start().await;
        let mock2 = MockServer::start().await;
        let mock3 = MockServer::start().await;

        // Setup health checks
        for mock_server in &[&mock1, &mock2, &mock3] {
            Mock::given(method("POST"))
                .and(path("/"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": "0x1"
                })))
                .up_to_n_times(1)
                .mount(mock_server)
                .await;
        }

        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("test_cache.json");
        let cache_manager = Arc::new(CacheManager::new(100, cache_path).unwrap());

        let provider_manager = Arc::new(
            ProviderManager::new(vec![mock1.uri(), mock2.uri(), mock3.uri()], 3)
                .await
                .expect("Failed to create provider manager"),
        );

        let metrics_collector = Arc::new(MetricsCollector::new());
        let handler = RpcHandler::new(provider_manager, cache_manager, metrics_collector).unwrap();

        // All providers return the same blockchain error
        let error_response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "Execution reverted: insufficient balance"
            }
        });

        // Each provider should be called exactly once per request
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&error_response))
            .expect(1) // Should only be called once
            .mount(&mock1)
            .await;

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&error_response))
            .expect(1) // Should only be called once
            .mount(&mock2)
            .await;

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&error_response))
            .expect(1) // Should only be called once
            .mount(&mock3)
            .await;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{}, "0x1000000"],
            "id": 1
        });

        // Should return the error after all 3 providers are tried once each
        let result = handler.handle_request(request).await.unwrap();
        assert_eq!(result, error_response);

        // Mock expectations will verify that each provider was called exactly once
        debug!("All providers tried exactly once - Option 1 working correctly");
    }

    #[tokio::test]
    async fn test_multi_provider_caching_behavior() {
        if skip_if_loopback_binds_restricted("test_multi_provider_caching_behavior").await {
            return;
        }
        logging::ensure_test_logging(None);

        // Create 2 mock servers
        let mock1 = MockServer::start().await;
        let mock2 = MockServer::start().await;

        // Setup health check responses
        for mock_server in &[&mock1, &mock2] {
            Mock::given(method("POST"))
                .and(path("/"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": "0x1234567"
                })))
                .up_to_n_times(1)
                .mount(mock_server)
                .await;
        }

        // Create provider manager
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("test_cache.json");
        let cache_manager = Arc::new(CacheManager::new(100, cache_path).unwrap());

        let provider_manager = Arc::new(
            ProviderManager::new(vec![mock1.uri(), mock2.uri()], 3)
                .await
                .expect("Failed to create provider manager"),
        );

        let metrics_collector = Arc::new(MetricsCollector::new());
        let handler = RpcHandler::new(provider_manager, cache_manager, metrics_collector).unwrap();

        let cacheable_response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "number": "0x1000000",
                "hash": "0x1234567890abcdef"
            }
        });

        // Setup expectations: Only ONE server should be hit for cacheable requests
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&cacheable_response))
            .up_to_n_times(1) // Should only be called once due to caching
            .mount(&mock1)
            .await;

        // mock2 should not be called at all for the cached request
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&cacheable_response))
            .up_to_n_times(1) // Should only be called once due to caching
            .mount(&mock2)
            .await;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber", // Cacheable method
            "params": ["0x1000000", false],
            "id": 1
        });

        // Send the same cacheable request 5 times
        for _ in 0..5 {
            let response = handler.handle_request(request.clone()).await.unwrap();
            assert_eq!(response, cacheable_response);
        }

        // The mock expectations will verify that only mock1 was called once
    }

    #[tokio::test]
    async fn test_rate_limit_detection() {
        if skip_if_loopback_binds_restricted("test_rate_limit_detection").await {
            return;
        }
        // Create test handler with minimal setup
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("test_cache.json");
        let cache_manager = Arc::new(CacheManager::new(100, cache_path).unwrap());

        // Mock server for testing (needed for provider manager init)
        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "0x1"
            })))
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        let provider_manager = Arc::new(
            ProviderManager::new(vec![mock_server.uri()], 3)
                .await
                .expect("Failed to create provider manager"),
        );

        let metrics_collector = Arc::new(MetricsCollector::new());
        let handler = RpcHandler::new(provider_manager, cache_manager, metrics_collector).unwrap();

        // Test various rate limit responses
        let test_cases = vec![
            // HTTP 429 status
            (StatusCode::TOO_MANY_REQUESTS, "{}", None, true),
            // HTTP 503 status
            (StatusCode::SERVICE_UNAVAILABLE, "{}", None, true),
            // Text pattern "rate limit"
            (StatusCode::OK, "rate limit exceeded", None, true),
            // Text pattern "cu limit exceeded" (Zan RPC)
            (StatusCode::OK, "cu limit exceeded; The current RPC traffic is too high", None, true),
            // JSON error with rate limit message
            (
                StatusCode::OK,
                "{}",
                Some(serde_json::json!({
                    "error": {
                        "code": -32005,
                        "message": "Too many requests"
                    }
                })),
                true,
            ),
            // JSON error with rate limit code
            (
                StatusCode::OK,
                "{}",
                Some(serde_json::json!({
                    "error": {
                        "code": 429,
                        "message": "Some error"
                    }
                })),
                true,
            ),
            // Normal response (not rate limited)
            (
                StatusCode::OK,
                "{}",
                Some(serde_json::json!({
                    "result": "0x1234"
                })),
                false,
            ),
            // Normal error (not rate limited)
            (
                StatusCode::OK,
                "{}",
                Some(serde_json::json!({
                    "error": {
                        "code": -32000,
                        "message": "Execution reverted"
                    }
                })),
                false,
            ),
        ];

        for (status, text, json, expected) in test_cases {
            let result = handler.is_rate_limit_response(status, text, json.as_ref());
            assert_eq!(
                result, expected,
                "Failed for status: {status:?}, text: {text}, json: {json:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_user_error_detection() {
        if skip_if_loopback_binds_restricted("test_user_error_detection").await {
            return;
        }
        // Similar minimal setup
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("test_cache.json");
        let cache_manager = Arc::new(CacheManager::new(100, cache_path).unwrap());

        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "0x1"
            })))
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        let provider_manager = Arc::new(
            ProviderManager::new(vec![mock_server.uri()], 3)
                .await
                .expect("Failed to create provider manager"),
        );

        let metrics_collector = Arc::new(MetricsCollector::new());
        let handler = RpcHandler::new(provider_manager, cache_manager, metrics_collector).unwrap();

        // Test various user error patterns
        let test_cases = vec![
            // Invalid Request
            (
                serde_json::json!({
                    "error": {
                        "code": -32600,
                        "message": "Invalid Request"
                    }
                }),
                true,
            ),
            // Method not found
            (
                serde_json::json!({
                    "error": {
                        "code": -32601,
                        "message": "Method not found"
                    }
                }),
                true,
            ),
            // Invalid params
            (
                serde_json::json!({
                    "error": {
                        "code": -32602,
                        "message": "Invalid params"
                    }
                }),
                true,
            ),
            // Parse error
            (
                serde_json::json!({
                    "error": {
                        "code": -32700,
                        "message": "Parse error"
                    }
                }),
                true,
            ),
            // Message-based detection
            (
                serde_json::json!({
                    "error": {
                        "code": -32000,
                        "message": "Invalid method eth_fooBar"
                    }
                }),
                true,
            ),
            // Blockchain execution error (not user error)
            (
                serde_json::json!({
                    "error": {
                        "code": -32000,
                        "message": "Execution reverted"
                    }
                }),
                false,
            ),
            // Rate limit error (not user error)
            (
                serde_json::json!({
                    "error": {
                        "code": 429,
                        "message": "Too many requests"
                    }
                }),
                false,
            ),
        ];

        for (response, expected) in test_cases {
            let result = handler.is_user_error(&response);
            assert_eq!(result, expected, "Failed for response: {response:?}");
        }
    }

    #[tokio::test]
    async fn test_rate_limit_fallback_to_healthy_provider() {
        if skip_if_loopback_binds_restricted("test_rate_limit_fallback_to_healthy_provider").await {
            return;
        }
        // Create 2 mock servers - first returns rate limit, second succeeds
        let mock1 = MockServer::start().await;
        let mock2 = MockServer::start().await;

        // Setup health check responses
        for mock_server in &[&mock1, &mock2] {
            Mock::given(method("POST"))
                .and(path("/"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": "0x1"
                })))
                .up_to_n_times(1)
                .mount(mock_server)
                .await;
        }

        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("test_cache.json");
        let cache_manager = Arc::new(CacheManager::new(100, cache_path).unwrap());

        let provider_manager = Arc::new(
            ProviderManager::new(vec![mock1.uri(), mock2.uri()], 3)
                .await
                .expect("Failed to create provider manager"),
        );

        let metrics_collector = Arc::new(MetricsCollector::new());
        let handler = RpcHandler::new(provider_manager, cache_manager, metrics_collector).unwrap();

        // First provider returns rate limit error
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(429).set_body_string("Rate limit exceeded"))
            .up_to_n_times(1)
            .mount(&mock1)
            .await;

        // Second provider returns success
        let success_response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": "0x12345"
        });

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&success_response))
            .expect(1)
            .mount(&mock2)
            .await;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });

        // Should get success from second provider despite first being rate limited
        let result = handler.handle_request(request).await.unwrap();
        assert_eq!(result, success_response);
    }

    #[tokio::test]
    async fn test_error_deduplication() {
        if skip_if_loopback_binds_restricted("test_error_deduplication").await {
            return;
        }
        // Create 3 mock servers that all return the same error
        let mock1 = MockServer::start().await;
        let mock2 = MockServer::start().await;
        let mock3 = MockServer::start().await;

        // Setup health checks
        for mock_server in &[&mock1, &mock2, &mock3] {
            Mock::given(method("POST"))
                .and(path("/"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": "0x1"
                })))
                .up_to_n_times(1)
                .mount(mock_server)
                .await;
        }

        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("test_cache.json");
        let cache_manager = Arc::new(CacheManager::new(100, cache_path).unwrap());

        let provider_manager = Arc::new(
            ProviderManager::new(vec![mock1.uri(), mock2.uri(), mock3.uri()], 3)
                .await
                .expect("Failed to create provider manager"),
        );

        let metrics_collector = Arc::new(MetricsCollector::new());
        let handler = RpcHandler::new(provider_manager, cache_manager, metrics_collector).unwrap();

        // All providers return the same blockchain error
        let error_response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32000,
                "message": "Execution reverted: insufficient balance"
            }
        });

        // First provider
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&error_response))
            .expect(1)
            .mount(&mock1)
            .await;

        // Second provider - same error should trigger early return
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&error_response))
            .expect(1)
            .mount(&mock2)
            .await;

        // Third provider WILL be called (need 3 matching errors now)
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&error_response))
            .expect(1)
            .mount(&mock3)
            .await;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{}, "0x1000000"],
            "id": 1
        });

        // Should return the error after 3 providers agree
        let result = handler.handle_request(request).await.unwrap();
        assert_eq!(result, error_response);
    }

    #[tokio::test]
    async fn test_eth_call_caching_with_deterministic_block() {
        let Some((handler, mock_server, _temp_dir)) =
            create_test_rpc_handler("test_eth_call_caching_with_deterministic_block").await
        else {
            return;
        };

        let response_data = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": "0x0000000000000000000000000000000000000000000000000000000000000001"
        });

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response_data))
            .expect(1) // Should only be called once due to caching
            .mount(&mock_server)
            .await;

        // eth_call with specific block number (should be cached)
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [
                {
                    "to": "0x1234567890123456789012345678901234567890",
                    "data": "0x"
                },
                "0x1000000"  // Specific block number
            ],
            "id": 1
        });

        // First request - should hit upstream
        let result1 = handler.handle_request(request.clone()).await.unwrap();
        assert_eq!(result1, response_data);

        // Second request - should hit cache
        let result2 = handler.handle_request(request).await.unwrap();
        assert_eq!(result2, response_data);

        // Mock server should only be called once
    }

    #[tokio::test]
    async fn test_eth_call_not_cached_with_latest() {
        let Some((handler, mock_server, _temp_dir)) =
            create_test_rpc_handler("test_eth_call_not_cached_with_latest").await
        else {
            return;
        };

        let response_data = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": "0x0000000000000000000000000000000000000000000000000000000000000001"
        });

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response_data))
            .expect(2) // Should be called twice (no caching)
            .mount(&mock_server)
            .await;

        // eth_call with "latest" (should NOT be cached)
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [
                {
                    "to": "0x1234567890123456789012345678901234567890",
                    "data": "0x"
                },
                "latest"  // Non-deterministic
            ],
            "id": 1
        });

        // Both requests should hit upstream
        let result1 = handler.handle_request(request.clone()).await.unwrap();
        assert_eq!(result1, response_data);

        let result2 = handler.handle_request(request).await.unwrap();
        assert_eq!(result2, response_data);

        // Mock server should be called twice
    }

    #[tokio::test]
    async fn test_debug_trace_validation_with_unsupported_response() {
        if skip_if_loopback_binds_restricted(
            "test_debug_trace_validation_with_unsupported_response",
        )
        .await
        {
            return;
        }
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("test_cache.json");
        let cache_manager = Arc::new(CacheManager::new(100, cache_path).unwrap());

        let mock_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "0x1"
            })))
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        let provider_manager = Arc::new(
            ProviderManager::new(vec![mock_server.uri()], 3)
                .await
                .expect("Failed to create provider manager"),
        );

        let metrics_collector = Arc::new(MetricsCollector::new());
        let handler = RpcHandler::new(provider_manager, cache_manager, metrics_collector).unwrap();

        // Test various unsupported responses
        let test_cases = vec![
            // String indicating not supported
            (
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": "method not supported"
                }),
                false,
            ),
            // Valid trace response
            (
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {
                        "output": "0x1234",
                        "gas": 21000
                    }
                }),
                true,
            ),
            // Empty object (invalid)
            (
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {}
                }),
                false,
            ),
            // Valid array response
            (
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": [{"action": "call"}]
                }),
                true,
            ),
        ];

        for (response, expected) in test_cases {
            let result = handler.is_valid_debug_trace_response(&response);
            assert_eq!(result, expected, "Failed for response: {response:?}");
        }
    }
}
