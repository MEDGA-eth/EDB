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

//! Comprehensive metrics collection for RPC proxy performance monitoring

use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// Method-level performance statistics
///
/// Tracks comprehensive performance metrics for individual RPC methods,
/// including cache performance, response times, and error rates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodStats {
    /// Number of cache hits for this method (served from cache)
    pub hits: u64,
    /// Number of requests forwarded to upstream providers (cache misses + non-cacheable)
    pub misses: u64,
    /// Total number of requests for this method (hits + misses)
    pub total_requests: u64,
    /// Average response time in milliseconds
    pub avg_response_time_ms: f64,
    /// Total response time in milliseconds (used for average calculation)
    pub total_response_time_ms: u64,
    /// Number of errors encountered for this method
    pub errors: u64,
}

impl Default for MethodStats {
    fn default() -> Self {
        Self {
            hits: 0,
            misses: 0,
            total_requests: 0,
            avg_response_time_ms: 0.0,
            total_response_time_ms: 0,
            errors: 0,
        }
    }
}

impl MethodStats {
    /// Calculate the cache hit rate as a percentage (0.0 to 100.0)
    #[allow(dead_code)]
    pub fn hit_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            (self.hits as f64 / self.total_requests as f64) * 100.0
        }
    }

    /// Calculate the error rate as a percentage (0.0 to 100.0)
    #[allow(dead_code)]
    pub fn error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            (self.errors as f64 / self.total_requests as f64) * 100.0
        }
    }

    /// Update the average response time based on current totals
    pub fn update_avg_response_time(&mut self) {
        if self.total_requests > 0 {
            self.avg_response_time_ms =
                self.total_response_time_ms as f64 / self.total_requests as f64;
        }
    }
}

/// Provider usage analytics
///
/// Tracks detailed usage statistics for individual RPC providers,
/// including performance metrics and historical response times.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderUsage {
    /// Total number of requests sent to this provider
    pub request_count: u64,
    /// Total response time in milliseconds (used for average calculation)
    pub total_response_time_ms: u64,
    /// Number of successful requests
    pub success_count: u64,
    /// Number of failed requests
    pub error_count: u64,
    /// Unix timestamp of the last request to this provider
    pub last_used_timestamp: u64,
    /// Recent response times (limited to last 100) for histogram analysis
    pub response_time_history: VecDeque<u64>,
}

impl Default for ProviderUsage {
    fn default() -> Self {
        Self {
            request_count: 0,
            total_response_time_ms: 0,
            success_count: 0,
            error_count: 0,
            last_used_timestamp: 0,
            response_time_history: VecDeque::with_capacity(100),
        }
    }
}

impl ProviderUsage {
    /// Calculate the average response time in milliseconds
    pub fn avg_response_time_ms(&self) -> f64 {
        if self.request_count == 0 {
            0.0
        } else {
            self.total_response_time_ms as f64 / self.request_count as f64
        }
    }

    /// Calculate the success rate as a percentage (0.0 to 100.0)
    pub fn success_rate(&self) -> f64 {
        if self.request_count == 0 {
            0.0
        } else {
            (self.success_count as f64 / self.request_count as f64) * 100.0
        }
    }

    /// Calculate this provider's load as a percentage of total requests
    pub fn load_percentage(&self, total_requests: u64) -> f64 {
        if total_requests == 0 {
            0.0
        } else {
            (self.request_count as f64 / total_requests as f64) * 100.0
        }
    }

    /// Record a request to this provider with response time and success status
    pub fn record_request(&mut self, response_time_ms: u64, success: bool) {
        self.request_count += 1;
        self.total_response_time_ms += response_time_ms;
        self.last_used_timestamp =
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        if success {
            self.success_count += 1;
        } else {
            self.error_count += 1;
        }

        // Keep only last 100 response times for histogram analysis
        self.response_time_history.push_back(response_time_ms);
        if self.response_time_history.len() > 100 {
            self.response_time_history.pop_front();
        }
    }
}

