#[macro_use]
extern crate tracing;

mod actions;
mod context;
mod core;
mod draw;
mod utils;
mod window;

pub use core::DebugFrontend;

use ratatui::{backend::CrosstermBackend, Terminal};

type FrontendTerminal = Terminal<CrosstermBackend<std::io::Stdout>>;
