use alloy_primitives::TxHash;
use clap::Parser;
use eyre::Result;

use super::{etherscan::EtherscanOpts, rpc::RpcOpts};

/// CLI arguments for `edb replay`.
#[derive(Clone, Debug, Parser)]
pub struct ReplayArgs {
    /// The hash of the transaction under replay.
    pub tx_hash: TxHash,

    #[command(flatten)]
    pub etherscan: EtherscanOpts,

    #[command(flatten)]
    rpc: RpcOpts,
}

impl ReplayArgs {
    pub async fn run(self) -> Result<()> {
        Ok(())
    }
}
