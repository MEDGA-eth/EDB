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

//! Replay command - replay_transaction function and tests

use alloy_primitives::TxHash;
use edb_common::fork_and_prepare;
use edb_engine::Engine;
use eyre::Result;

use crate::{Ui, utils};

/// Replay an existing transaction following the correct architecture
pub async fn replay_transaction(tx_hash: TxHash, cli: &crate::Cli, rpc_url: &str) -> Result<()> {
    tracing::info!("Starting transaction replay workflow");

    let fork_result = fork_and_prepare(rpc_url, tx_hash, cli.quick).await?;
    tracing::info!(
        "Forked chain and prepared database for transaction replay at block {}",
        fork_result.fork_info.block_number
    );

    let engine_config = cli.to_engine_config(rpc_url);
    let engine = Engine::new(engine_config);

    let rpc_server_addr = match cli.ui {
        Ui::Tui => engine.prepare(fork_result, None).await?,
        Ui::Web => engine.prepare_with_router(fork_result, None, Some(edb_web::router())).await?,
    };

    match cli.ui {
        Ui::Tui => {
            utils::start_tui(&cli.tui_options, rpc_server_addr).await?;
        }
        Ui::Web => {
            let url = format!("http://{rpc_server_addr}/");
            utils::open_browser(&url);
            tracing::info!("Web UI ready. Press Ctrl+C to exit.");
            tokio::signal::ctrl_c().await?;
        }
    }

    tracing::info!("Shutting down EDB...");
    engine.shutdown_rpc_server(&tx_hash)?;
    Ok(())
}
