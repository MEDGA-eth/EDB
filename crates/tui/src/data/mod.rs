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

//! Centralized state management system for TUI
//!
//! This module implements a unified manager architecture with the following components:
//!
//! - `DataManager`: Central container holding all managers, passed to all app functions
//! - `ExecutionManager`: Manages trace and snapshot data with cached state
//! - `Resolver`: Handles ABI resolution and address labeling with cached lookups
//! - `Theme`: Direct theme configuration without async wrapping
//!
//! # Architecture
//!
//! The manager system follows a two-layer design:
//!
//! 1. **Manager Layer** (e.g., ExecutionManager, Resolver)
//!    - Holds cached state for immediate read access
//!    - Tracks pending requests when data is not cached
//!    - User-controllable state (e.g., current_snapshot) lives here
//!
//! 2. **Core Layer** (e.g., ExecutionManagerCore, ResolverCore)
//!    - Wrapped in Arc<tokio::sync::RwLock>
//!    - Handles RPC communication and data fetching
//!    - Processes pending requests from managers
//!    - Run by background tasks spawned in TUI::run()
//!
//! # Data Flow
//!
//! 1. Panels read from managers during render (non-blocking)
//! 2. Cache misses create pending requests
//! 3. App::update() pushes pending requests to cores
//! 4. Background tasks process core pending requests
//! 5. App::update() pulls processed data back to managers
//!
//! # Benefits
//!
//! - **Non-blocking UI**: Rendering never waits on RPC calls
//! - **Centralized state**: Single DataManager instance for all panels
//! - **Efficient caching**: Data fetched once, used everywhere
//! - **Clean separation**: UI logic separate from data fetching

use eyre::Result;
use std::sync::Arc;
use tokio::sync::RwLock;

pub mod manager;
pub mod theme;
pub mod watcher;

use crate::{
    RpcClient,
    data::{
        manager::{
            core::{ManagerCore, ManagerTr},
            execution::{ExecutionManager, ExecutionRequest, ExecutionState},
            resolve::{Resolver, ResolverRequest, ResolverState},
        },
        theme::Theme,
        watcher::Watcher,
    },
};

/// Central data manager containing all state managers
///
/// This structure is passed as a mutable reference to all app functions,
/// providing centralized access to execution state, resolver, and theme.
pub struct DataManager {
    /// Execution state manager for trace and snapshot data
    pub execution: ExecutionManager,
    /// Resolver for contract ABIs and labels
    pub resolver: Resolver,
    /// Theme configuration (no Arc/RwLock needed)
    pub theme: Theme,
    /// Expression watcher (no Arc/RwLock needed)
    pub watcher: Watcher,
}

impl DataManager {
    /// Create a new DataManager with all managers initialized
    pub async fn new(rpc_client: Arc<RpcClient>) -> Result<Self> {
        // Initialize cores with tokio::sync::RwLock
        let exec_core = Arc::new(RwLock::new(ManagerCore::new(rpc_client.clone()).await?));
        let resolver_core = Arc::new(RwLock::new(ManagerCore::new(rpc_client.clone()).await?));

        Ok(Self {
            execution: ExecutionManager::new(exec_core).await,
            resolver: Resolver::new(resolver_core).await,
            theme: Theme::default(),
            watcher: Watcher::default(),
        })
    }

    /// Push all pending requests from managers to their cores
    ///
    /// This should be called in app.update() to queue requests for processing
    pub async fn update_pending_requests(&mut self) -> Result<()> {
        self.execution.push_pending_to_core().await?;
        self.resolver.push_pending_to_core().await?;
        Ok(())
    }

    /// Pull processed data from cores back to managers
    ///
    /// This updates the cached state in managers with data processed by cores
    pub fn process_core_updates(&mut self) -> Result<()> {
        self.execution.pull_from_core()?;
        self.resolver.pull_from_core()?;
        Ok(())
    }

    /// Get clone of execution core for background processing
    pub fn get_execution_core(&self) -> Arc<RwLock<ManagerCore<ExecutionState, ExecutionRequest>>> {
        self.execution.get_core()
    }

    /// Get clone of resolver core for background processing
    pub fn get_resolver_core(&self) -> Arc<RwLock<ManagerCore<ResolverState, ResolverRequest>>> {
        self.resolver.get_core()
    }
}
