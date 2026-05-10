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

//! WebSocket server command - manages remote debugging sessions

use alloy_primitives::TxHash;
use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
    routing::get,
};
use edb_common::fork_and_prepare;
use edb_engine::Engine;
use eyre::Result;
use futures::{SinkExt, StreamExt};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::{
    select,
    sync::{Mutex, mpsc, oneshot},
};
use tracing::{error, info, warn};

use crate::ws_protocol::{ClientRequest, ProgressMessage, ServerResponse};

/// Server state shared across WebSocket connections
#[derive(Clone)]
struct ServerState {
    /// Shared thread-safe Engine instance
    /// Engine now handles its own internal locking per transaction
    engine: Arc<Engine>,
    /// Track number of active connections per tx_hash
    active_connections: Arc<Mutex<HashMap<TxHash, usize>>>,
    /// Channel to send work to the worker thread
    worker_tx: mpsc::UnboundedSender<WorkerMessage>,
}

/// Start the WebSocket server
pub async fn start_server(ws_port: u16, cli: &crate::Cli) -> Result<()> {
    info!("Starting EDB WebSocket server on port {}", ws_port);

    // The server mode has no pre-wired RPC URL; connections supply their own
    // transaction hashes and the worker forks on demand. Use an empty string as
    // a placeholder — the worker receives the real URL from each request.
    let rpc_url = String::new();

    // Create the engine with configuration
    let mut engine_config = edb_engine::EngineConfig::default()
        .with_quick_mode(cli.quick)
        .with_rpc_proxy_url(rpc_url.clone());
    if let Some(api_key) = &cli.etherscan_api_key {
        engine_config = engine_config.with_etherscan_api_key(api_key.clone());
    }
    let engine = Engine::new(engine_config);
    let engine = Arc::new(engine);

    // Spawn the worker thread for handling requests
    let worker_tx = spawn_worker(Arc::clone(&engine), rpc_url, cli.quick);

    // Create shared state
    let state = ServerState {
        engine: Arc::clone(&engine),
        active_connections: Arc::new(Mutex::new(HashMap::new())),
        worker_tx,
    };

    // Create the Axum router
    let app = Router::new().route("/", get(ws_handler)).with_state(state);

    // Bind and serve
    let addr = SocketAddr::from(([127, 0, 0, 1], ws_port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_addr = listener.local_addr()?;

    info!("WebSocket server listening on {}", actual_addr);

    println!("Server listening on {actual_addr}");
    // Run the server normally - the worker thread handles !Send operations
    axum::serve(listener, app).await.expect("Server failed");

    Ok(())
}

/// WebSocket upgrade handler
async fn ws_handler(ws: WebSocketUpgrade, State(state): State<ServerState>) -> Response {
    ws.on_upgrade(|socket| async move {
        let _ = handle_socket(socket, state).await;
    })
}

/// Handle a WebSocket connection
async fn handle_socket(socket: WebSocket, state: ServerState) {
    let (mut sender, mut receiver) = socket.split();

    // Wait for client request
    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
        };

        // Only handle text messages
        let text = match msg {
            Message::Text(text) => text,
            Message::Close(_) => {
                info!("Client closed connection");
                break;
            }
            _ => continue,
        };

        // Parse client request
        let request: ClientRequest = match serde_json::from_str(&text) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to parse client request: {}", e);
                let response = ServerResponse::error(format!("Invalid request: {e}"));
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = sender.send(Message::Text(json.into())).await;
                }
                continue;
            }
        };

        // Create a progress channel to send progress messages to the client
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<ProgressMessage>();

        // Handle the request
        // Replay requests are delegated to the worker thread (handles !Send types)
        // Other requests can be handled directly
        let request_task = async {
            match request {
                ClientRequest::Replay { tx_hash } => {
                    handle_replay_request(tx_hash, &state, progress_tx).await
                }
                ClientRequest::Test { test_name, block } => {
                    handle_test_request(test_name, block, &state, progress_tx).await
                }
            }
        };
        tokio::pin!(request_task);

        // Wait for the request to complete while keep sending progress messages to the client
        let response = loop {
            select! {
                Some(msg) = progress_rx.recv() => {
                    let response = ServerResponse::Progress {
                        message: msg.message,
                        current_step: msg.current_step,
                        total_steps: msg.total_steps,
                    };
                    let response_json = serde_json::to_string(&response).expect("Failed to serialize progress message");
                    if let Err(e) = sender.send(Message::Text(response_json.into())).await {
                        error!("Failed to send progress message: {}", e);
                    }
                }
                response = &mut request_task => {
                    break response;
                }
            }
        };

        // Send response
        if let Ok(json) = serde_json::to_string(&response)
            && let Err(e) = sender.send(Message::Text(json.into())).await
        {
            error!("Failed to send response: {}", e);
            break;
        }

        // If successful, track this connection and wait for close
        if let ServerResponse::Success { tx_hash, .. } = &response
            && let Ok(tx_hash) = tx_hash.parse::<TxHash>()
        {
            // Keep connection alive and track it
            track_connection(tx_hash, &state, &mut receiver).await;
        }
    }
}

