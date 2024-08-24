use crate::cmd::{disasm::DisasmArgs, replay::ReplayArgs, script::ScriptArgs, test::TestArgs};
use clap::{Parser, Subcommand};

const VERSION_MESSAGE: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("VERGEN_GIT_SHA"),
    " ",
    env!("VERGEN_BUILD_TIMESTAMP"),
    ")"
);

/// EDB: The EVM Project Debugger.
#[derive(Parser, Debug)]
#[command(
    name = "edb",
    version = VERSION_MESSAGE,
    after_help = "Find more information in our homepage: https://medga.org/",
    next_display_order = None,
)]
pub struct EDBArgs {
    #[command(subcommand)]
    pub cmd: EDBSubcommand,
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum EDBSubcommand {
    /// Replay and debug an on-chain transaction.
    #[command(visible_alias = "r")]
    Replay(ReplayArgs),

    /// Debug a script.
    #[command(visible_alias = "s")]
    Script(ScriptArgs),

    /// Debug a test case.
    #[command(visible_alias = "t")]
    Test(TestArgs),

    /// Disassemble a contract, if source code is avaiable, a refined source-mapped information
    /// will be displayed as well.
    #[command(visible_alias = "d")]
    Disasm(DisasmArgs),
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify_cli() {
        EDBArgs::command().debug_assert();
    }
}
