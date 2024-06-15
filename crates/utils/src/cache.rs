use std::path::PathBuf;

use alloy_chains::Chain;

pub struct CachePath {}

impl CachePath {
    /// Returns the path to edb's cache dir: `~/.edb/cache`.
    pub fn edb_cache_dir() -> Option<PathBuf> {
        dirs_next::home_dir().map(|p| p.join(".edb").join("cache"))
    }

    /// Returns the path to edb rpc cache dir: `~/.edb/cache/rpc`.
    pub fn edb_rpc_cache_dir() -> Option<PathBuf> {
        Some(Self::edb_cache_dir()?.join("rpc"))
    }
    /// Returns the path to edb chain's cache dir: `~/.edb/cache/rpc/<chain>`
    pub fn edb_chain_cache_dir(chain_id: impl Into<Chain>) -> Option<PathBuf> {
        Some(Self::edb_rpc_cache_dir()?.join(chain_id.into().to_string()))
    }

    /// Returns the path to edb's etherscan cache dir: `~/.edb/cache/etherscan`.
    pub fn edb_etherscan_cache_dir() -> Option<PathBuf> {
        Some(Self::edb_cache_dir()?.join("etherscan"))
    }

    /// Returns the path to the cache dir of the `block` on the `chain`:
    /// `~/.edb/cache/rpc/<chain>/<block>`
    pub fn edb_block_cache_dir(chain_id: impl Into<Chain>, block: u64) -> Option<PathBuf> {
        Some(Self::edb_chain_cache_dir(chain_id)?.join(format!("{block}")))
    }

    /// Returns the path to the cache file of the `block` on the `chain`:
    /// `~/.edb/cache/rpc/<chain>/<block>/storage.json`
    pub fn edb_block_cache_file(chain_id: impl Into<Chain>, block: u64) -> Option<PathBuf> {
        Some(Self::edb_block_cache_dir(chain_id, block)?.join("storage.json"))
    }
}