/// Historical metric data point for time-series analysis
///
/// Represents a snapshot of system metrics at a specific point in time,
/// used for trend analysis and historical monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalMetric {
    /// Unix timestamp when this metric was recorded
    pub timestamp: u64,
    /// Number of cache hits at this point in time
    pub cache_hits: u64,
    /// Number of cache misses at this point in time
    pub cache_misses: u64,
    /// Current cache size (number of entries)
    pub cache_size: u64,
    /// Number of healthy providers at this point in time
    pub healthy_providers: u64,
    /// Total number of configured providers
    pub total_providers: u64,
    /// Request rate (requests per minute) at this point in time
    pub requests_per_minute: u64,
    /// Average response time across all methods and providers
    pub avg_response_time_ms: f64,
    /// Number of active EDB instances connected
    pub active_instances: usize,
}

/// Comprehensive metrics collector for the RPC proxy
///
/// Thread-safe metrics collection system that tracks cache performance,
/// provider usage, method-level statistics, and historical trends.
/// Uses atomic operations and read-write locks for concurrent access.
#[derive(Debug)]
pub struct MetricsCollector {
    // Cache metrics - atomic for high-performance concurrent access
    /// Total number of cache hits across all methods (served from cache)
    pub cache_hits: AtomicU64,
    /// Total number of cache misses that required checking the cache (excludes non-cacheable)
    pub cache_misses: AtomicU64,
    /// Total number of requests processed (all requests)
    pub total_requests: AtomicU64,

    // Provider metrics - protected by RwLock for complex operations
    /// Per-provider usage statistics and performance metrics
    pub provider_usage: Arc<RwLock<HashMap<String, ProviderUsage>>>,

    // Method-level metrics - protected by RwLock for complex operations
    /// Per-method performance statistics and cache effectiveness
    pub method_stats: Arc<RwLock<HashMap<String, MethodStats>>>,

    // Historical data (limited to 1000 points to prevent memory leaks)
    /// Time-series data for trend analysis and monitoring dashboards
    pub metrics_history: Arc<RwLock<VecDeque<HistoricalMetric>>>,

    // Request rate tracking (timestamps of last 1000 requests)
    /// Recent request timestamps for calculating requests-per-minute
    pub request_timestamps: Arc<RwLock<VecDeque<u64>>>,

