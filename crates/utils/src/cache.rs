use std::{fs, marker::PhantomData, path::PathBuf, time::Duration};

use alloy_chains::Chain;
use eyre::Result;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub struct CachePath {
    root: Option<PathBuf>,
}

impl CachePath {
    /// New cache path.
    pub fn new(root: Option<impl Into<PathBuf>>) -> Self {
        Self { root: root.map(Into::into) }
    }

    /// Returns the path to edb's cache dir: `~/.edb/cache` by default.
    pub fn edb_cache_dir(&self) -> Option<PathBuf> {
        self.root.clone().or_else(|| dirs_next::home_dir().map(|p| p.join(".edb").join("cache")))
    }

    /// Returns the path to edb rpc cache dir: `<cache_root>/rpc`.
    pub fn edb_rpc_cache_dir(&self) -> Option<PathBuf> {
        Some(self.edb_cache_dir()?.join("rpc"))
    }
    /// Returns the path to edb chain's cache dir: `<cache_root>/rpc/<chain>`
    pub fn edb_chain_cache_dir(&self, chain_id: impl Into<Chain>) -> Option<PathBuf> {
        Some(self.edb_rpc_cache_dir()?.join(chain_id.into().to_string()))
    }

    /// Returns the path to the cache dir of the `block` on the `chain`:
    /// `<cache_root>/rpc/<chain>/<block>`
    pub fn edb_block_cache_dir(&self, chain_id: impl Into<Chain>, block: u64) -> Option<PathBuf> {
        Some(self.edb_chain_cache_dir(chain_id)?.join(format!("{block}")))
    }

    /// Returns the path to the cache file of the `block` on the `chain`:
    /// `<cache_root>/rpc/<chain>/<block>/storage.json`
    pub fn edb_block_cache_file(&self, chain_id: impl Into<Chain>, block: u64) -> Option<PathBuf> {
        Some(self.edb_block_cache_dir(chain_id, block)?.join("storage.json"))
    }

    /// Returns the path to edb's etherscan cache dir: `<cache_root>/etherscan`.
    pub fn edb_etherscan_cache_dir(&self) -> Option<PathBuf> {
        Some(self.edb_cache_dir()?.join("etherscan"))
    }

    /// Returns the path to edb's etherscan cache dir for `chain_id`:
    /// `<cache_root>/etherscan/<chain>`
    pub fn edb_etherscan_chain_cache_dir(&self, chain_id: impl Into<Chain>) -> Option<PathBuf> {
        Some(self.edb_etherscan_cache_dir()?.join(chain_id.into().to_string()))
    }

    /// Returns the path to edb's compiler cache dir: `<cache_root>/solc`.
    pub fn edb_compiler_cache_dir(&self) -> Option<PathBuf> {
        Some(self.edb_cache_dir()?.join("solc"))
    }

    /// Returns the path to edb's compiler cache dir for `chain_id`:
    /// `<cache_root>/solc/<chain>`
    pub fn edb_compiler_chain_cache_dir(&self, chain_id: impl Into<Chain>) -> Option<PathBuf> {
        Some(self.edb_compiler_cache_dir()?.join(chain_id.into().to_string()))
    }

    /// Returns the path to edb's backend cache dir: `<cache_root>/backend`.
    pub fn edb_backend_cache_dir(&self) -> Option<PathBuf> {
        Some(self.edb_cache_dir()?.join("backend"))
    }

    /// Returns the path to edb's backend cache dir for `chain_id`:
    /// `<cache_root>/backend/<chain>`
    pub fn edb_backend_chain_cache_dir(&self, chain_id: impl Into<Chain>) -> Option<PathBuf> {
        Some(self.edb_backend_cache_dir()?.join(chain_id.into().to_string()))
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
    cache_dir: Option<PathBuf>,
    cache_ttl: Option<Duration>,
    phantom: PhantomData<T>,
}

impl<T> Cache<T>
where
    T: Serialize + DeserializeOwned,
{
    pub fn new(cache_dir: Option<impl Into<PathBuf>>, cache_ttl: Option<Duration>) -> Result<Self> {
        let cache_dir = cache_dir
            .map(|p| {
                let p = p.into();
                fs::create_dir_all(&p)?;
                Ok::<_, std::io::Error>(p)
            })
            .transpose()?;

        Ok(Self { cache_dir, cache_ttl, phantom: PhantomData })
    }

    pub fn cache_dir(&self) -> Option<&PathBuf> {
        self.cache_dir.as_ref()
    }

    pub fn cache_ttl(&self) -> Option<Duration> {
        self.cache_ttl
    }

    pub fn load_cache(&self, label: impl Into<String>) -> Option<T> {
        let cache_dir = self.cache_dir()?;
        let cache_file = cache_dir.join(format!("{}.json", label.into()));
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
        if let Some(cache_dir) = self.cache_dir() {
            let cache_file = cache_dir.join(format!("{}.json", label.into()));
            trace!("saving cache: {:?}", cache_file);

            let cache = CacheWrapper::new(data, self.cache_ttl);
            let content = serde_json::to_string(&cache)?;
            fs::write(&cache_file, content)?;
            Ok(())
        } else {
            Ok(())
        }
    }
}
