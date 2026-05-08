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

//! Test utilities for integration tests

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// Path utilities for test resources
pub mod paths {
    use std::env;

    use super::*;

    /// Get the baseline directory path
    pub fn get_baseline_dir() -> PathBuf {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .parent() // crates/
            .and_then(|p| p.parent()) // workspace root
            .expect("Failed to find workspace root")
            .join("testdata")
            .join("rpc_baseline")
    }
}

/// Initialization utilities for tests
pub mod init {
    /// Initialize test environment with cache directory and logging
    /// This is a convenience function that sets up both cache and logging
    pub fn init_test_environment(use_temp_dir: bool) {
        edb_common::test_utils::setup_test_environment(use_temp_dir);
        edb_common::logging::ensure_test_logging(None);
    }
}

/// Proxy utilities for testing with RPC proxy
pub mod proxy {
    use edb_rpc_proxy::proxy::ProxyServerBuilder;
    use std::{env, path::PathBuf, time::Duration};
    use tokio::time::sleep;
    use tracing::info;

    /// Helper to set up a cache-only test proxy with caching in testdata directory
    async fn setup_test_proxy_with_cache_only(
        grace_period: u64,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // Create a temporary cache directory for test isolation
        let cache_dir = edb_common::test_utils::create_temp_cache_dir();
        setup_test_proxy(grace_period, Some(cache_dir), Some(vec![])).await
    }

    /// Helper to set up a test proxy with caching in testdata directory
    async fn setup_test_proxy_with_cache(
        grace_period: u64,
    ) -> Result<String, Box<dyn std::error::Error>> {
        // Use the shared testdata cache directory
        let cache_dir = edb_common::test_utils::get_testdata_cache_root();
        setup_test_proxy(grace_period, Some(cache_dir), None).await
    }

    /// Configurable proxy setup based on environment variable
    /// Set EDB_TEST_PROXY_MODE to "cache-only" to use cache-only mode, otherwise uses normal cache
    /// mode
    pub async fn setup_test_proxy_configurable(
        grace_period: u64,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let proxy_mode = env::var(edb_common::env::EDB_TEST_PROXY_MODE).unwrap_or_default();

        match proxy_mode.as_str() {
            "cache-only" => {
                info!("Using cache-only proxy mode");
                setup_test_proxy_with_cache_only(grace_period).await
            }
            _ => {
                info!("Using normal cache proxy mode");
                setup_test_proxy_with_cache(grace_period).await
            }
        }
    }

    /// Helper to set up a test proxy with caching in testdata directory
    async fn setup_test_proxy(
        grace_period: u64,
        cache_dir: Option<PathBuf>,
        rpc_urls: Option<Vec<String>>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut proxy_builder = ProxyServerBuilder::new()
            .max_cache_items(500000)
            .grace_period(grace_period)
            .heartbeat_interval(30);

        if let Some(dir) = cache_dir {
            info!("Starting test proxy with cache at {dir:?}");
            proxy_builder = proxy_builder.cache_dir(dir);
        }

        if let Some(urls) = rpc_urls {
            info!("Starting test proxy with RPC URLs: {urls:?}");
            proxy_builder = proxy_builder.rpc_urls(urls);
        }

        let proxy = proxy_builder.build().await?;

        // Find an available port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        drop(listener);

        // Start proxy in background
        tokio::spawn(async move {
            if let Err(e) = proxy.serve(addr).await {
                eprintln!("Proxy error: {e}");
            }
        });

        // Wait for proxy to start
        sleep(Duration::from_millis(200)).await;

        let proxy_url = format!("http://{addr}");
        println!("Test proxy started at {proxy_url}");

        Ok(proxy_url)
    }

    /// Gracefully shutdown a test proxy using the edb_shutdown endpoint
    pub async fn shutdown_test_proxy(proxy_url: &str) -> Result<(), Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        let shutdown_request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "edb_shutdown",
            "id": 1
        });

        match client.post(proxy_url).json(&shutdown_request).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    println!("Test proxy shutdown successfully");
                    // Give the server a moment to shutdown gracefully
                    sleep(Duration::from_millis(100)).await;
                } else {
                    println!("Proxy shutdown request failed with status: {}", response.status());
                }
            }
            Err(e) => {
                // Connection refused is expected after shutdown
                if e.is_connect() {
                    println!("Test proxy shutdown confirmed (connection refused)");
                } else {
                    println!("Error during proxy shutdown: {e}");
                }
            }
        }
        Ok(())
    }

    /// Register test instance with proxy for monitoring
    pub async fn register_with_proxy(proxy_url: &str) -> Result<(), Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        let register_request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "edb_register",
            "params": [std::process::id(), chrono::Utc::now().timestamp()],
            "id": 1
        });

        client.post(proxy_url).json(&register_request).send().await?;
        println!("Registered test instance with proxy");
        Ok(())
    }

    /// Get cache statistics from proxy
    pub async fn get_cache_stats(
        proxy_url: &str,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let client = reqwest::Client::new();
        let stats_request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "edb_cache_stats",
            "id": 1
        });

        let response = client.post(proxy_url).json(&stats_request).send().await?;
        let body: serde_json::Value = response.json().await?;
        Ok(body["result"].clone())
    }
}

