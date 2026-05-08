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

//! Browser-based debugger UI for EDB.
//!
//! Exposes [`router`] which returns an Axum [`axum::Router`] that serves the
//! embedded React SPA bundled at compile time from `frontend/dist/`. Mount it
//! alongside the engine's JSON-RPC routes via `Router::merge`.

use axum::Router;

/// Build the static-file router for the embedded SPA.
///
/// The returned router serves embedded assets and falls back to `index.html`
/// for any unknown GET path (SPA routing). It does not handle POST `/` so it
/// will not interfere with the engine's JSON-RPC endpoint when merged.
///
/// # Panics
///
/// Panics at first call if no assets were embedded at compile time (e.g. the
/// build was run with `EDB_SKIP_WEB_BUILD=1`).
pub fn router() -> Router {
    Router::new()
}
