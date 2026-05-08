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

//! Multi-provider RPC management with health checking and load balancing

use eyre::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Default Ethereum mainnet RPC endpoints
/// These are free public endpoints from chainlist.org, sorted by latency
pub const DEFAULT_MAINNET_RPCS: &[&str] = &[
    "https://rpc.eth.gateway.fm",
    // "https://ethereum-rpc.publicnode.com", // disable due to publicnode's temporary issues
    "https://mainnet.gateway.tenderly.co",
    // "https://rpc.flashbots.net/fast", // disable due to flashbots' temporary issues
    // "https://rpc.flashbots.net", // disable due to flashbots' temporary issues
    "https://gateway.tenderly.co/public/mainnet",
    "https://eth-mainnet.public.blastapi.io",
    "https://ethereum-mainnet.gateway.tatum.io",
    "https://eth.api.onfinality.io/public",
    "https://eth.llamarpc.com",
    "https://api.zan.top/eth-mainnet",
    "https://eth.drpc.org",
    "https://ethereum.rpc.subquery.network/public",
];

/// Information about an RPC provider
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    /// The RPC endpoint URL
    pub url: String,
    /// Whether the provider is currently healthy
    pub is_healthy: bool,
    /// When the provider was last health checked
    pub last_health_check: Option<Instant>,
    /// Response time in milliseconds for the last successful request
    pub response_time_ms: Option<u64>,
    /// Number of consecutive failures (reset on success)
    pub consecutive_failures: u32,
}

/// Serializable version of ProviderInfo for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfoResponse {
    /// The RPC endpoint URL
    pub url: String,
    /// Whether the provider is currently healthy
    pub is_healthy: bool,
    /// Seconds since the last health check
    pub last_health_check_seconds_ago: Option<u64>,
    /// Response time in milliseconds for the last successful request
    pub response_time_ms: Option<u64>,
    /// Number of consecutive failures (reset on success)
    pub consecutive_failures: u32,
}

impl From<&ProviderInfo> for ProviderInfoResponse {
    fn from(info: &ProviderInfo) -> Self {
        Self {
            url: info.url.clone(),
            is_healthy: info.is_healthy,
            last_health_check_seconds_ago: info.last_health_check.map(|t| t.elapsed().as_secs()),
            response_time_ms: info.response_time_ms,
            consecutive_failures: info.consecutive_failures,
        }
    }
}

/// Multi-provider manager with health checking and round-robin load balancing
pub struct ProviderManager {
    /// List of all providers (healthy and unhealthy)
    providers: Arc<RwLock<Vec<ProviderInfo>>>,
    /// Round-robin counter for load balancing
    round_robin_counter: AtomicUsize,
    /// HTTP client for health checks
    client: reqwest::Client,
    /// Maximum consecutive failures before marking unhealthy
    max_failures: u32,
}

/// Calculate performance tier based on response time (100ms buckets)
/// Lower tier numbers indicate better performance
fn get_performance_tier(response_time_ms: u64) -> u8 {
    match response_time_ms / 100 {
        0..=1 => 1, // 0-199ms: Tier 1 (fastest)
        2..=3 => 2, // 200-399ms: Tier 2
        4..=5 => 3, // 400-599ms: Tier 3
        _ => 4,     // 600ms+: Tier 4 (slowest)
    }
}

/// Calculate weight for each performance tier
/// Higher weight means more likely to be selected
fn get_tier_weight(tier: u8) -> u32 {
    match tier {
        1 => 100, // Fast providers get 100% weight
        2 => 60,  // Medium providers get 60% weight
        3 => 30,  // Slow providers get 30% weight
        4 => 10,  // Very slow providers get 10% weight
        _ => 1,   // Fallback for unknown tiers
    }
}

impl ProviderManager {
    /// Create a new provider manager with the given RPC URLs
    pub async fn new(rpc_urls: Vec<String>, max_failures: u32) -> Result<Self> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build()?;

