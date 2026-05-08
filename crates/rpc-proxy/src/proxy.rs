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

//! Core proxy server implementation

use crate::{
    cache::CacheManager,
    health::HealthService,
    metrics::MetricsCollector,
    providers::{DEFAULT_MAINNET_RPCS, ProviderManager},
    registry::EdbRegistry,
    rpc::RpcHandler,
};
use axum::{
    Router,
    extract::State,
    http::{Method, StatusCode},
    response::Json,
    routing::post,
};
use eyre::Result;
use serde_json::Value;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::{net::TcpListener, sync::broadcast};
use tower_http::cors::{Any, CorsLayer};
use tracing::{debug, info, warn};

/// Builder for configuring ProxyServer with fluent API and sensible defaults
#[derive(Debug, Clone)]
pub struct ProxyServerBuilder {
    rpc_urls: Option<Vec<String>>,
    max_cache_items: u32,
    cache_dir: Option<PathBuf>,
    grace_period: u64,
    heartbeat_interval: u64,
    max_failures: u32,
    health_check_interval: u64,
    cache_save_interval: u64,
}

impl Default for ProxyServerBuilder {
    fn default() -> Self {
        Self {
            // General Configuration
            rpc_urls: None, // Will use DEFAULT_MAINNET_RPCS

            // Cache Configuration
            max_cache_items: 1024000,
            cache_dir: None,        // Will use ~/.edb/cache/rpc/<chain_id>
            cache_save_interval: 5, // 5 minutes

            // Provider Health Check Configuration
            max_failures: 3,
            health_check_interval: 60,

            // EDB Register Configuration
            grace_period: 0, // No auto-shutdown by default
            heartbeat_interval: 10,
        }
    }
}

impl ProxyServerBuilder {
    /// Create a new builder with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set custom RPC URLs (comma-separated string or Vec)
    #[allow(dead_code)]
    pub fn rpc_urls<T: Into<Vec<String>>>(mut self, urls: T) -> Self {
        self.rpc_urls = Some(urls.into());
        self
    }

    /// Set custom RPC URLs from comma-separated string
    pub fn rpc_urls_str(mut self, urls: &str) -> Self {
        self.rpc_urls =
            Some(urls.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
        self
    }

    /// Set maximum number of cached items
    pub fn max_cache_items(mut self, max_items: u32) -> Self {
        self.max_cache_items = max_items;
        self
    }

    /// Set cache directory path
    pub fn cache_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.cache_dir = Some(dir.into());
        self
    }

    /// Set grace period in seconds before shutdown when no EDB instances (0 = no auto-shutdown)
    pub fn grace_period(mut self, seconds: u64) -> Self {
        self.grace_period = seconds;
        self
    }

    /// Set heartbeat check interval in seconds
    pub fn heartbeat_interval(mut self, seconds: u64) -> Self {
        self.heartbeat_interval = seconds;
        self
    }

    /// Set maximum consecutive failures before marking provider unhealthy
    pub fn max_failures(mut self, failures: u32) -> Self {
        self.max_failures = failures;
        self
    }

    /// Set provider health check interval in seconds
    pub fn health_check_interval(mut self, seconds: u64) -> Self {
        self.health_check_interval = seconds;
        self
    }

    /// Set cache save interval in minutes (0 = save only on shutdown)
    pub fn cache_save_interval(mut self, minutes: u64) -> Self {
        self.cache_save_interval = minutes;
        self
    }

    /// Build the ProxyServer with the configured settings
    pub async fn build(self) -> Result<ProxyServer> {
        // Resolve RPC URLs
        let rpc_urls = self
            .rpc_urls
            .unwrap_or_else(|| DEFAULT_MAINNET_RPCS.iter().map(|s| s.to_string()).collect());

        // Resolve cache path
        let cache_path = CacheManager::get_cache_path(&rpc_urls, self.cache_dir).await?;

        // Now call the simplified ProxyServer::new with deterministic values
        ProxyServer::new(
            rpc_urls,
            self.max_cache_items,
            cache_path,
            self.grace_period,
            self.heartbeat_interval,
            self.max_failures,
            self.health_check_interval,
            self.cache_save_interval,
        )
        .await
    }
}

