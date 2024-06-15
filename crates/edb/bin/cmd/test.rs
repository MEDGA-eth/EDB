use clap::Parser;
use eyre::Result;

use super::{etherscan::EtherscanOpts, rpc::RpcOpts};

/// CLI arguments for `edb replay`.
#[derive(Clone, Debug, Parser)]
pub struct TestArgs {
    #[command(flatten)]
    pub etherscan: EtherscanOpts,

    #[command(flatten)]
    rpc: RpcOpts,
}

impl TestArgs {
    pub async fn run(self) -> Result<()> {
        unimplemented!()
    }
}
