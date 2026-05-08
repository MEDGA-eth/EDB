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

//! TUI (Terminal User Interface) for monitoring the RPC proxy
//!
//! Provides a real-time monitoring interface showing:
//! - Provider health and response times
//! - Cache statistics and hit rates
//! - EDB instance registry
//! - Request metrics and performance charts
//! - Enhanced metrics (cache hit rates, provider usage analytics)

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use eyre::Result;
use ratatui::prelude::*;
use std::{
    io,
    time::{Duration, Instant},
};
use tokio::time::sleep;

mod app;
pub mod remote;
mod widgets;

use app::App;
use remote::RemoteProxyClient;

async fn run_tui_loop<B>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()>
where
    B: Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(app.refresh_interval); // 4 FPS

    loop {
        // Handle events
        if event::poll(Duration::from_millis(0))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('r') => app.refresh().await,
                KeyCode::Char('c') => app.clear_cache().await,
                KeyCode::Char('h') => app.toggle_help(),
                KeyCode::Tab => app.next_tab(),
                KeyCode::BackTab => app.previous_tab(),
                KeyCode::Up => app.scroll_up(),
                KeyCode::Down => app.scroll_down(),
                KeyCode::Left => app.previous_provider(),
                KeyCode::Right => app.next_provider(),
                _ => {}
            }
        }

        // Update app state on tick
        if last_tick.elapsed() >= tick_rate {
            app.update().await;
            last_tick = Instant::now();
        }

        // Render UI
        terminal.draw(|f| app.render(f))?;

        // Small sleep to prevent CPU spinning
        sleep(Duration::from_millis(10)).await;
    }

    Ok(())
}

/// Run the TUI interface for monitoring a remote proxy server
pub async fn run_tui(proxy_url: String, refresh_interval: u64, timeout: u64) -> Result<()> {
    // Create remote client
    let client = RemoteProxyClient::new(proxy_url.clone(), timeout);

    // Test connection
    match client.ping().await {
        Ok(_) => {
            tracing::info!("Successfully connected to proxy at {}", proxy_url);
        }
        Err(e) => {
            tracing::error!("Failed to connect to proxy at {}: {}", proxy_url, e);
            eyre::bail!("Cannot connect to proxy server. Make sure it's running and accessible.");
        }
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state for remote monitoring
    let mut app = App::new_remote(client, refresh_interval, proxy_url);

    // Run TUI loop
    let result = run_tui_loop(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    result
}