/// Handle replay request
async fn handle_replay_request(
    tx_hash_str: String,
    state: &ServerState,
    progress_tx: mpsc::UnboundedSender<ProgressMessage>,
) -> ServerResponse {
    // Parse transaction hash
    let tx_hash: TxHash = match tx_hash_str.parse() {
        Ok(hash) => hash,
        Err(e) => {
            return ServerResponse::error(format!("Invalid transaction hash: {e}"));
        }
    };

    info!("Received replay request for tx: {:?}", tx_hash);

    // Check if we already have a server for this tx
    // Engine handles this check internally now, but we still track connections
    if let Some(addr) = state.engine.get_rpc_server_addr(&tx_hash) {
        info!("Reusing existing RPC server on port {} for tx: {:?}", addr.port(), tx_hash);

        // Increment connection count
        let mut connections = state.active_connections.lock().await;
        *connections.entry(tx_hash).or_insert(0) += 1;

        return ServerResponse::success(addr.port(), tx_hash_str, true);
    }

    // Delegate to the worker thread for handling !Send operations
    info!("Delegating to worker thread for tx: {:?}", tx_hash);
    let (response_tx, response_rx) = oneshot::channel();

    if let Err(e) =
        state.worker_tx.send(WorkerMessage::Replay { tx_hash, progress_tx, response_tx })
    {
        error!("Failed to send message to worker: {}", e);
        return ServerResponse::error("Internal error: worker unavailable".to_string());
    }

    // Wait for the worker to complete the preparation
    let port = match response_rx.await {
        Ok(Ok(port)) => port,
        Ok(Err(e)) => {
            error!("Worker failed to process request: {}", e);
            return ServerResponse::error(format!("Preparation failed: {e}"));
        }
        Err(e) => {
            error!("Worker channel closed: {}", e);
            return ServerResponse::error(format!("Internal error: {e}"));
        }
    };

    info!("Worker completed preparation, RPC server on port {} for tx: {:?}", port, tx_hash);

    // Initialize connection count to 1
    {
        let mut connections = state.active_connections.lock().await;
        connections.insert(tx_hash, 1);
    }

    ServerResponse::success(port, tx_hash_str, false)
}

/// Handle test request (not yet implemented)
async fn handle_test_request(
    _test_name: String,
    _block: Option<u64>,
    _state: &ServerState,
    _progress_tx: mpsc::UnboundedSender<ProgressMessage>,
) -> ServerResponse {
    ServerResponse::error("Test debugging not yet implemented")
}

/// Track connection and handle disconnection
async fn track_connection(
    tx_hash: TxHash,
    state: &ServerState,
    receiver: &mut futures::stream::SplitStream<WebSocket>,
) {
    // Wait for the connection to close
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Close(_)) | Err(_) => break,
            _ => continue,
        }
    }

    info!("Connection closed for tx: {:?}", tx_hash);

    // Decrement connection count
    let should_shutdown = {
        let mut connections = state.active_connections.lock().await;
        if let Some(count) = connections.get_mut(&tx_hash) {
            *count -= 1;
            if *count == 0 {
                connections.remove(&tx_hash);
                true
            } else {
                info!("Still {} active connections for tx: {:?}", count, tx_hash);
                false
            }
        } else {
            false
        }
    };

    // If no more connections, shutdown the RPC server
    if should_shutdown {
        info!("No more connections for tx: {:?}, shutting down RPC server", tx_hash);

        // Shutdown the RPC server using Engine's shutdown method
        if let Err(e) = state.engine.shutdown_rpc_server(&tx_hash) {
            warn!("Error shutting down RPC server: {}", e);
        } else {
            info!("RPC server shutdown successfully for tx: {:?}", tx_hash);
        }
    }
}

/// Represents messages sent to the worker thread for handling operations involving types that are
/// not `Send`.
pub enum WorkerMessage {
    Replay {
        tx_hash: TxHash,
        progress_tx: mpsc::UnboundedSender<ProgressMessage>,
        response_tx: oneshot::Sender<Result<u16>>,
    },
}

/// Spawn the worker thread that handles requests
///
/// Returns a channel sender that can be used to send work to the worker
pub fn spawn_worker(
    engine: Arc<Engine>,
    rpc_url: String,
    quick: bool,
) -> mpsc::UnboundedSender<WorkerMessage> {
    let (worker_tx, worker_rx) = mpsc::unbounded_channel();

    std::thread::spawn(move || {
        // Create a multi-threaded runtime for this worker thread
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create worker runtime");

        rt.block_on(async move {
            worker_task(worker_rx, engine, rpc_url, quick).await;
        });
    });

    worker_tx
}

/// Worker task that processes messages
async fn worker_task(
    mut worker_rx: mpsc::UnboundedReceiver<WorkerMessage>,
    engine: Arc<Engine>,
    rpc_url: String,
    quick: bool,
) {
    info!("Worker task started");

    while let Some(msg) = worker_rx.recv().await {
        match msg {
            WorkerMessage::Replay { tx_hash, progress_tx, response_tx } => {
                info!("Worker processing replay for tx: {:?}", tx_hash);

                // Fork and prepare - this contains !Send types
                progress_tx.send(ProgressMessage::new("Forking and preparing database")).ok();
                let fork_result = match fork_and_prepare(&rpc_url, tx_hash, quick).await {
                    Ok(result) => result,
                    Err(e) => {
                        error!("Failed to fork and prepare: {}", e);
                        let _ = response_tx.send(Err(e));
                        continue;
                    }
                };

                // Run engine.prepare - this also uses !Send types
                progress_tx.send(ProgressMessage::new("Preparing engine")).ok();
                let result = match engine.prepare(fork_result, Some(progress_tx)).await {
                    Ok(addr) => {
                        info!("RPC server started on port {} for tx: {:?}", addr.port(), tx_hash);
                        Ok(addr.port())
                    }
                    Err(e) => {
                        error!("Failed to prepare engine: {}", e);
                        Err(e)
                    }
                };

                // Send the result back
                let _ = response_tx.send(result);
            }
        }
    }

    info!("Worker task shutting down");
}
