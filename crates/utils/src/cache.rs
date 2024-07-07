use std::{fs, marker::PhantomData, path::PathBuf, time::Duration};

use alloy_chains::Chain;
use eyre::Result;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

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

    /// Returns the path to edb's etherscan cache dir: `~/.edb/cache/etherscan`.
    pub fn edb_etherscan_cache_dir() -> Option<PathBuf> {
        Some(Self::edb_cache_dir()?.join("etherscan"))
    }

    /// Returns the path to edb's etherscan cache dir for `chain_id`:
    /// `~/.edb/cache/etherscan/<chain>`
    pub fn edb_etherscan_chain_cache_dir(chain_id: impl Into<Chain>) -> Option<PathBuf> {
        Some(Self::edb_etherscan_cache_dir()?.join(chain_id.into().to_string()))
    }

    /// Returns the path to edb's compiler cache dir: `~/.edb/cache/solc`.
    pub fn edb_compiler_cache_dir() -> Option<PathBuf> {
        Some(Self::edb_cache_dir()?.join("solc"))
    }

    /// Returns the path to edb's compiler cache dir for `chain_id`:
    /// `~/.edb/cache/solc/<chain>`
    pub fn edb_compiler_chain_cache_dir(chain_id: impl Into<Chain>) -> Option<PathBuf> {
        Some(Self::edb_compiler_cache_dir()?.join(chain_id.into().to_string()))
    }

    /// Returns the path to edb's backend cache dir: `~/.edb/cache/backend`.
    pub fn edb_backend_cache_dir() -> Option<PathBuf> {
        Some(Self::edb_cache_dir()?.join("backend"))
    }

    /// Returns the path to edb's backend cache dir for `chain_id`:
    /// `~/.edb/cache/backend/<chain>`
    pub fn edb_backend_chain_cache_dir(chain_id: impl Into<Chain>) -> Option<PathBuf> {
        Some(Self::edb_backend_cache_dir()?.join(chain_id.into().to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheWrapper<T> {
    pub data: T,
    pub expires_at: u64,
}

impl<T> CacheWrapper<T> {
    pub fn new(data: T, ttl: Option<Duration>) -> Self {
        Self {
            data,
            expires_at: ttl
                .map(|ttl| ttl.as_secs().saturating_add(chrono::Utc::now().timestamp() as u64))
                .unwrap_or(u64::MAX),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at < chrono::Utc::now().timestamp() as u64
    }
}

/// A cache manager that stores data in the file system.
///  - `T` is the type of the data to be cached.
///  - `cache_dir` is the directory where the cache files are stored.
///  - `cache_ttl` is the time-to-live of the cache files. If it is `None`, the cache files will
///    never expire.
#[derive(Debug, Clone)]
pub struct Cache<T> {
    cache_dir: PathBuf,
    cache_ttl: Option<Duration>,
    phantom: PhantomData<T>,
}

impl<T> Cache<T>
where
    T: Serialize + DeserializeOwned,
{
    pub fn new(cache_dir: impl Into<PathBuf>, cache_ttl: Option<Duration>) -> Result<Self> {
        let cache_dir = cache_dir.into();
        fs::create_dir_all(&cache_dir)?;
        Ok(Self { cache_dir, cache_ttl, phantom: PhantomData })
    }

    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    pub fn cache_ttl(&self) -> Option<Duration> {
        self.cache_ttl
    }

    pub fn load_cache(&self, label: impl Into<String>) -> Option<T> {
        let cache_file = self.cache_dir.join(format!("{}.json", label.into()));
        trace!("loading cache: {:?}", cache_file);
        if !cache_file.exists() {
            return None;
        }

        let content = fs::read_to_string(&cache_file).ok()?;
        let cache: CacheWrapper<_> = if let Ok(cache) = serde_json::from_str(&content) {
            cache
        } else {
            warn!("the cache file has been corrupted: {:?}", cache_file);
            let _ = fs::remove_file(&cache_file); // we do not care about the result
            return None;
        };

        if cache.is_expired() {
            trace!("the cache file has expired: {:?}", cache_file);
            let _ = fs::remove_file(&cache_file); // we do not care about the result
            None
        } else {
            trace!("hit the cache: {:?}", cache_file);
            Some(cache.data)
        }
    }

    pub fn save_cache(&self, label: impl Into<String>, data: &T) -> Result<()> {
        let cache_file = self.cache_dir.join(format!("{}.json", label.into()));
        trace!("saving cache: {:?}", cache_file);

        let cache = CacheWrapper::new(data, self.cache_ttl);
        let content = serde_json::to_string(&cache)?;
        fs::write(&cache_file, content)?;
        Ok(())
    }
}