/// Logging and error capture utilities for tests
pub mod logging {
    use super::*;
    use tracing::Level;
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    /// A custom tracing layer that captures error logs
    #[derive(Clone, Default)]
    pub struct ErrorCapture {
        errors: Arc<Mutex<Vec<String>>>,
    }

    impl ErrorCapture {
        /// Create a new ErrorCapture instance
        pub fn new() -> Self {
            Self { errors: Arc::new(Mutex::new(Vec::new())) }
        }

        /// Retrieve captured error messages
        pub fn get_errors(&self) -> Vec<String> {
            self.errors.lock().unwrap().clone()
        }

        /// Check if any errors were captured
        pub fn has_errors(&self) -> bool {
            !self.errors.lock().unwrap().is_empty()
        }

        /// Clear captured errors
        pub fn clear_errors(&self) {
            self.errors.lock().unwrap().clear();
        }
    }

    impl<S> tracing_subscriber::Layer<S> for ErrorCapture
    where
        S: tracing::Subscriber,
    {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            // Check if this is an error-level event
            if event.metadata().level() == &Level::ERROR {
                // Create a visitor to extract the message
                struct MessageVisitor {
                    message: String,
                }

                impl tracing::field::Visit for MessageVisitor {
                    fn record_debug(
                        &mut self,
                        field: &tracing::field::Field,
                        value: &dyn std::fmt::Debug,
                    ) {
                        if field.name() == "message" {
                            self.message = format!("{value:?}");
                        }
                    }

                    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                        if field.name() == "message" {
                            self.message = value.to_string();
                        }
                    }
                }

                let mut visitor = MessageVisitor { message: String::new() };
                event.record(&mut visitor);

                if !visitor.message.is_empty() {
                    self.errors.lock().unwrap().push(visitor.message);
                }
            }
        }
    }

    /// Setup test logging with error capture
    pub fn setup_test_logging_with_error_capture() -> ErrorCapture {
        let error_capture = ErrorCapture::new();

        // Initialize tracing with our custom error capture layer
        let _ = tracing_subscriber::registry()
            .with(error_capture.clone())
            .with(tracing_subscriber::fmt::layer().with_test_writer())
            .with(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("edb=debug".parse().unwrap())
                    .add_directive("edb_engine=debug".parse().unwrap())
                    .add_directive("edb_common=debug".parse().unwrap()),
            )
            .try_init();

        error_capture
    }
}

/// Engine testing utilities
pub mod engine {
    use std::net::SocketAddr;

    use alloy_primitives::TxHash;
    use edb_common::fork_and_prepare;
    use edb_engine::{Engine, EngineConfig};
    use tracing::info;

    use super::logging;

    /// Test result containing any errors captured during replay
    pub struct ReplayTestResult {
        /// Analysis Engine
        pub engine: Engine,
        /// RPC Server Address
        pub rpc_handle_addr: SocketAddr,
        /// Captured error messages
        pub errors: Vec<String>,
        /// Whether the replay was successful (no errors)
        pub success: bool,
    }

    /// Mimics the replay_transaction function from the CLI for testing
    pub async fn replay_transaction_test(
        tx_hash: TxHash,
        rpc_url: &str,
        quick_mode: bool,
        etherscan_api_key: Option<String>,
    ) -> Result<ReplayTestResult, Box<dyn std::error::Error>> {
        // Setup error capture
        let error_capture = logging::setup_test_logging_with_error_capture();

        info!("Starting transaction replay workflow for test");

        // Step 1: Fork the chain and replay earlier transactions in the block
        info!("Forking and preparing database for transaction: {}", tx_hash);
        let fork_result = fork_and_prepare(rpc_url, tx_hash, quick_mode).await?;

        info!(
            "Forked chain and prepared database for transaction replay at block {}",
            fork_result.fork_info.block_number
        );

        // Step 2: Build inputs for the engine
        let mut engine_config =
            EngineConfig::default().with_quick_mode(quick_mode).with_rpc_proxy_url(rpc_url.into());

        if let Some(api_key) = etherscan_api_key {
            engine_config = engine_config.with_etherscan_api_key(api_key);
        }

        // Step 3: Call engine::prepare with forked database and EVM config
        info!("Calling engine::prepare with prepared inputs");

        // Create the engine and run preparation
        let engine = Engine::new(engine_config);
        let rpc_handle_addr = engine.prepare(fork_result, None).await?;

        info!("Engine preparation completed successfully");

        // Return test results
        let errors = error_capture.get_errors();
        Ok(ReplayTestResult { engine, rpc_handle_addr, success: errors.is_empty(), errors })
    }

    /// Run a replay test and assert no errors occurred
    pub async fn assert_replay_success(
        tx_hash: TxHash,
        rpc_url: &str,
        quick_mode: bool,
        etherscan_api_key: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let result =
            replay_transaction_test(tx_hash, rpc_url, quick_mode, etherscan_api_key).await?;

        if !result.success {
            panic!("Errors found during replay: {:?}", result.errors);
        }

        Ok(())
    }
}
