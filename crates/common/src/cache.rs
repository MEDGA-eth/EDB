// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Cache utilities.

use std::{fs, marker::PhantomData, path::PathBuf, time::Duration};

use alloy_chains::Chain;
use eyre::Result;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tracing::{trace, warn};

/// Default cache TTL for etherscan.
/// Set to 1 day since the source code of a contract is unlikely to change frequently.
pub const DEFAULT_ETHERSCAN_CACHE_TTL: u64 = 86400;

/// Trait for cache paths.
pub trait CachePath {
    /// Returns the path to edb's cache dir: `~/.edb/cache` by default.
    fn edb_cache_dir(&self) -> Option<PathBuf>;

    /// Check whether the cache is valid.
    fn is_valid(&self) -> bool {
        self.edb_cache_dir().is_some()
    }

    /// Returns the path to edb rpc cache dir: `<cache_root>/rpc`.
    fn edb_rpc_cache_dir(&self) -> Option<PathBuf> {
        Some(self.edb_cache_dir()?.join("rpc"))
    }
    /// Returns the path to edb chain's cache dir: `<cache_root>/rpc/<chain>`
    fn rpc_chain_cache_dir(&self, chain_id: impl Into<Chain>) -> Option<PathBuf> {
        Some(self.edb_rpc_cache_dir()?.join(chain_id.into().to_string()))
    }

    /// Returns the path to edb's etherscan cache dir: `<cache_root>/etherscan`.
    fn etherscan_cache_dir(&self) -> Option<PathBuf> {
        Some(self.edb_cache_dir()?.join("etherscan"))
    }

    /// Returns the path to edb's etherscan cache dir for `chain_id`:
    /// `<cache_root>/etherscan/<chain>`
    fn etherscan_chain_cache_dir(&self, chain_id: impl Into<Chain>) -> Option<PathBuf> {
        Some(self.etherscan_cache_dir()?.join(chain_id.into().to_string()))
    }

    /// Returns the path to edb's compiler cache dir: `<cache_root>/solc`.
    fn compiler_cache_dir(&self) -> Option<PathBuf> {
        Some(self.edb_cache_dir()?.join("solc"))
    }

    /// Returns the path to edb's compiler cache dir for `chain_id`:
    /// `<cache_root>/solc/<chain>`
    fn compiler_chain_cache_dir(&self, chain_id: impl Into<Chain>) -> Option<PathBuf> {
        Some(self.compiler_cache_dir()?.join(chain_id.into().to_string()))
    }
}

/// Cache path for edb.
#[derive(Debug)]
pub struct EdbCachePath {
    root: Option<PathBuf>,
}

impl Default for EdbCachePath {
    fn default() -> Self {
        Self { root: dirs_next::home_dir().map(|p| p.join(".edb").join("cache")) }
    }
}

impl EdbCachePath {
    /// New cache path.
    pub fn new(root: Option<impl Into<PathBuf>>) -> Self {
        Self {
            root: root
                .map(Into::into)
                .or_else(|| dirs_next::home_dir().map(|p| p.join(".edb").join("cache"))),
        }
    }

    /// New empty cache path.
    pub fn empty() -> Self {
        Self { root: None }
    }
}

impl CachePath for EdbCachePath {
    fn edb_cache_dir(&self) -> Option<PathBuf> {
        self.root.clone()
    }
}