/// Main proxy server that combines RPC handling, registry, and health services
///
/// The ProxyServer coordinates between:
/// - RPC request handling and caching
/// - EDB instance registry and lifecycle management
/// - Health check and monitoring endpoints
///
/// Use ProxyServerBuilder for easy configuration:
/// ```no_run
/// # use edb_rpc_proxy::proxy::ProxyServerBuilder;
/// # async fn example() -> eyre::Result<()> {
/// let proxy = ProxyServerBuilder::new().max_cache_items(50000).grace_period(300).build().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct ProxyServer {
    /// RPC request handler with caching capabilities
    pub rpc_handler: Arc<RpcHandler>,
    /// Registry for tracking connected EDB instances
    pub registry: Arc<EdbRegistry>,
    /// Health check service for monitoring
    pub health_service: Arc<HealthService>,
    /// Metrics collector for performance tracking
    pub metrics_collector: Arc<MetricsCollector>,
    /// Shutdown signal sender
    shutdown_tx: broadcast::Sender<()>,
}

#[derive(Clone)]
struct AppState {
    proxy: ProxyServer,
}

impl ProxyServer {
    /// Creates a new proxy server with deterministic configuration
    ///
    /// This method is now simplified to take concrete values rather than Options.
    /// Use ProxyServerBuilder for a more convenient fluent API.
    ///
    /// # Arguments
    /// * `rpc_urls` - List of upstream RPC endpoint URLs
    /// * `max_cache_items` - Maximum number of items to cache
    /// * `cache_path` - Resolved path for cache persistence
    /// * `grace_period` - Seconds to wait before shutdown when no EDB instances
    /// * `heartbeat_interval` - Seconds between heartbeat checks
    /// * `max_failures` - Maximum consecutive failures before marking provider unhealthy
    /// * `health_check_interval` - Seconds between provider health checks
    /// * `cache_save_interval` - Minutes between periodic cache saves
    ///
    /// # Returns
    /// A new ProxyServer instance with background tasks started
    #[allow(clippy::too_many_arguments)]
    async fn new(
        rpc_urls: Vec<String>,
        max_cache_items: u32,
        cache_path: PathBuf,
        grace_period: u64,
        heartbeat_interval: u64,
        max_failures: u32,
        health_check_interval: u64,
        cache_save_interval: u64,
    ) -> Result<Self> {
        info!("Starting EDB RPC Proxy with {} providers", rpc_urls.len());
        for url in &rpc_urls {
            info!("  - {}", url);
        }

        let cache_manager = Arc::new(CacheManager::new(max_cache_items, cache_path)?);
        let metrics_collector = Arc::new(MetricsCollector::new());

        // Create provider manager with all URLs
        let provider_manager = Arc::new(ProviderManager::new(rpc_urls, max_failures).await?);

        // Create RPC handler with provider manager
        let rpc_handler = Arc::new(RpcHandler::new(
            provider_manager.clone(),
            cache_manager.clone(),
            metrics_collector.clone(),
        )?);
        let health_service = Arc::new(HealthService::new());
        let (shutdown_tx, _) = broadcast::channel(1);

        // Create registry with shutdown channel
        let registry =
            Arc::new(EdbRegistry::new(grace_period, heartbeat_interval, shutdown_tx.clone()));

        // Start background tasks (if grace period is active)
        if grace_period > 0 {
            let registry_clone = Arc::clone(&registry);
            tokio::spawn(async move {
                registry_clone.start_heartbeat_monitor().await;
            });
        }

        // Start periodic health checks for providers
        let provider_manager_clone = provider_manager.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(health_check_interval));
            loop {
                interval.tick().await;
                provider_manager_clone.health_check_all().await;
            }
        });

        // Start periodic cache saving (if enabled)
        if cache_save_interval > 0 {
            let cache_manager_clone = cache_manager.clone();
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(cache_save_interval * 60));
                loop {
                    interval.tick().await;
                    if let Err(e) = cache_manager_clone.save_to_disk().await {
                        warn!("Failed to save cache periodically: {}", e);
                    } else {
                        debug!("Cache saved to disk (periodic save)");
                    }
                }
            });
        }

        // Start background metrics collection task
        let metrics_collector_clone = metrics_collector.clone();
        let cache_manager_clone = cache_manager.clone();
        let provider_manager_clone = provider_manager;
        let registry_clone = registry.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;

                // Collect current metrics for historical tracking
                let cache_stats = cache_manager_clone.detailed_stats().await;
                let providers_info = provider_manager_clone.get_providers_info().await;
                let healthy_providers =
                    providers_info.iter().filter(|p| p.is_healthy).count() as u64;
                let total_providers = providers_info.len() as u64;
                let active_instances = registry_clone.get_active_instances().await.len();

                let total_entries =
                    cache_stats.get("total_entries").and_then(|v| v.as_u64()).unwrap_or(0);
                metrics_collector_clone.add_historical_point(
                    total_entries,
                    healthy_providers,
                    total_providers,
                    active_instances,
                );
            }
        });

        Ok(Self { rpc_handler, registry, health_service, metrics_collector, shutdown_tx })
    }

    /// Returns a reference to the cache manager
    ///
    /// # Returns
    /// Reference to the underlying cache manager
    pub fn cache_manager(&self) -> &Arc<CacheManager> {
        self.rpc_handler.cache_manager()
    }

    /// Starts the proxy server listening on the specified address
    ///
    /// Creates an Axum web server with routes for:
    /// - Standard JSON-RPC requests (POST /)
    /// - EDB-specific management endpoints (edb_ping, edb_register, etc.)
    ///
    /// # Arguments
    /// * `addr` - Socket address to bind to
    ///
    /// # Returns
    /// Result indicating server startup success or failure
    pub async fn serve(self, addr: SocketAddr) -> Result<()> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let cache_manager_for_shutdown = self.cache_manager().clone();

        let app = Router::new()
            .route("/", post(handle_rpc))
            .layer(
                CorsLayer::new()
                    .allow_methods([Method::POST, Method::GET])
                    .allow_headers(Any)
                    .allow_origin(Any),
            )
            .with_state(AppState { proxy: self });

        let listener = TcpListener::bind(addr).await?;
        info!("EDB RPC Proxy listening on {}", addr);

        // Create the server with graceful shutdown
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = shutdown_rx.recv().await;
            info!("Shutdown signal received, saving cache and stopping server gracefully");

            // Save cache before shutdown
            if let Err(e) = cache_manager_for_shutdown.save_to_disk().await {
                warn!("Failed to save cache during shutdown: {}", e);
            } else {
                info!("Cache saved successfully during shutdown");
            }
        });

        server.await?;

        Ok(())
    }
}

