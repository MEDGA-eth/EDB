#[macro_use]
extern crate tracing;

mod cmd;
mod args;
mod utils;
mod opts;

use clap::Parser;
use eyre::Result;
use args::{EDBArgs, EDBSubcommand};

fn main() -> Result<()> {
    utils::install_error_handler();
    utils::subscriber();
    utils::enable_paint();

    let opts = EDBArgs::parse();

    match opts.cmd {
        EDBSubcommand::Replay(cmd) => utils::block_on(cmd.run()),
        EDBSubcommand::Script(cmd) => utils::block_on(cmd.run()),
        EDBSubcommand::Test(cmd) => utils::block_on(cmd.run()),
    }
}