        // Initialize providers with parallel health checks
        let health_futures: Vec<_> = rpc_urls
            .iter()
            .map(|url| {
                let client = client.clone();
                let url = url.clone();
                async move {
                    let mut provider = ProviderInfo {
                        url: url.clone(),
                        is_healthy: false,
                        last_health_check: None,
                        response_time_ms: None,
                        consecutive_failures: 0,
                    };

                    match Self::check_provider_health(&client, &url).await {
                        Ok(response_time) => {
                            provider.is_healthy = true;
                            provider.response_time_ms = Some(response_time);
                            provider.last_health_check = Some(Instant::now());
                            info!("Provider {} is healthy ({}ms)", url, response_time);
                        }
                        Err(_) => {
                            warn!("Provider {} is not responding during initialization", url);
                            provider.consecutive_failures = 1;
                        }
                    }

                    provider
                }
            })
            .collect();

        let providers: Vec<ProviderInfo> = futures::future::join_all(health_futures).await;

        // Ensure at least one provider is healthy
        let healthy_count = providers.iter().filter(|p| p.is_healthy).count();
        if healthy_count == 0 {
            warn!("No healthy RPC providers available, we will use cache-only mode")
        }

        info!("Initialized with {} healthy providers out of {}", healthy_count, providers.len());