async fn handle_rpc(
    State(state): State<AppState>,
    Json(request): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    // Handle special EDB health check methods
    debug!("Received RPC request: {}", request);
    let response = if let Some(method) = request.get("method").and_then(|m| m.as_str()) {
        match method {
            "edb_ping" => {
                let response = state.proxy.health_service.ping().await;
                Ok(Json(response))
            }
            "edb_info" => {
                let response = state.proxy.health_service.info().await;
                Ok(Json(response))
            }
            "edb_register" => {
                if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                    if let (Some(pid), Some(timestamp)) = (
                        params.first().and_then(|v| v.as_u64()),
                        params.get(1).and_then(|v| v.as_u64()),
                    ) {
                        let response =
                            state.proxy.registry.register_edb_instance(pid as u32, timestamp).await;
                        Ok(Json(response))
                    } else {
                        Err(StatusCode::BAD_REQUEST)
                    }
                } else {
                    Err(StatusCode::BAD_REQUEST)
                }
            }
            "edb_heartbeat" => {
                if let Some(params) = request.get("params").and_then(|p| p.as_array()) {
                    if let Some(pid) = params.first().and_then(|v| v.as_u64()) {
                        let response = state.proxy.registry.heartbeat(pid as u32).await;
                        Ok(Json(response))
                    } else {
                        Err(StatusCode::BAD_REQUEST)
                    }
                } else {
                    Err(StatusCode::BAD_REQUEST)
                }
            }
            "edb_cache_stats" => {
                let stats = state.proxy.cache_manager().detailed_stats().await;
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").unwrap_or(&serde_json::Value::from(1)),
                    "result": stats
                });
                Ok(Json(response))
            }
            "edb_active_instances" => {
                let active_pids = state.proxy.registry.get_active_instances().await;
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").unwrap_or(&serde_json::Value::from(1)),
                    "result": {
                        "active_instances": active_pids,
                        "count": active_pids.len()
                    }
                });
                Ok(Json(response))
            }
            "edb_providers" => {
                let providers_info =
                    state.proxy.rpc_handler.provider_manager().get_providers_info().await;
                let healthy_count =
                    state.proxy.rpc_handler.provider_manager().healthy_provider_count().await;
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").unwrap_or(&serde_json::Value::from(1)),
                    "result": {
                        "providers": providers_info,
                        "healthy_count": healthy_count,
                        "total_count": providers_info.len()
                    }
                });
                Ok(Json(response))
            }
            "edb_cache_metrics" => {
                let metrics = state.proxy.metrics_collector;
                let method_stats = metrics.get_method_stats();
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").unwrap_or(&serde_json::Value::from(1)),
                    "result": {
                        "total_requests": metrics.total_requests.load(std::sync::atomic::Ordering::Relaxed),
                        "cache_hits": metrics.cache_hits.load(std::sync::atomic::Ordering::Relaxed),
                        "cache_misses": metrics.cache_misses.load(std::sync::atomic::Ordering::Relaxed),
                        "hit_rate": format!("{:.1}%", metrics.cache_hit_rate()),
                        "error_rate": format!("{:.1}%", metrics.error_rate()),
                        "method_stats": method_stats,
                        "total_errors": metrics.total_errors.load(std::sync::atomic::Ordering::Relaxed),
                        "rate_limit_errors": metrics.rate_limit_errors.load(std::sync::atomic::Ordering::Relaxed),
                        "user_errors": metrics.user_errors.load(std::sync::atomic::Ordering::Relaxed)
                    }
                });
                Ok(Json(response))
            }
            "edb_provider_metrics" => {
                let metrics = state.proxy.metrics_collector;
                let provider_usage = metrics.get_provider_usage();
                let total_requests_to_providers =
                    metrics.cache_misses.load(std::sync::atomic::Ordering::Relaxed);

                let providers: Vec<serde_json::Value> = provider_usage.iter().map(|(url, usage)| {
                    serde_json::json!({
                        "url": url,
                        "request_count": usage.request_count,
                        "success_rate": format!("{:.1}%", usage.success_rate()),
                        "avg_response_time_ms": usage.avg_response_time_ms(),
                        "load_percentage": format!("{:.1}%", usage.load_percentage(total_requests_to_providers)),
                        "last_used_timestamp": usage.last_used_timestamp,
                        "error_count": usage.error_count
                    })
                }).collect();

                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").unwrap_or(&serde_json::Value::from(1)),
                    "result": {
                        "providers": providers,
                    }
                });
                Ok(Json(response))
            }
            "edb_metrics_history" => {
                let metrics = state.proxy.metrics_collector;
                let history = metrics.get_metrics_history();

                let cache_history: Vec<serde_json::Value> = history.iter().map(|h| {
                    serde_json::json!({
                        "timestamp": h.timestamp,
                        "cache_size": h.cache_size,
                        "hit_rate": if h.cache_hits + h.cache_misses > 0 {
                            (h.cache_hits as f64 / (h.cache_hits + h.cache_misses) as f64) * 100.0
                        } else { 0.0 },
                        "requests_per_minute": h.requests_per_minute
                    })
                }).collect();

                let provider_history: Vec<serde_json::Value> = history
                    .iter()
                    .map(|h| {
                        serde_json::json!({
                            "timestamp": h.timestamp,
                            "healthy_providers": h.healthy_providers,
                            "total_providers": h.total_providers,
                            "avg_response_time_ms": h.avg_response_time_ms
                        })
                    })
                    .collect();

                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").unwrap_or(&serde_json::Value::from(1)),
                    "result": {
                        "cache_history": cache_history,
                        "provider_history": provider_history
                    }
                });
                Ok(Json(response))
            }
            "edb_request_metrics" => {
                let metrics = state.proxy.metrics_collector;
                let recent_methods: Vec<String> = metrics
                    .get_method_stats()
                    .iter()
                    .filter(|(_, stats)| stats.total_requests > 0)
                    .take(10)
                    .map(|(method, _)| method.clone())
                    .collect();

                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").unwrap_or(&serde_json::Value::from(1)),
                    "result": {
                        "requests_per_minute": metrics.requests_per_minute(),
                        "active_requests": 0, // TODO: implement active request tracking
                        "recent_methods": recent_methods,
                        "error_rate": format!("{:.1}%", metrics.error_rate()),
                        "peak_requests_per_minute": 0 // TODO: implement peak tracking
                    }
                });
                Ok(Json(response))
            }
            "edb_deleteCache" => {
                let params = request.get("params").unwrap_or(&serde_json::Value::Null);

                let result = if params.is_array() && !params.as_array().unwrap().is_empty() {
                    let arr = params.as_array().unwrap();
                    let method = arr[0].as_str().unwrap_or("");

                    if arr.len() == 1 {
                        // Just method: delete all entries for this method
                        match state.proxy.rpc_handler.cache_manager().delete_by_method(method).await
                        {
                            Ok(deleted_count) => {
                                serde_json::json!({
                                    "success": true,
                                    "deleted_count": deleted_count,
                                    "message": format!("Deleted {} entries for method '{}'", deleted_count, method)
                                })
                            }
                            Err(e) => {
                                warn!("Failed to delete cache entries: {}", e);
                                serde_json::json!({
                                    "success": false,
                                    "error": format!("Failed to delete cache entries: {}", e)
                                })
                            }
                        }
                    } else {
                        // Method + params: delete specific request
                        let req = serde_json::json!({
                            "method": method,
                            "params": arr[1].clone()
                        });
                        let key = state.proxy.rpc_handler.generate_cache_key(&req);

                        match state.proxy.rpc_handler.cache_manager().delete_by_key(&key).await {
                            Ok(true) => {
                                serde_json::json!({
                                    "success": true,
                                    "deleted_count": 1,
                                    "message": "Deleted specific cache entry"
                                })
                            }
                            Ok(false) => {
                                serde_json::json!({
                                    "success": true,
                                    "deleted_count": 0,
                                    "message": "Cache entry not found"
                                })
                            }
                            Err(e) => {
                                warn!("Failed to delete cache entry: {}", e);
                                serde_json::json!({
                                    "success": false,
                                    "error": format!("Failed to delete cache entry: {}", e)
                                })
                            }
                        }
                    }
                } else {
                    serde_json::json!({
                        "success": false,
                        "error": "Invalid parameter format. Expected [method] or [method, params]"
                    })
                };

                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").unwrap_or(&serde_json::Value::from(1)),
                    "result": result
                });

                Ok(Json(response))
            }
            "edb_shutdown" => {
                info!("Shutdown request received");
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.get("id").unwrap_or(&serde_json::Value::from(1)),
                    "result": {
                        "status": "shutting_down",
                        "message": "Server shutdown initiated"
                    }
                });

                // Send shutdown signal (non-blocking)
                let _ = state.proxy.shutdown_tx.send(());

                Ok(Json(response))
            }
            _ => {
                // Forward to RPC handler
                match state.proxy.rpc_handler.handle_request(request).await {
                    Ok(response) => Ok(Json(response)),
                    Err(e) => {
                        warn!("RPC request failed: {}", e);
                        Err(StatusCode::INTERNAL_SERVER_ERROR)
                    }
                }
            }
        }
    } else {
        warn!("Invalid RPC request: {}", request);
        Err(StatusCode::BAD_REQUEST)
    };

    debug!("RPC response: {}", &format!("{response:?}").chars().take(200).collect::<String>());
    response
}