    // Error tracking - atomic for high-performance concurrent access
    /// Total number of errors across all request types
    pub total_errors: AtomicU64,
    /// Number of rate limiting errors encountered
    pub rate_limit_errors: AtomicU64,
    /// Number of user-caused errors (4xx responses)
    pub user_errors: AtomicU64,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            total_requests: AtomicU64::new(0),
            provider_usage: Arc::new(RwLock::new(HashMap::new())),
            method_stats: Arc::new(RwLock::new(HashMap::new())),
            metrics_history: Arc::new(RwLock::new(VecDeque::with_capacity(1000))),
            request_timestamps: Arc::new(RwLock::new(VecDeque::with_capacity(1000))),
            total_errors: AtomicU64::new(0),
            rate_limit_errors: AtomicU64::new(0),
            user_errors: AtomicU64::new(0),
        }
    }

    /// Record a cache hit for a specific method
    pub fn record_cache_hit(&self, method: &str, response_time_ms: u64) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);

        // XXX (ZZ): since we will not go through the forward_request when cache is hit,
        // we need to update the following metrics accordingly
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.record_request_timestamp();

        // Update method stats
        if let Ok(mut stats) = self.method_stats.write() {
            let method_stat = stats.entry(method.to_string()).or_default();
            method_stat.hits += 1;
            method_stat.total_requests += 1;
            method_stat.total_response_time_ms += response_time_ms;
            method_stat.update_avg_response_time();
        }
    }

    /// Record a cache miss for a specific method
    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a request that was forwarded to an upstream provider
    /// This includes both cache misses and non-cacheable requests
    pub fn record_request(
        &self,
        method: &str,
        provider_url: &str,
        response_time_ms: u64,
        success: bool,
    ) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.record_request_timestamp();

        if !success {
            self.total_errors.fetch_add(1, Ordering::Relaxed);
        }

        // Update method stats
        if let Ok(mut stats) = self.method_stats.write() {
            let method_stat = stats.entry(method.to_string()).or_default();
            method_stat.misses += 1; // Any forwarded request counts as a miss
            method_stat.total_requests += 1;
            method_stat.total_response_time_ms += response_time_ms;
            if !success {
                method_stat.errors += 1;
            }
            method_stat.update_avg_response_time();
        }

        // Update provider usage
        if let Ok(mut usage) = self.provider_usage.write() {
            let provider_usage = usage.entry(provider_url.to_string()).or_default();
            provider_usage.record_request(response_time_ms, success);
        }
    }

    /// Record an error by type
    pub fn record_error(&self, error_type: ErrorType) {
        match error_type {
            ErrorType::RateLimit => self.rate_limit_errors.fetch_add(1, Ordering::Relaxed),
            ErrorType::UserError => self.user_errors.fetch_add(1, Ordering::Relaxed),
            ErrorType::Other => self.total_errors.fetch_add(1, Ordering::Relaxed),
        };
    }

    /// Record a request timestamp for rate calculation
    fn record_request_timestamp(&self) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

        if let Ok(mut timestamps) = self.request_timestamps.write() {
            timestamps.push_back(now);
            // Keep only last 1000 timestamps (about 1 hour at 1 req/sec)
            if timestamps.len() > 1000 {
                timestamps.pop_front();
            }
        }
    }

    /// Calculate requests per minute based on recent timestamps
    pub fn requests_per_minute(&self) -> u64 {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let one_minute_ago = now - 60;

        if let Ok(timestamps) = self.request_timestamps.read() {
            timestamps.iter().filter(|&&ts| ts >= one_minute_ago).count() as u64
        } else {
            0
        }
    }

    /// Get overall cache hit rate as percentage
    pub fn cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let total = self.total_requests.load(Ordering::Relaxed);

        if total == 0 { 0.0 } else { (hits as f64 / total as f64) * 100.0 }
    }

    /// Get error rate as percentage
    pub fn error_rate(&self) -> f64 {
        let errors = self.total_errors.load(Ordering::Relaxed);
        let total = self.total_requests.load(Ordering::Relaxed);

        if total == 0 { 0.0 } else { (errors as f64 / total as f64) * 100.0 }
    }

    /// Add a historical data point
    pub fn add_historical_point(
        &self,
        cache_size: u64,
        healthy_providers: u64,
        total_providers: u64,
        active_instances: usize,
    ) {
        let metric = HistoricalMetric {
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            cache_size,
            healthy_providers,
            total_providers,
            requests_per_minute: self.requests_per_minute(),
            avg_response_time_ms: self.overall_avg_response_time(),
            active_instances,
        };

        if let Ok(mut history) = self.metrics_history.write() {
            history.push_back(metric);
            // Keep only last 1000 data points to prevent memory growth
            if history.len() > 1000 {
                history.pop_front();
            }
        }
    }

    /// Calculate overall average response time
    fn overall_avg_response_time(&self) -> f64 {
        if let Ok(stats) = self.method_stats.read() {
            let total_time: u64 = stats.values().map(|s| s.total_response_time_ms).sum();
            let total_requests: u64 = stats.values().map(|s| s.total_requests).sum();

            if total_requests > 0 { total_time as f64 / total_requests as f64 } else { 0.0 }
        } else {
            0.0
        }
    }

    /// Get method statistics as a cloned HashMap
    pub fn get_method_stats(&self) -> HashMap<String, MethodStats> {
        self.method_stats
            .read()
            .unwrap_or_else(|_| {
                std::thread::yield_now();
                self.method_stats.read().expect("Failed to acquire method stats lock")
            })
            .clone()
    }

    /// Get provider usage statistics as a cloned HashMap
    pub fn get_provider_usage(&self) -> HashMap<String, ProviderUsage> {
        self.provider_usage
            .read()
            .unwrap_or_else(|_| {
                std::thread::yield_now();
                self.provider_usage.read().expect("Failed to acquire provider usage lock")
            })
            .clone()
    }

    /// Get historical metrics as a cloned VecDeque
    pub fn get_metrics_history(&self) -> VecDeque<HistoricalMetric> {
        self.metrics_history
            .read()
            .unwrap_or_else(|_| {
                std::thread::yield_now();
                self.metrics_history.read().expect("Failed to acquire metrics history lock")
            })
            .clone()
    }
}

