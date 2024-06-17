use clap::Parser;
use eyre::Result;

use crate::opts::{EtherscanOpts, RpcOpts};

/// CLI arguments for `edb replay`.
#[derive(Clone, Debug, Parser)]
pub struct ScriptArgs {
    #[command(flatten)]
    pub etherscan: EtherscanOpts,

    #[command(flatten)]
    rpc: RpcOpts,
}

impl ScriptArgs {
    pub async fn run(self) -> Result<()> {
        unimplemented!()
    }
}
