use std::ffi::OsStr;

use alloy_chains::{Chain, NamedChain};
use clap::{
    builder::{PossibleValuesParser, TypedValueParser},
    Parser,
};
use eyre::Result;
use foundry_block_explorers::Client;
use serde::Serialize;
use strum::VariantNames;

/// Custom Clap value parser for [`Chain`]s.
///
/// Displays all possible chains when an invalid chain is provided.
#[derive(Clone, Debug)]
struct ChainValueParser {
    pub inner: PossibleValuesParser,
}

impl Default for ChainValueParser {
    fn default() -> Self {
        Self { inner: PossibleValuesParser::from(NamedChain::VARIANTS) }
    }
}

impl TypedValueParser for ChainValueParser {
    type Value = Chain;

    fn parse_ref(
        &self,
        cmd: &clap::Command,
        arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let s =
            value.to_str().ok_or_else(|| clap::Error::new(clap::error::ErrorKind::InvalidUtf8))?;
        if let Ok(id) = s.parse() {
            Ok(Chain::from_id(id))
        } else {
            // NamedChain::VARIANTS is a subset of all possible variants, since there are aliases:
            // mumbai instead of polygon-mumbai etc
            //
            // Parse first as NamedChain, if it fails parse with NamedChain::VARIANTS for displaying
            // the error to the user
            s.parse()
                .map_err(|_| self.inner.parse_ref(cmd, arg, value).unwrap_err())
                .map(Chain::from_named)
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Parser)]
pub struct EtherscanOpts {
    /// The Etherscan (or equivalent) API key.
    #[arg(short = 'e', long = "etherscan-api-key", alias = "api-key", env = "ETHERSCAN_API_KEY")]
    #[serde(rename = "etherscan_api_key", skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,

    /// The chain name or EIP-155 chain ID.
    #[arg(
        short,
        long,
        alias = "chain-id",
        env = "CHAIN",
        value_parser = ChainValueParser::default(),
    )]
    #[serde(rename = "chain_id", skip_serializing_if = "Option::is_none")]
    pub chain: Option<Chain>,
}

impl EtherscanOpts {
    #[allow(dead_code)] // XXX: decide whether to keep this after having the first prototype
    /// Returns true if the Etherscan API key is set.
    pub fn has_key(&self) -> bool {
        self.key.as_ref().filter(|key| !key.trim().is_empty()).is_some()
    }

    /// Returns the Etherscan API key.
    pub fn key(&self) -> Option<String> {
        self.key.as_ref().filter(|key| !key.trim().is_empty()).cloned()
    }

    /// Returns the chain.
    pub fn chain(&self) -> Chain {
        self.chain.unwrap_or_default()
    }

    /// Create an Etherscan Client.
    pub fn client(&self) -> Result<Client> {
        let chain = self.chain();
        let cb = Client::builder().chain(chain)?;

        if let Some(key) = self.key() {
            Ok(cb.with_api_key(key).build()?)
        } else {
            Ok(cb.build()?)
        }
    }
}
