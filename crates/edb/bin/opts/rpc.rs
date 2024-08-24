use std::borrow::Cow;

use alloy_provider::network::AnyNetwork;
use clap::Parser;
use edb_utils::rpc::CachedProvider;
use eyre::Result;
use foundry_common::provider::{ProviderBuilder, RetryProvider};

const FLASHBOTS_URL: &str = "https://rpc.flashbots.net/fast";
const LOCALHOST_URL: &str = "http://localhost:8545";

#[derive(Clone, Debug, Default, Parser)]
pub struct RpcOpts {
    /// The RPC endpoint.
    #[arg(short = 'r', long = "rpc-url", env = "ETH_RPC_URL")]
    pub url: Option<String>,

    /// Sets the number of assumed available compute units per second for this provider
    ///
    /// default value: 330
    ///
    /// See also, https://docs.alchemy.com/reference/compute-units#what-are-cups-compute-units-per-second
    #[arg(long, alias = "cups", value_name = "CUPS")]
    pub compute_units_per_second: Option<u64>,

    /// Disables rate limiting for this node's provider.
    ///
    /// default value: false
    ///
    /// See also, https://docs.alchemy.com/reference/compute-units#what-are-cups-compute-units-per-second
    #[arg(long, value_name = "NO_RATE_LIMITS", visible_alias = "no-rpc-rate-limit")]
    pub no_rate_limit: bool,

    /// Use the Flashbots RPC URL with fast mode (<https://rpc.flashbots.net/fast>).
    ///
    /// This shares the transaction privately with all registered builders.
    ///
    /// See: <https://docs.flashbots.net/flashbots-protect/quick-start#faster-transactions>
    #[arg(long)]
    pub flashbots: bool,

    /// JWT Secret for the RPC endpoint.
    ///
    /// The JWT secret will be used to create a JWT for a RPC. For example, the following can be
    /// used to simulate a CL `engine_forkchoiceUpdated` call:
    ///
    /// cast rpc --jwt-secret <JWT_SECRET> engine_forkchoiceUpdatedV2
    /// '["0x6bb38c26db65749ab6e472080a3d20a2f35776494e72016d1e339593f21c59bc",
    /// "0x6bb38c26db65749ab6e472080a3d20a2f35776494e72016d1e339593f21c59bc",
    /// "0x6bb38c26db65749ab6e472080a3d20a2f35776494e72016d1e339593f21c59bc"]'
    #[arg(long, env = "ETH_RPC_JWT_SECRET")]
    pub jwt_secret: Option<String>,
}

impl RpcOpts {
    /// Returns the RPC endpoint.
    pub fn url(&self, fallback_to_default: bool) -> Result<Option<Cow<'_, str>>> {
        let url = match (self.flashbots, self.url.as_deref()) {
            (true, ..) => Some(Cow::Borrowed(FLASHBOTS_URL)),
            (false, Some(url)) => Some(Cow::Borrowed(url)),
            (false, None) if fallback_to_default => Some(Cow::Borrowed(LOCALHOST_URL)),
            _ => None,
        };
        Ok(url)
    }

    #[allow(dead_code)] // XXX: decide whether to keep this after having the first prototype
    /// Returns the JWT secret.
    pub fn jwt(&self) -> Result<Option<Cow<'_, str>>> {
        Ok(self.jwt_secret.as_deref().map(Cow::Borrowed))
    }

    /// Create a RPC provider.
    pub fn provider(
        &self,
        fallback_to_default: bool,
    ) -> Result<CachedProvider<RetryProvider, AnyNetwork>> {
        let fork_url = self.url(fallback_to_default)?.unwrap().to_string();
        let compute_units_per_second =
            if self.no_rate_limit { Some(u64::MAX) } else { self.compute_units_per_second };

        let mut provider_builder =
            ProviderBuilder::new(&fork_url).compute_units_per_second_opt(compute_units_per_second);

        if let Some(jwt) = self.jwt_secret.as_deref() {
            provider_builder = provider_builder.jwt(jwt);
        }

        let provider = provider_builder.build()?;

        Ok(CachedProvider::new(provider))
    }
}
