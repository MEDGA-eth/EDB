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

//! JSON-RPC server implementation for EDB debugging API.
//!
//! This module implements the main HTTP server that handles JSON-RPC debugging requests
//! using the Axum web framework. The server provides a thread-safe, multi-client debugging
//! interface with graceful shutdown capabilities.
//!
//! # Features
//!
//! - **Multi-threaded**: Handles concurrent debugging requests from multiple clients
//! - **Thread-safe**: Uses Arc-wrapped EngineContext for safe shared access
//! - **Graceful shutdown**: Supports clean server termination
//! - **Health monitoring**: Provides health check endpoint for monitoring
//! - **Error handling**: Comprehensive error reporting with JSON-RPC 2.0 compliance
//!
//! # Server Architecture
//!
//! The server follows a standard Axum pattern:
//! 1. Creates a router with RPC and health check endpoints
//! 2. Spawns the server in a background task
//! 3. Returns a handle for monitoring and shutdown
//! 4. Routes requests to the appropriate method handlers
//!
//! # Endpoints
//!
//! - `POST /` - Main JSON-RPC endpoint for debugging methods
//! - `GET /health` - Health check endpoint returning server status

use super::{
    methods::MethodHandler,
    types::{RpcError, RpcRequest, RpcResponse},
    utils::get_default_rpc_port,
};
use crate::EngineContext;
use axum::{
    Router,
    extract::{Json as JsonExtract, State},
    response::Json as JsonResponse,
    routing::{get, post},
};
use eyre::Result;
use revm::{Database, DatabaseCommit, DatabaseRef, database::CacheDB};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::oneshot;
use tracing::{error, info, warn};

/// Handle to control a running RPC server.
///
/// This handle provides access to server information and allows for graceful shutdown.
/// The server runs in a background task and can be monitored and controlled through this handle.
#[derive(Debug)]
pub struct RpcServerHandle {
    /// Address the server is listening on
    pub addr: SocketAddr,
    /// Shutdown signal sender (consumed when shutting down)
    shutdown_tx: oneshot::Sender<()>,
}

impl RpcServerHandle {
    /// Get the server address
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Get the port number
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Gracefully shutdown the RPC server
    pub fn shutdown(self) -> Result<()> {
        if self.shutdown_tx.send(()).is_err() {
            warn!("RPC server already shut down");
        }
        Ok(())
    }
}

/// Thread-safe RPC state for Axum request handling.
///
/// This wrapper provides the shared state needed by Axum handlers.
/// It contains the server instance wrapped in Arc for thread-safe sharing.
#[derive(Clone)]
struct RpcState<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    /// The debug RPC server instance (shared across request handlers)
    server: Arc<DebugRpcServer<DB>>,
}

/// Main debug RPC server providing JSON-RPC debugging API.
///
/// This server provides read-only access to the debugging engine context through
/// a JSON-RPC interface. It handles method dispatch, error handling, and response
/// formatting according to the JSON-RPC 2.0 specification.
///
/// The server is thread-safe and can handle concurrent requests from multiple clients.
/// All access to the engine context is read-only to ensure debugging session integrity.
pub struct DebugRpcServer<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    /// Immutable debugging context providing read-only access to debugging data
    context: Arc<EngineContext<DB>>,
    /// Method handler for dispatching RPC requests to appropriate implementations
    method_handler: Arc<MethodHandler<DB>>,
}

impl<DB> DebugRpcServer<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    /// Create a new debug RPC server
    pub fn new(context: EngineContext<DB>) -> Self {
        let context = Arc::new(context);
        let method_handler = Arc::new(MethodHandler::new(context.clone()));

        Self { context, method_handler }
    }

    /// Start the RPC server
    pub async fn start(self) -> Result<RpcServerHandle> {
        let port = get_default_rpc_port()?;
        self.start_on_port(port).await
    }

    /// Start the RPC server on a specific port using standard multi-threaded pattern
    ///
    /// This method creates the Axum server with Send+Sync state, leveraging
    /// the now thread-safe EngineContext.
    pub async fn start_on_port(self, port: u16) -> Result<RpcServerHandle> {
        // Create the Axum app with the server as state
        let app = Router::new()
            .route("/", post(handle_rpc_request))
            .route("/health", get(health_check))
            .with_state(RpcState { server: Arc::new(self) });

        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let actual_addr = listener.local_addr()?;

        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // Spawn the Axum server
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    shutdown_rx.await.ok();
                })
                .await
                .expect("RPC server failed");
        });

        info!("Debug RPC server started on {}", actual_addr);

        Ok(RpcServerHandle { addr: actual_addr, shutdown_tx })
    }

    /// Handle an RPC request (called from worker thread)
    async fn handle_request(&self, request: RpcRequest) -> RpcResponse {
        let id = request.id.clone();

        // Dispatch to method handler
        match self.method_handler.handle_method(&request.method, request.params).await {
            Ok(result) => {
                RpcResponse { jsonrpc: "2.0".to_string(), result: Some(result), error: None, id }
            }
            Err(err) => {
                error!(target: "rpc", "Error handling RPC request: {:?}", err);
                RpcResponse { jsonrpc: "2.0".to_string(), result: None, error: Some(err), id }
            }
        }
    }

    /// Get total snapshot count (stateless)
    pub fn snapshot_count(&self) -> usize {
        self.context.snapshots.len()
    }

    /// Validate snapshot index (stateless helper)
    pub fn validate_snapshot_index(&self, index: usize) -> Result<()> {
        if index >= self.snapshot_count() {
            return Err(eyre::eyre!(
                "Snapshot index {} out of bounds (max: {})",
                index,
                self.snapshot_count() - 1
            ));
        }
        Ok(())
    }

    /// Get read-only access to the engine context
    pub fn context(&self) -> &Arc<EngineContext<DB>> {
        &self.context
    }
}

/// Handle RPC requests directly with the thread-safe server
async fn handle_rpc_request<DB>(
    State(state): State<RpcState<DB>>,
    JsonExtract(request): JsonExtract<RpcRequest>,
) -> JsonResponse<RpcResponse>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    // Validate JSON-RPC version
    if request.jsonrpc != "2.0" {
        return JsonResponse(RpcResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(RpcError {
                code: -32600,
                message: "Invalid Request - JSON-RPC version must be 2.0".to_string(),
                data: None,
            }),
            id: request.id.clone(),
        });
    }

    // Handle request directly (no channel proxy needed)
    let response = state.server.handle_request(request).await;
    JsonResponse(response)
}

/// Health check endpoint
async fn health_check() -> JsonResponse<serde_json::Value> {
    JsonResponse(serde_json::json!({
        "status": "healthy",
        "service": "edb-debug-rpc-server",
        "version": env!("CARGO_PKG_VERSION"),
        "architecture": "multi-threaded"
    }))
}

/// Create and start a debug RPC server with auto-port detection
pub async fn start_debug_server<DB>(context: EngineContext<DB>) -> Result<RpcServerHandle>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone + Send + Sync + 'static,
    <CacheDB<DB> as Database>::Error: Clone + Send + Sync,
    <DB as Database>::Error: Clone + Send + Sync,
{
    let server = DebugRpcServer::new(context);
    server.start().await
}