/// Error type classification for metrics
///
/// Categorizes different types of errors for detailed error analysis
/// and monitoring. This helps identify systemic issues vs user errors.
#[derive(Debug, Clone, Copy)]
pub enum ErrorType {
    /// Rate limiting errors (429 responses)
    RateLimit,
    /// User-caused errors (4xx responses except 429)
    UserError,
    /// Other system errors (5xx responses, network errors, etc.)
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_method_stats_default() {
        let stats = MethodStats::default();
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.misses, 0);
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.avg_response_time_ms, 0.0);
        assert_eq!(stats.total_response_time_ms, 0);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn test_method_stats_hit_rate() {
        let mut stats = MethodStats::default();

        // No requests - should return 0%
        assert_eq!(stats.hit_rate(), 0.0);

        // 50% hit rate
        stats.hits = 5;
        stats.total_requests = 10;
        assert_eq!(stats.hit_rate(), 50.0);

        // 100% hit rate
        stats.hits = 10;
        stats.total_requests = 10;
        assert_eq!(stats.hit_rate(), 100.0);

        // 0% hit rate
        stats.hits = 0;
        stats.total_requests = 10;
        assert_eq!(stats.hit_rate(), 0.0);
    }

    #[test]
    fn test_method_stats_error_rate() {
        let mut stats = MethodStats::default();

        // No requests - should return 0%
        assert_eq!(stats.error_rate(), 0.0);

        // 20% error rate
        stats.errors = 2;
        stats.total_requests = 10;
        assert_eq!(stats.error_rate(), 20.0);

        // 100% error rate
        stats.errors = 10;
        stats.total_requests = 10;
        assert_eq!(stats.error_rate(), 100.0);
    }

    #[test]
    fn test_method_stats_update_avg_response_time() {
        let mut stats = MethodStats::default();

        // No requests - average should remain 0
        stats.update_avg_response_time();
        assert_eq!(stats.avg_response_time_ms, 0.0);

        // With requests - should calculate correct average
        stats.total_response_time_ms = 500;
        stats.total_requests = 5;
        stats.update_avg_response_time();
        assert_eq!(stats.avg_response_time_ms, 100.0);

        // Updated totals
        stats.total_response_time_ms = 1200;
        stats.total_requests = 4;
        stats.update_avg_response_time();
        assert_eq!(stats.avg_response_time_ms, 300.0);
    }

    #[test]
    fn test_provider_usage_default() {
        let usage = ProviderUsage::default();
        assert_eq!(usage.request_count, 0);
        assert_eq!(usage.total_response_time_ms, 0);
        assert_eq!(usage.success_count, 0);
        assert_eq!(usage.error_count, 0);
        assert_eq!(usage.last_used_timestamp, 0);
        assert_eq!(usage.response_time_history.len(), 0);
        assert_eq!(usage.response_time_history.capacity(), 100);
    }

    #[test]
    fn test_provider_usage_avg_response_time() {
        let mut usage = ProviderUsage::default();

        // No requests - should return 0
        assert_eq!(usage.avg_response_time_ms(), 0.0);

        // With requests
        usage.total_response_time_ms = 1000;
        usage.request_count = 5;
        assert_eq!(usage.avg_response_time_ms(), 200.0);
    }

    #[test]
    fn test_provider_usage_success_rate() {
        let mut usage = ProviderUsage::default();

        // No requests - should return 0%
        assert_eq!(usage.success_rate(), 0.0);

        // 80% success rate
        usage.success_count = 8;
        usage.request_count = 10;
        assert_eq!(usage.success_rate(), 80.0);

        // 100% success rate
        usage.success_count = 10;
        usage.request_count = 10;
        assert_eq!(usage.success_rate(), 100.0);
    }

    #[test]
    fn test_provider_usage_load_percentage() {
        let mut usage = ProviderUsage::default();

        // No total requests - should return 0%
        assert_eq!(usage.load_percentage(0), 0.0);

        // 25% of total load
        usage.request_count = 25;
        assert_eq!(usage.load_percentage(100), 25.0);

        // 100% of total load
        usage.request_count = 100;
        assert_eq!(usage.load_percentage(100), 100.0);
    }

    #[test]
    fn test_provider_usage_record_request() {
        let mut usage = ProviderUsage::default();
        let initial_timestamp = usage.last_used_timestamp;

        // Record successful request
        usage.record_request(150, true);
        assert_eq!(usage.request_count, 1);
        assert_eq!(usage.total_response_time_ms, 150);
        assert_eq!(usage.success_count, 1);
        assert_eq!(usage.error_count, 0);
        assert!(usage.last_used_timestamp > initial_timestamp);
        assert_eq!(usage.response_time_history.len(), 1);
        assert_eq!(usage.response_time_history[0], 150);

        // Record failed request
        usage.record_request(300, false);
        assert_eq!(usage.request_count, 2);
        assert_eq!(usage.total_response_time_ms, 450);
        assert_eq!(usage.success_count, 1);
        assert_eq!(usage.error_count, 1);
        assert_eq!(usage.response_time_history.len(), 2);
        assert_eq!(usage.response_time_history[1], 300);
    }

    #[test]
    fn test_provider_usage_response_time_history_limit() {
        let mut usage = ProviderUsage::default();

        // Fill history beyond capacity
        for i in 0..150 {
            usage.record_request(i, true);
        }

        // Should be limited to 100 entries
        assert_eq!(usage.response_time_history.len(), 100);

        // Should contain the most recent 100 entries (50-149)
        assert_eq!(usage.response_time_history.front(), Some(&50));
        assert_eq!(usage.response_time_history.back(), Some(&149));
    }

    #[test]
    fn test_metrics_collector_new() {
        let collector = MetricsCollector::new();

        assert_eq!(collector.cache_hits.load(Ordering::Relaxed), 0);
        assert_eq!(collector.cache_misses.load(Ordering::Relaxed), 0);
        assert_eq!(collector.total_requests.load(Ordering::Relaxed), 0);
        assert_eq!(collector.total_errors.load(Ordering::Relaxed), 0);
        assert_eq!(collector.rate_limit_errors.load(Ordering::Relaxed), 0);
        assert_eq!(collector.user_errors.load(Ordering::Relaxed), 0);

        assert!(collector.provider_usage.read().unwrap().is_empty());
        assert!(collector.method_stats.read().unwrap().is_empty());
        assert!(collector.metrics_history.read().unwrap().is_empty());
        assert!(collector.request_timestamps.read().unwrap().is_empty());
    }

    #[test]
    fn test_metrics_collector_record_cache_hit() {
        let collector = MetricsCollector::new();

        collector.record_cache_hit("eth_getBalance", 50);

        assert_eq!(collector.cache_hits.load(Ordering::Relaxed), 1);
        assert_eq!(collector.cache_misses.load(Ordering::Relaxed), 0);
        assert_eq!(collector.total_requests.load(Ordering::Relaxed), 1);

        // Check method stats
        let stats = collector.get_method_stats();
        assert_eq!(stats.len(), 1);
        let method_stat = stats.get("eth_getBalance").unwrap();
        assert_eq!(method_stat.hits, 1);
        assert_eq!(method_stat.misses, 0);
        assert_eq!(method_stat.total_requests, 1);
        assert_eq!(method_stat.total_response_time_ms, 50);
        assert_eq!(method_stat.avg_response_time_ms, 50.0);
        assert_eq!(method_stat.errors, 0);

        // Check request timestamp recorded
        assert_eq!(collector.request_timestamps.read().unwrap().len(), 1);
    }

    #[test]
    fn test_metrics_collector_record_cache_miss() {
        let collector = MetricsCollector::new();
        let provider_url = "https://eth.llamarpc.com";

        // Record a cache miss (just increments the counter)
        collector.record_cache_miss();

        // Then record the forwarded request
        collector.record_request("eth_getBlockByNumber", provider_url, 200, true);

        assert_eq!(collector.cache_hits.load(Ordering::Relaxed), 0);
        assert_eq!(collector.cache_misses.load(Ordering::Relaxed), 1);
        assert_eq!(collector.total_requests.load(Ordering::Relaxed), 1);
        assert_eq!(collector.total_errors.load(Ordering::Relaxed), 0);

        // Check method stats
        let stats = collector.get_method_stats();
        let method_stat = stats.get("eth_getBlockByNumber").unwrap();
        assert_eq!(method_stat.hits, 0);
        assert_eq!(method_stat.misses, 1);
        assert_eq!(method_stat.total_requests, 1);
        assert_eq!(method_stat.total_response_time_ms, 200);
        assert_eq!(method_stat.errors, 0);

        // Check provider usage
        let usage = collector.get_provider_usage();
        let provider_usage = usage.get(provider_url).unwrap();
        assert_eq!(provider_usage.request_count, 1);
        assert_eq!(provider_usage.success_count, 1);
        assert_eq!(provider_usage.error_count, 0);
        assert_eq!(provider_usage.total_response_time_ms, 200);
    }

    #[test]
    fn test_metrics_collector_record_cache_miss_with_error() {
        let collector = MetricsCollector::new();
        let provider_url = "https://failing-provider.com";

        // Record a cache miss
        collector.record_cache_miss();

        // Then record the failed forwarded request
        collector.record_request("eth_getBalance", provider_url, 5000, false);

        assert_eq!(collector.cache_misses.load(Ordering::Relaxed), 1);
        assert_eq!(collector.total_errors.load(Ordering::Relaxed), 1);

        // Check method stats include error
        let stats = collector.get_method_stats();
        let method_stat = stats.get("eth_getBalance").unwrap();
        assert_eq!(method_stat.errors, 1);

        // Check provider usage includes error
        let usage = collector.get_provider_usage();
        let provider_usage = usage.get(provider_url).unwrap();
        assert_eq!(provider_usage.error_count, 1);
        assert_eq!(provider_usage.success_count, 0);
    }

    #[test]
    fn test_metrics_collector_record_request() {
        let collector = MetricsCollector::new();
        let provider_url = "https://eth.llamarpc.com";

        // Record a non-cacheable request (directly forwarded)
        collector.record_request("eth_sendRawTransaction", provider_url, 100, true);

        assert_eq!(collector.cache_hits.load(Ordering::Relaxed), 0);
        assert_eq!(collector.cache_misses.load(Ordering::Relaxed), 0);
        assert_eq!(collector.total_requests.load(Ordering::Relaxed), 1);

        // Method should be tracked with misses incremented (forwarded request)
        let stats = collector.get_method_stats();
        let method_stat = stats.get("eth_sendRawTransaction").unwrap();
        assert_eq!(method_stat.hits, 0);
        assert_eq!(method_stat.misses, 1); // Forwarded requests count as misses
        assert_eq!(method_stat.total_requests, 1);
    }

    #[test]
    fn test_metrics_collector_record_error() {
        let collector = MetricsCollector::new();

        collector.record_error(ErrorType::RateLimit);
        collector.record_error(ErrorType::UserError);
        collector.record_error(ErrorType::Other);

        assert_eq!(collector.rate_limit_errors.load(Ordering::Relaxed), 1);
        assert_eq!(collector.user_errors.load(Ordering::Relaxed), 1);
        assert_eq!(collector.total_errors.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_metrics_collector_cache_hit_rate() {
        let collector = MetricsCollector::new();

        // No requests - should return 0%
        assert_eq!(collector.cache_hit_rate(), 0.0);

        // Record some hits and misses
        collector.record_cache_hit("method1", 50);
        collector.record_cache_hit("method2", 75);

        // Record cache misses followed by forwarded requests
        collector.record_cache_miss();
        collector.record_request("method3", "provider1", 100, true);

        collector.record_cache_miss();
        collector.record_request("method4", "provider1", 125, true);

        // Direct forwarded request (non-cacheable)
        collector.record_request("method5", "provider1", 150, true);

        // 2 hits out of 5 total requests = 40%
        assert_eq!(collector.cache_hit_rate(), 40.0);
    }

    #[test]
    fn test_metrics_collector_error_rate() {
        let collector = MetricsCollector::new();

        // No requests - should return 0%
        assert_eq!(collector.error_rate(), 0.0);

        // Record some requests with errors
        collector.record_cache_hit("method1", 50); // Success

        // Cache miss with error
        collector.record_cache_miss();
        collector.record_request("method2", "provider1", 100, false); // Error

        collector.record_request("method3", "provider1", 150, true); // Success
        collector.record_request("method4", "provider1", 200, false); // Error

        // 2 errors out of 4 total requests = 50%
        assert_eq!(collector.error_rate(), 50.0);
    }

    #[test]
    fn test_metrics_collector_requests_per_minute() {
        let collector = MetricsCollector::new();

        // No requests initially
        assert_eq!(collector.requests_per_minute(), 0);

        // Record some requests
        collector.record_cache_hit("method1", 50);

        collector.record_cache_miss();
        collector.record_request("method2", "provider1", 100, true);

        // Should count recent requests (within last minute)
        assert_eq!(collector.requests_per_minute(), 2);
    }

    #[test]
    fn test_metrics_collector_add_historical_point() {
        let collector = MetricsCollector::new();

        // Add some metrics first
        collector.record_cache_hit("method1", 100);

        collector.record_cache_miss();
        collector.record_request("method2", "provider1", 200, true);

        collector.add_historical_point(1000, 3, 5, 2);

        let history = collector.get_metrics_history();
        assert_eq!(history.len(), 1);

        let point = &history[0];
        assert_eq!(point.cache_hits, 1);
        assert_eq!(point.cache_misses, 1);
        assert_eq!(point.cache_size, 1000);
        assert_eq!(point.healthy_providers, 3);
        assert_eq!(point.total_providers, 5);
        assert_eq!(point.active_instances, 2);
        assert!(point.timestamp > 0);
        assert!(point.avg_response_time_ms > 0.0);
    }

    #[test]
    fn test_metrics_collector_historical_point_limit() {
        let collector = MetricsCollector::new();

        // Add more than the limit (1000)
        for i in 0..1100 {
            collector.add_historical_point(i, 1, 1, 1);
        }

        let history = collector.get_metrics_history();
        assert_eq!(history.len(), 1000);

        // Should have the most recent 1000 points
        assert_eq!(history.front().unwrap().cache_size, 100); // 1100 - 1000
        assert_eq!(history.back().unwrap().cache_size, 1099);
    }

    #[test]
    fn test_metrics_collector_request_timestamps_limit() {
        let collector = MetricsCollector::new();

        // Record more requests than the timestamp limit (1000)
        for _i in 0..1100 {
            collector.record_cache_hit("method", 50);
        }

        let timestamps = collector.request_timestamps.read().unwrap();
        assert_eq!(timestamps.len(), 1000);
    }

    #[test]
    fn test_metrics_collector_concurrent_access() {
        let collector = Arc::new(MetricsCollector::new());
        let mut handles = vec![];

        // Spawn multiple threads that record metrics concurrently
        for i in 0..10 {
            let collector_clone = Arc::clone(&collector);
            let handle = thread::spawn(move || {
                for j in 0..100 {
                    collector_clone.record_cache_hit(&format!("method_{i}"), j as u64);

                    // Record cache miss and forwarded request
                    collector_clone.record_cache_miss();
                    collector_clone.record_request(
                        &format!("method_{i}"),
                        &format!("provider_{i}"),
                        (j * 2) as u64,
                        true,
                    );
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify final counts
        assert_eq!(collector.cache_hits.load(Ordering::Relaxed), 1000);
        assert_eq!(collector.cache_misses.load(Ordering::Relaxed), 1000);
        assert_eq!(collector.total_requests.load(Ordering::Relaxed), 2000);

        // Verify method stats were updated correctly
        let stats = collector.get_method_stats();
        assert_eq!(stats.len(), 10); // 10 different methods

        for i in 0..10 {
            let method_name = format!("method_{i}");
            let method_stat = stats.get(&method_name).unwrap();
            assert_eq!(method_stat.hits, 100);
            assert_eq!(method_stat.misses, 100);
            assert_eq!(method_stat.total_requests, 200);
        }

        // Verify provider stats were updated correctly
        let usage = collector.get_provider_usage();
        assert_eq!(usage.len(), 10); // 10 different providers

        for i in 0..10 {
            let provider_name = format!("provider_{i}");
            let provider_usage = usage.get(&provider_name).unwrap();
            assert_eq!(provider_usage.request_count, 100);
            assert_eq!(provider_usage.success_count, 100);
        }
    }

    #[test]
    fn test_metrics_collector_overall_avg_response_time() {
        let collector = MetricsCollector::new();

        // No requests initially
        assert_eq!(collector.overall_avg_response_time(), 0.0);

        // Add requests with different response times
        collector.record_cache_hit("method1", 100); // 100ms
        collector.record_cache_hit("method2", 200); // 200ms

        collector.record_cache_miss();
        collector.record_request("method3", "provider1", 300, true); // 300ms

        // Should calculate weighted average across all methods
        // method1: 100ms total, 1 request
        // method2: 200ms total, 1 request
        // method3: 300ms total, 1 request
        // Overall: (100 + 200 + 300) / 3 = 200ms
        assert_eq!(collector.overall_avg_response_time(), 200.0);
    }

    #[test]
    fn test_metrics_serialization() {
        let method_stats = MethodStats {
            hits: 10,
            total_requests: 15,
            avg_response_time_ms: 150.5,
            ..Default::default()
        };

        // Test serialization to JSON
        let json = serde_json::to_string(&method_stats).unwrap();
        assert!(json.contains("\"hits\":10"));
        assert!(json.contains("\"total_requests\":15"));
        assert!(json.contains("\"avg_response_time_ms\":150.5"));

        // Test deserialization from JSON
        let deserialized: MethodStats = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.hits, method_stats.hits);
        assert_eq!(deserialized.total_requests, method_stats.total_requests);
        assert_eq!(deserialized.avg_response_time_ms, method_stats.avg_response_time_ms);
    }

    #[test]
    fn test_edge_cases() {
        let collector = MetricsCollector::new();

        // Test with empty method name
        collector.record_cache_hit("", 100);
        let stats = collector.get_method_stats();
        assert!(stats.contains_key(""));

        // Test with very long method name
        let long_method = "a".repeat(1000);
        collector.record_cache_hit(&long_method, 200);
        let stats = collector.get_method_stats();
        assert!(stats.contains_key(&long_method));

        // Test with zero response time
        collector.record_cache_hit("zero_time", 0);
        let stats = collector.get_method_stats();
        let method_stat = stats.get("zero_time").unwrap();
        assert_eq!(method_stat.avg_response_time_ms, 0.0);

        // Test with very large response time
        collector.record_cache_hit("large_time", u64::MAX);
        let stats = collector.get_method_stats();
        let method_stat = stats.get("large_time").unwrap();
        assert_eq!(method_stat.total_response_time_ms, u64::MAX);
    }
}
