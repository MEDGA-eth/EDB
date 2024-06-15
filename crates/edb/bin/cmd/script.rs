use clap::Parser;
use eyre::Result;

use super::{etherscan::EtherscanOpts, rpc::RpcOpts};

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