impl CachePath for Option<EdbCachePath> {
    fn edb_cache_dir(&self) -> Option<PathBuf> {
        self.as_ref()?.edb_cache_dir()
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

/// Trait for cache.
pub trait Cache {
    /// The type of the data to be cached.
    type Data: Serialize + DeserializeOwned;

    /// Loads the cache for the given label.
    fn load_cache(&self, label: impl Into<String>) -> Option<Self::Data>;

    /// Saves the cache for the given label.
    fn save_cache(&self, label: impl Into<String>, data: &Self::Data) -> Result<()>;
}

/// A cache manager that stores data in the file system.
///  - `T` is the type of the data to be cached.
///  - `cache_dir` is the directory where the cache files are stored.
///  - `cache_ttl` is the time-to-live of the cache files. If it is `None`, the cache files will
///    never expire.
#[derive(Debug, Clone)]
pub struct EdbCache<T> {
    cache_dir: PathBuf,
    cache_ttl: Option<Duration>,
    phantom: PhantomData<T>,
}

impl<T> EdbCache<T>
where
    T: Serialize + DeserializeOwned,
{
    /// New cache.
    pub fn new(
        cache_dir: Option<impl Into<PathBuf>>,
        cache_ttl: Option<Duration>,
    ) -> Result<Option<Self>> {
        if let Some(cache_dir) = cache_dir {
            let cache_dir = cache_dir.into();
            fs::create_dir_all(&cache_dir)?;
            Ok(Some(Self { cache_dir, cache_ttl, phantom: PhantomData }))
        } else {
            Ok(None)
        }
    }

    /// Returns the cache directory.
    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    /// Returns the cache TTL.
    pub fn cache_ttl(&self) -> Option<Duration> {
        self.cache_ttl
    }
}

impl<T> Cache for EdbCache<T>
where
    T: Serialize + DeserializeOwned,
{
    type Data = T;

    fn load_cache(&self, label: impl Into<String>) -> Option<T> {
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

    fn save_cache(&self, label: impl Into<String>, data: &T) -> Result<()> {
        let cache_file = self.cache_dir.join(format!("{}.json", label.into()));
        trace!("saving cache: {:?}", cache_file);

        let cache = CacheWrapper::new(data, self.cache_ttl);
        let content = serde_json::to_string(&cache)?;
        fs::write(&cache_file, content)?;
        Ok(())
    }
}

impl<T> Cache for Option<EdbCache<T>>
where
    T: Serialize + DeserializeOwned,
{
    type Data = T;

    fn load_cache(&self, label: impl Into<String>) -> Option<T> {
        self.as_ref()?.load_cache(label)
    }

    fn save_cache(&self, label: impl Into<String>, data: &T) -> Result<()> {
        if let Some(cache) = self { cache.save_cache(label, data) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_chains::Chain;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestData {
        value: String,
        number: u32,
    }

    #[test]
    fn test_edb_cache_path_default() {
        let cache_path = EdbCachePath::default();
        assert!(cache_path.is_valid());

        if let Some(cache_dir) = cache_path.edb_cache_dir() {
            assert!(cache_dir.to_string_lossy().contains(".edb"));
            assert!(cache_dir.to_string_lossy().contains("cache"));
        }
    }

    #[test]
    fn test_edb_cache_path_new_with_root() {
        let temp_path = std::env::temp_dir().join("edb_test");
        let cache_path = EdbCachePath::new(Some(&temp_path));

        assert!(cache_path.is_valid());
        assert_eq!(cache_path.edb_cache_dir(), Some(temp_path));
    }

    #[test]
    fn test_edb_cache_path_empty() {
        let cache_path = EdbCachePath::empty();
        assert!(!cache_path.is_valid());
        assert!(cache_path.edb_cache_dir().is_none());
    }

    #[test]
    fn test_cache_path_directories() {
        let temp_path = std::env::temp_dir().join("edb_test_2");
        let cache_path = EdbCachePath::new(Some(&temp_path));

        // Test RPC cache directories
        let rpc_dir = cache_path.edb_rpc_cache_dir();
        assert!(rpc_dir.is_some());
        assert!(rpc_dir.unwrap().ends_with("rpc"));

        let eth_rpc_dir = cache_path.rpc_chain_cache_dir(Chain::mainnet());
        assert!(eth_rpc_dir.is_some());
        assert!(eth_rpc_dir.unwrap().ends_with("mainnet"));

        // Test Etherscan cache directories
        let etherscan_dir = cache_path.etherscan_cache_dir();
        assert!(etherscan_dir.is_some());
        assert!(etherscan_dir.unwrap().ends_with("etherscan"));

        let eth_etherscan_dir = cache_path.etherscan_chain_cache_dir(Chain::mainnet());
        assert!(eth_etherscan_dir.is_some());
        assert!(eth_etherscan_dir.unwrap().ends_with("mainnet"));

        // Test compiler cache directories
        let compiler_dir = cache_path.compiler_cache_dir();
        assert!(compiler_dir.is_some());
        assert!(compiler_dir.unwrap().ends_with("solc"));

        let eth_compiler_dir = cache_path.compiler_chain_cache_dir(Chain::mainnet());
        assert!(eth_compiler_dir.is_some());
        assert!(eth_compiler_dir.unwrap().ends_with("mainnet"));
    }

    #[test]
    fn test_cache_wrapper_no_ttl() {
        let data = TestData { value: "test".to_string(), number: 42 };
        let wrapper = CacheWrapper::new(data.clone(), None);

        assert_eq!(wrapper.data, data);
        assert_eq!(wrapper.expires_at, u64::MAX);
        assert!(!wrapper.is_expired());
    }

    #[test]
    fn test_cache_wrapper_with_ttl() {
        let data = TestData { value: "test".to_string(), number: 42 };
        let ttl = Duration::from_secs(3600); // 1 hour
        let wrapper = CacheWrapper::new(data.clone(), Some(ttl));

        assert_eq!(wrapper.data, data);
        assert!(wrapper.expires_at > chrono::Utc::now().timestamp() as u64);
        assert!(!wrapper.is_expired());
    }

    #[test]
    fn test_cache_wrapper_expired() {
        let data = TestData { value: "test".to_string(), number: 42 };
        let ttl = Duration::from_secs(0);
        let wrapper = CacheWrapper::new(data, Some(ttl));

        // Since TTL is 0, it should be expired immediately
        std::thread::sleep(Duration::from_millis(2000));
        assert!(wrapper.is_expired());
    }

    #[test]
    fn test_edb_cache_new_with_directory() {
        let temp_path = std::env::temp_dir().join("edb_test_cache");
        let cache = EdbCache::<TestData>::new(Some(&temp_path), None).unwrap();

        assert!(cache.is_some());
        let cache = cache.unwrap();
        assert_eq!(cache.cache_dir(), &temp_path);
        assert!(cache.cache_ttl().is_none());
    }

    #[test]
    fn test_edb_cache_new_with_ttl() {
        let temp_path = std::env::temp_dir().join("edb_test_cache");
        let ttl = Duration::from_secs(3600);
        let cache = EdbCache::<TestData>::new(Some(&temp_path), Some(ttl)).unwrap();

        assert!(cache.is_some());
        let cache = cache.unwrap();
        assert_eq!(cache.cache_ttl(), Some(ttl));
    }

    #[test]
    fn test_edb_cache_new_no_directory() {
        let cache = EdbCache::<TestData>::new(None::<&str>, None).unwrap();
        assert!(cache.is_none());
    }

    #[test]
    fn test_edb_cache_save_and_load() {
        let temp_path = std::env::temp_dir().join("edb_test_cache");
        let cache = EdbCache::<TestData>::new(Some(&temp_path), None).unwrap().unwrap();

        let test_data = TestData { value: "hello".to_string(), number: 123 };
        let label = "test_label";

        // Save data
        cache.save_cache(label, &test_data).unwrap();

        // Load data
        let loaded_data = cache.load_cache(label);
        assert!(loaded_data.is_some());
        assert_eq!(loaded_data.unwrap(), test_data);
    }

    #[test]
    fn test_edb_cache_load_nonexistent() {
        let temp_path = std::env::temp_dir().join("edb_test_cache");
        let cache = EdbCache::<TestData>::new(Some(&temp_path), None).unwrap().unwrap();

        let loaded_data = cache.load_cache("nonexistent");
        assert!(loaded_data.is_none());
    }

    #[test]
    fn test_edb_cache_expired_data() {
        let temp_path = std::env::temp_dir().join("edb_test_cache");
        let ttl = Duration::from_millis(10);
        let cache = EdbCache::<TestData>::new(Some(&temp_path), Some(ttl)).unwrap().unwrap();

        let test_data = TestData { value: "expire_me".to_string(), number: 999 };
        let label = "expire_test";

        // Save data
        cache.save_cache(label, &test_data).unwrap();

        // Wait for expiration
        std::thread::sleep(Duration::from_millis(2000));

        // Try to load expired data
        let loaded_data = cache.load_cache(label);
        assert!(loaded_data.is_none());

        // Cache file should be removed
        let cache_file = &temp_path.join(format!("{label}.json"));
        assert!(!cache_file.exists());
    }

    #[test]
    fn test_edb_cache_corrupted_file() {
        let temp_path = std::env::temp_dir().join("edb_test_cache");
        let cache = EdbCache::<TestData>::new(Some(&temp_path), None).unwrap().unwrap();

        let label = "corrupted";
        let cache_file = &temp_path.join(format!("{label}.json"));

        // Write corrupted data
        fs::write(cache_file, "invalid json").unwrap();

        // Try to load corrupted data
        let loaded_data = cache.load_cache(label);
        assert!(loaded_data.is_none());

        // Corrupted file should be removed
        assert!(!cache_file.exists());
    }

    #[test]
    fn test_option_cache_some() {
        let temp_path = std::env::temp_dir().join("edb_test_cache");
        let cache = EdbCache::<TestData>::new(Some(&temp_path), None).unwrap();

        let test_data = TestData { value: "optional".to_string(), number: 456 };
        let label = "option_test";

        // Save through Option wrapper
        cache.save_cache(label, &test_data).unwrap();

        // Load through Option wrapper
        let loaded_data = cache.load_cache(label);
        assert!(loaded_data.is_some());
        assert_eq!(loaded_data.unwrap(), test_data);
    }

    #[test]
    fn test_option_cache_none() {
        let cache: Option<EdbCache<TestData>> = None;
        let test_data = TestData { value: "none".to_string(), number: 789 };

        // Save should succeed but do nothing
        let result = cache.save_cache("none_test", &test_data);
        assert!(result.is_ok());

        // Load should return None
        let loaded_data = cache.load_cache("none_test");
        assert!(loaded_data.is_none());
    }

    #[test]
    fn test_option_cache_path_some() {
        let temp_path = std::env::temp_dir().join("edb_test_cache");
        let cache_path = EdbCachePath::new(Some(&temp_path));

        assert!(cache_path.is_valid());
        assert_eq!(cache_path.edb_cache_dir(), Some(temp_path));
    }

    #[test]
    fn test_option_cache_path_none() {
        let cache_path: Option<EdbCachePath> = None;
        assert!(cache_path.edb_cache_dir().is_none());
    }

    #[test]
    fn test_multiple_cache_operations() {
        let temp_path = std::env::temp_dir().join("edb_test_cache");
        let cache = EdbCache::<TestData>::new(Some(&temp_path), None).unwrap().unwrap();

        // Save multiple items
        for i in 0..10 {
            let data = TestData { value: format!("item_{i}"), number: i as u32 };
            cache.save_cache(format!("item_{i}"), &data).unwrap();
        }

        // Load and verify all items
        for i in 0..10 {
            let loaded = cache.load_cache(format!("item_{i}"));
            assert!(loaded.is_some());
            let loaded = loaded.unwrap();
            assert_eq!(loaded.value, format!("item_{i}"));
            assert_eq!(loaded.number, i as u32);
        }
    }

    #[test]
    fn test_cache_overwrite() {
        let temp_path = std::env::temp_dir().join("edb_test_cache");
        let cache = EdbCache::<TestData>::new(Some(&temp_path), None).unwrap().unwrap();

        let label = "overwrite_test";

        // Save initial data
        let data1 = TestData { value: "original".to_string(), number: 1 };
        cache.save_cache(label, &data1).unwrap();

        // Overwrite with new data
        let data2 = TestData { value: "updated".to_string(), number: 2 };
        cache.save_cache(label, &data2).unwrap();

        // Load should return the updated data
        let loaded = cache.load_cache(label);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap(), data2);
    }
}