        Ok(Self {
            providers: Arc::new(RwLock::new(providers)),
            round_robin_counter: AtomicUsize::new(0),
            client,
            max_failures,
        })
    }

    /// Check the health of a specific provider
    async fn check_provider_health(client: &reqwest::Client, url: &str) -> Result<u64> {
        let start = Instant::now();

        // Simple eth_blockNumber request to check if provider is responsive
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });

        let response = client
            .post(url)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let response_time = start.elapsed().as_millis() as u64;

        // Check if we got a valid response
        let json: serde_json::Value = response.json().await?;
        if json.get("result").is_some() {
            Ok(response_time)
        } else {
            Err(eyre::eyre!("Invalid response from provider"))
        }
    }

    /// Get a weighted random provider that hasn't been tried yet
    /// Only considers healthy providers not in the exclusion set
    pub async fn get_weighted_provider_excluding(
        &self,
        tried_providers: &HashSet<String>,
    ) -> Option<String> {
        let providers = self.providers.read().await;
        let available_providers: Vec<_> = providers
            .iter()
            .filter(|p| p.is_healthy && !tried_providers.contains(&p.url))
            .collect();

        if available_providers.is_empty() {
            return None;
        }

        // If only one available provider, return it
        if available_providers.len() == 1 {
            return Some(available_providers[0].url.clone());
        }

        // Calculate weights for each available provider
        let mut weighted_providers = Vec::new();
        let mut total_weight = 0u32;

        for provider in &available_providers {
            // Use response time if available, otherwise assume medium performance
            let response_time = provider.response_time_ms.unwrap_or(300); // Default to 300ms
            let tier = get_performance_tier(response_time);
            let weight = get_tier_weight(tier);

            total_weight += weight;
            weighted_providers.push((provider, weight));
        }

        // Generate random number for weighted selection
        let random_weight = rand::random_range(0..total_weight);

        // Find the provider corresponding to the random weight
        let mut current_weight = 0u32;
        for (provider, weight) in weighted_providers {
            current_weight += weight;
            if random_weight < current_weight {
                return Some(provider.url.clone());
            }
        }

        // Fallback to first available provider (should not reach here)
        Some(available_providers[0].url.clone())
    }

    /// Get a weighted random provider based on response time
    /// Only considers healthy providers
    /// DEPRECATED: Use get_weighted_provider_excluding instead
    #[allow(dead_code)]
    async fn get_weighted_provider(&self) -> Option<String> {
        let providers = self.providers.read().await;
        let healthy_providers: Vec<_> = providers.iter().filter(|p| p.is_healthy).collect();

        if healthy_providers.is_empty() {
            return None;
        }

        // If only one healthy provider, return it
        if healthy_providers.len() == 1 {
            return Some(healthy_providers[0].url.clone());
        }

        // Calculate weights for each healthy provider
        let mut weighted_providers = Vec::new();
        let mut total_weight = 0u32;

        for provider in &healthy_providers {
            // Use response time if available, otherwise assume medium performance
            let response_time = provider.response_time_ms.unwrap_or(300); // Default to 300ms
            let tier = get_performance_tier(response_time);
            let weight = get_tier_weight(tier);

            total_weight += weight;
            weighted_providers.push((provider, weight));
        }

        // Generate random number for weighted selection
        let random_weight = rand::random_range(0..total_weight);

        // Find the provider corresponding to the random weight
        let mut current_weight = 0u32;
        for (provider, weight) in weighted_providers {
            current_weight += weight;
            if random_weight < current_weight {
                return Some(provider.url.clone());
            }
        }

        // Fallback to first healthy provider (should not reach here)
        Some(healthy_providers[0].url.clone())
    }

    /// Get the next healthy provider using round-robin
    /// DEPRECATED: Use get_weighted_provider_excluding instead
    #[allow(dead_code)]
    pub async fn get_next_provider(&self) -> Option<String> {
        let providers = self.providers.read().await;
        let healthy_providers: Vec<_> = providers.iter().filter(|p| p.is_healthy).collect();

        if healthy_providers.is_empty() {
            return None;
        }

        // Round-robin selection
        let index =
            self.round_robin_counter.fetch_add(1, Ordering::Relaxed) % healthy_providers.len();
        Some(healthy_providers[index].url.clone())
    }

    /// Mark a provider as failed and update its health status
    pub async fn mark_provider_failed(&self, url: &str) {
        let mut providers = self.providers.write().await;

        if let Some(provider) = providers.iter_mut().find(|p| p.url == url) {
            provider.consecutive_failures += 1;

            if provider.consecutive_failures >= self.max_failures {
                provider.is_healthy = false;
                debug!("Provider {} marked as unhealthy after {} failures", url, self.max_failures);
            }
        }
    }

    /// Mark a provider as successful and reset failure count
    pub async fn mark_provider_success(&self, url: &str, response_time_ms: u64) {
        let mut providers = self.providers.write().await;

        if let Some(provider) = providers.iter_mut().find(|p| p.url == url) {
            provider.consecutive_failures = 0;
            provider.is_healthy = true;
            provider.response_time_ms = Some(response_time_ms);
            provider.last_health_check = Some(Instant::now());

            debug!("Provider {} successful ({}ms)", url, response_time_ms);
        }
    }

    /// Perform health checks on all providers
    pub async fn health_check_all(&self) {
        let providers_snapshot = {
            let providers = self.providers.read().await;
            providers.clone()
        };

        let health_futures: Vec<_> = providers_snapshot
            .into_iter()
            .filter_map(|provider| {
                // Check if provider needs health check (unhealthy or stale)
                let needs_check = !provider.is_healthy
                    || provider
                        .last_health_check
                        .is_none_or(|t| t.elapsed() > Duration::from_secs(60));

                if needs_check {
                    let client = self.client.clone();
                    let url = provider.url.clone();
                    let was_healthy = provider.is_healthy;

                    Some(async move {
                        match Self::check_provider_health(&client, &url).await {
                            Ok(response_time) => {
                                if !was_healthy {
                                    debug!("Provider {} is now healthy", url);
                                }
                                (url, Ok(response_time))
                            }
                            Err(e) => {
                                debug!("Health check failed for {}: {}", url, e);
                                (url, Err(()))
                            }
                        }
                    })
                } else {
                    None
                }
            })
            .collect();

        let results = futures::future::join_all(health_futures).await;

        for (url, result) in results {
            match result {
                Ok(response_time) => {
                    self.mark_provider_success(&url, response_time).await;
                }
                Err(()) => {
                    self.mark_provider_failed(&url).await;
                }
            }
        }
    }

    /// Get information about all providers (serializable version)
    pub async fn get_providers_info(&self) -> Vec<ProviderInfoResponse> {
        let providers = self.providers.read().await;
        providers.iter().map(|p| p.into()).collect()
    }

    /// Get count of healthy providers
    pub async fn healthy_provider_count(&self) -> usize {
        let providers = self.providers.read().await;
        providers.iter().filter(|p| p.is_healthy).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
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

    #[tokio::test]
    async fn test_provider_initialization() {
        if skip_if_loopback_binds_restricted("test_provider_initialization").await {
            return;
        }
        edb_common::logging::ensure_test_logging(None);
        info!("Testing provider initialization with health checks");

        // Start mock servers
        let mock1 = MockServer::start().await;
        let mock2 = MockServer::start().await;

        // Setup successful response for mock1
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "0x1234567"
            })))
            .mount(&mock1)
            .await;

        // Setup failed response for mock2
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock2)
            .await;

        let urls = vec![mock1.uri(), mock2.uri()];
        let manager = ProviderManager::new(urls, 3).await.unwrap();

        // Check that only mock1 is healthy
        assert_eq!(manager.healthy_provider_count().await, 1);

        let providers = manager.get_providers_info().await;
        assert_eq!(providers.len(), 2);
        assert!(providers[0].is_healthy);
        assert!(!providers[1].is_healthy);
    }

    #[tokio::test]
    async fn test_round_robin_selection() {
        if skip_if_loopback_binds_restricted("test_round_robin_selection").await {
            return;
        }
        edb_common::logging::ensure_test_logging(None);
        info!("Testing round-robin provider selection");

        // Start 3 healthy mock servers
        let mocks =
            vec![MockServer::start().await, MockServer::start().await, MockServer::start().await];

        for mock in &mocks {
            Mock::given(method("POST"))
                .and(path("/"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": "0x1234567"
                })))
                .mount(mock)
                .await;
        }

        let urls: Vec<String> = mocks.iter().map(|m| m.uri()).collect();
        let manager = ProviderManager::new(urls.clone(), 3).await.unwrap();

        // Get providers multiple times and verify round-robin
        let mut selections = Vec::new();
        for _ in 0..9 {
            selections.push(manager.get_next_provider().await.unwrap());
        }

        // Each provider should be selected 3 times
        for url in &urls {
            assert_eq!(selections.iter().filter(|s| *s == url).count(), 3);
        }
    }

    #[tokio::test]
    async fn test_provider_failure_handling() {
        if skip_if_loopback_binds_restricted("test_provider_failure_handling").await {
            return;
        }
        edb_common::logging::ensure_test_logging(None);
        debug!("Testing provider failure detection and handling");

        let mock = MockServer::start().await;

        // Initially healthy
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": "0x1234567"
            })))
            .expect(1)
            .mount(&mock)
            .await;

        let manager = ProviderManager::new(vec![mock.uri()], 2).await.unwrap();
        assert_eq!(manager.healthy_provider_count().await, 1);

        // Mark as failed twice (max_failures = 2)
        manager.mark_provider_failed(&mock.uri()).await;
        assert_eq!(manager.healthy_provider_count().await, 1); // Still healthy after 1 failure

        manager.mark_provider_failed(&mock.uri()).await;
        assert_eq!(manager.healthy_provider_count().await, 0); // Unhealthy after 2 failures
    }

    #[tokio::test]
    async fn test_weighted_provider_selection() {
        if skip_if_loopback_binds_restricted("test_weighted_provider_selection").await {
            return;
        }
        edb_common::logging::ensure_test_logging(None);
        debug!("Testing weighted provider selection based on response time");

        // Start 3 healthy mock servers with different response times
        let mocks =
            vec![MockServer::start().await, MockServer::start().await, MockServer::start().await];

        for mock in &mocks {
            Mock::given(method("POST"))
                .and(path("/"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": "0x1234567"
                })))
                .mount(mock)
                .await;
        }

        let urls: Vec<String> = mocks.iter().map(|m| m.uri()).collect();
        let manager = ProviderManager::new(urls.clone(), 3).await.unwrap();

        // Simulate different response times for providers
        manager.mark_provider_success(&urls[0], 50).await; // Fast: Tier 1 (100 weight)
        manager.mark_provider_success(&urls[1], 250).await; // Medium: Tier 2 (60 weight)
        manager.mark_provider_success(&urls[2], 500).await; // Slow: Tier 3 (30 weight)

        // Test weighted selection multiple times
        let mut selections = std::collections::HashMap::new();
        for _ in 0..100 {
            if let Some(provider) = manager.get_weighted_provider().await {
                *selections.entry(provider).or_insert(0) += 1;
            }
        }

        // Verify all providers were selected
        assert_eq!(selections.len(), 3);

        // Fast provider should be selected most often due to higher weight
        let fast_count = selections.get(&urls[0]).unwrap_or(&0);
        let medium_count = selections.get(&urls[1]).unwrap_or(&0);
        let slow_count = selections.get(&urls[2]).unwrap_or(&0);

        debug!(
            "Selection counts - Fast: {}, Medium: {}, Slow: {}",
            fast_count, medium_count, slow_count
        );

        // Fast provider should have more selections than slow provider
        assert!(
            fast_count > slow_count,
            "Fast provider should be selected more often than slow provider"
        );
    }
}
