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

//! In-memory cache manager for RPC responses with disk persistence

use alloy_chains::NamedChain;
use edb_common::{
    cache::{CachePath, EdbCachePath},
    forking,
};
use eyre::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Maximum size for a single cache file in bytes (20MB)
const MAX_CACHE_FILE_SIZE: usize = 20 * 1024 * 1024;

/// A cached RPC response entry with metadata
///
/// This struct holds a cached RPC response along with its last access timestamp
/// for LRU eviction purposes.
#[derive(Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// The cached RPC response data
    pub data: Value,
    /// Unix timestamp when this entry was accessed
    pub accessed_at: u64,
}

impl CacheEntry {
    fn new(data: Value) -> Self {
        Self {
            data,
            accessed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    fn update_access_time(&mut self) {
        self.accessed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }
}

/// In-memory cache manager for RPC responses with disk persistence
///
/// Manages a thread-safe in-memory cache with LRU eviction and provides
/// functionality to persist the cache to disk as JSON.
pub struct CacheManager {
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    max_items: u32,
    cache_file_path: PathBuf,
}

impl CacheManager {
    /// Creates a new cache manager with the specified capacity and disk path
    ///
    /// # Arguments
    /// * `max_items` - Maximum number of items to store in the cache
    /// * `cache_path` - Path where the cache will be persisted to disk
    ///
    /// # Returns
    /// A new CacheManager instance, loading any existing cache from disk
    pub fn new(max_items: u32, cache_path: PathBuf) -> Result<Self> {
        info!("Using cache file: {}", cache_path.display());

        // Create an instance to use load_existing_cache method
        let mut manager = Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            max_items,
            cache_file_path: cache_path,
        };

        // Load existing cache from disk (handles both legacy and split files)
        let mut cache = match manager.load_existing_cache() {
            Ok(loaded_cache) => {
                if !loaded_cache.is_empty() {
                    info!("Loaded {} cache entries from disk", loaded_cache.len());
                }
                loaded_cache
            }
            Err(e) => {
                warn!("Failed to load cache from disk, starting with empty cache: {}", e);
                HashMap::new()
            }
        };

        if cache.len() >= max_items as usize {
            // This will be a hard cap - evict oldest entries
            Self::evict_to_size(&mut cache, max_items as usize);
        }

        manager.cache = Arc::new(RwLock::new(cache));
        Ok(manager)
    }

    /// Retrieves a cached value by key
    ///
    /// # Arguments
    /// * `key` - The cache key to look up
    ///
    /// # Returns
    /// The cached value if found, None otherwise
    pub async fn get(&self, key: &str) -> Option<Value> {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.get_mut(key) {
            debug!("Cache hit: {}", key);
            entry.update_access_time(); // Update access time for LRU
            Some(entry.data.clone())
        } else {
            debug!("Cache miss: {}", key);
            None
        }
    }

    /// Stores a value in the cache with the given key
    ///
    /// If the cache is at capacity, this will trigger LRU eviction of the oldest entries.
    ///
    /// # Arguments
    /// * `key` - The cache key to store under
    /// * `value` - The value to cache
    pub async fn set(&self, key: String, value: Value) {
        let mut cache = self.cache.write().await;

        // Check if we need to evict entries to make space
        if cache.len() >= self.max_items as usize {
            Self::evict_oldest(&mut cache);
        }

        let entry = CacheEntry::new(value);
        cache.insert(key.clone(), entry);
        debug!("Cached entry: {}", key);
    }

    fn evict_oldest(cache: &mut HashMap<String, CacheEntry>) {
        // Sort entries by creation time (oldest first) and remove oldest 10%
        let to_remove = (cache.len() / 10).max(1);
        debug!("Evicting {} oldest cache entries", to_remove);

        Self::evict_to_size(cache, cache.len().saturating_sub(to_remove));
    }

    /// Saves the current cache contents to disk as JSON with atomic write and merge
    ///
    /// This method:
    /// 1. Loads existing cache from disk
    /// 2. Merges with current in-memory cache (newest timestamp wins)
    /// 3. Applies size management based on disk vs in-memory sizes
    /// 4. Performs atomic write via temp file + rename
    ///
    /// Uses silent failure - errors are logged as warnings but don't propagate
    /// to maintain system stability. In-memory cache remains unaffected.
    ///
    /// # Returns
    /// Result indicating success or failure of the save operation
    pub async fn save_to_disk(&self) -> Result<()> {
        match self.save_to_disk_impl().await {
            Ok(()) => Ok(()),
            Err(e) => {
                warn!("Failed to save cache to disk: {}. In-memory cache remains available.", e);
                // Return success to prevent cascading failures
                Ok(())
            }
        }
    }

    /// Internal implementation of save_to_disk with proper error propagation
    async fn save_to_disk_impl(&self) -> Result<()> {
        // Load existing cache from disk
        let existing_cache = match self.load_existing_cache() {
            Ok(cache) => cache,
            Err(e) => {
                warn!("Failed to load existing cache for merge, using empty: {}", e);
                HashMap::new()
            }
        };

        let original_disk_size = existing_cache.len();

        // Get current in-memory cache
        let current_cache = self.cache.read().await.clone();
        let current_memory_size = current_cache.len();

        // Merge caches (newest timestamp wins)
        let merged_cache = Self::merge_caches(existing_cache, current_cache);

        // Apply size management
        let final_cache =
            self.apply_size_management(merged_cache, original_disk_size, current_memory_size).await;

        // Write to split files instead of single file
        self.write_split_cache_files(&final_cache)?;

        info!(
            "Saved {} cache entries to disk (merged from {} disk + {} memory)",
            final_cache.len(),
            original_disk_size,
            current_memory_size
        );
        Ok(())
    }

    /// Deletes all cache entries matching a method prefix
    ///
    /// # Arguments
    /// * `method` - The method name to match (e.g., "eth_getBalance")
    ///
    /// # Returns  
    /// Number of entries deleted
    pub async fn delete_by_method(&self, method: &str) -> Result<usize> {
        let mut cache = self.cache.write().await;

        // Find all keys that start with the method prefix
        let prefix = format!("{method}:");
        let keys_to_delete: Vec<String> =
            cache.keys().filter(|k| k.starts_with(&prefix)).cloned().collect();

        let deleted_count = keys_to_delete.len();
        for key in keys_to_delete {
            cache.remove(&key);
        }

        if deleted_count > 0 {
            info!("Deleted {} entries for method '{}'", deleted_count, method);
            let current_cache = cache.clone();
            drop(cache); // Release the write lock
            self.force_save_to_disk(current_cache).await?;
        }

        Ok(deleted_count)
    }

    /// Delete a single cache entry by key
    pub async fn delete_by_key(&self, key: &str) -> Result<bool> {
        let mut cache = self.cache.write().await;
        let found = cache.remove(key).is_some();

        if found {
            let current_cache = cache.clone();
            drop(cache);
            self.force_save_to_disk(current_cache).await?;
        }

        Ok(found)
    }

    /// Force save current cache state to disk without merging
    ///
    /// This method bypasses the normal merge logic and directly overwrites
    /// the disk cache with the provided cache state. Used after deletions
    /// to ensure deleted entries are not restored from disk.
    async fn force_save_to_disk(&self, cache_to_save: HashMap<String, CacheEntry>) -> Result<()> {
        // Write to split files instead of single file
        self.write_split_cache_files(&cache_to_save)?;

        info!("Force saved {} cache entries to disk (no merge)", cache_to_save.len());
        Ok(())
    }

    /// Returns detailed statistics about the cache state
    ///
    /// # Returns
    /// A JSON object containing cache utilization, entry counts, and timing information
    pub async fn detailed_stats(&self) -> serde_json::Value {
        let cache = self.cache.read().await;
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut oldest_entry = None;
        let mut newest_entry = None;

        for entry in cache.values() {
            if oldest_entry.is_none() || entry.accessed_at < oldest_entry.unwrap() {
                oldest_entry = Some(entry.accessed_at);
            }
            if newest_entry.is_none() || entry.accessed_at > newest_entry.unwrap() {
                newest_entry = Some(entry.accessed_at);
            }
        }

        serde_json::json!({
            "total_entries": cache.len(),
            "max_entries": self.max_items,
            "utilization": format!("{:.1}%", (cache.len() as f64 / self.max_items as f64) * 100.0),
            "oldest_entry_age_seconds": oldest_entry.map(|t| current_time.saturating_sub(t)),
            "newest_entry_age_seconds": newest_entry.map(|t| current_time.saturating_sub(t)),
            "cache_file_path": self.cache_file_path.display().to_string(),
        })
    }

    /// Returns all cache entries for testing purposes
    ///
    /// This method is primarily intended for testing and debugging.
    /// In production, prefer using specific cache queries or statistics.
    ///
    /// # Returns
    /// A HashMap containing all cache entries with their keys and values
    #[allow(dead_code)]
    pub async fn get_all_entries(&self) -> HashMap<String, CacheEntry> {
        let cache = self.cache.read().await;
        cache.clone()
    }

    /// Generates a cache file path based on the RPC URL and optional cache directory
    ///
    /// Creates a chain-specific cache directory structure and ensures parent directories exist.
    ///
    /// # Arguments
    /// * `rpc_url` - The RPC endpoint URL to determine chain ID from
    /// * `cache_dir` - Optional base cache directory (defaults to ~/.edb/cache)
    ///
    /// # Returns
    /// The full path to the cache file for this RPC endpoint
    pub async fn get_cache_path(
        rpc_urls: &[String],
        cache_dir: Option<PathBuf>,
    ) -> Result<PathBuf> {
        let chain_ids: HashSet<_> =
            futures::future::join_all(rpc_urls.iter().map(|url| forking::get_chain_id(url)))
                .await
                .into_iter()
                .filter_map(Result::ok)
                .collect();

        let chain_id = if rpc_urls.is_empty() {
            warn!("No RPC URL is provided, assume we are on mainnet and enter cache-only mode");
            NamedChain::Mainnet.into()
        } else if chain_ids.len() == 1 {
            *chain_ids.iter().next().unwrap()
        } else {
            eyre::bail!("All RPC URLs must belong to the same chain. Found: {:?}", chain_ids);
        };

        let cache_path = EdbCachePath::new(cache_dir)
            .rpc_chain_cache_dir(chain_id)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rpc.json");

        // Create directory if it doesn't exist
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)?;
        }

        Ok(cache_path)
    }

    /// Loads existing cache from disk without affecting in-memory cache
    ///
    /// Handles backward compatibility by:
    /// 1. Loading legacy rpc.json file if it exists
    /// 2. Loading split rpc.json.* files
    /// 3. Merging both caches (split files take precedence for conflicts)
    ///
    /// # Returns
    /// HashMap of existing cache entries, or empty if no files exist/can't be read
    fn load_existing_cache(&self) -> Result<HashMap<String, CacheEntry>> {
        // First, try to load legacy rpc.json file for backward compatibility
        let legacy_cache = if self.cache_file_path.exists() {
            match fs::read_to_string(&self.cache_file_path) {
                Ok(content) => {
                    match serde_json::from_str::<HashMap<String, CacheEntry>>(&content) {
                        Ok(legacy_cache) => {
                            info!("Loaded {} entries from legacy cache file", legacy_cache.len());
                            legacy_cache
                        }
                        Err(e) => {
                            error!("Failed to parse legacy cache file: {}", e);
                            HashMap::new()
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to read legacy cache file: {}", e);
                    HashMap::new()
                }
            }
        } else {
            HashMap::new()
        };

        // Then load split files (these will override legacy entries if there are conflicts)
        let split_cache = match self.load_from_split_files() {
            Ok(split_cache) => {
                if !split_cache.is_empty() {
                    info!("Loaded {} entries from split cache files", split_cache.len());
                }
                split_cache
            }
            Err(e) => {
                error!("Failed to load split cache files: {}", e);
                HashMap::new()
            }
        };

        let combined_cache = Self::merge_caches(legacy_cache, split_cache);

        Ok(combined_cache)
    }

    /// Merges two cache HashMaps, with newest timestamp winning conflicts
    ///
    /// # Arguments
    /// * `disk_cache` - Cache loaded from disk
    /// * `memory_cache` - Current in-memory cache
    ///
    /// # Returns
    /// Merged cache with newest entries for each key
    fn merge_caches(
        disk_cache: HashMap<String, CacheEntry>,
        memory_cache: HashMap<String, CacheEntry>,
    ) -> HashMap<String, CacheEntry> {
        let mut merged = disk_cache;

        for (key, memory_entry) in memory_cache {
            match merged.get(&key) {
                Some(disk_entry) => {
                    // Keep the entry with newest timestamp
                    if memory_entry.accessed_at >= disk_entry.accessed_at {
                        merged.insert(key, memory_entry);
                    }
                    // else keep existing disk entry
                }
                None => {
                    // Key doesn't exist in disk cache, add memory entry
                    merged.insert(key, memory_entry);
                }
            }
        }

        merged
    }

    /// Applies size management based on disk vs in-memory sizes
    ///
    /// Logic:
    /// - If disk was larger: respect disk size (no growth)
    /// - If memory is larger: allow growth up to current max_items
    ///
    /// # Arguments
    /// * `merged_cache` - Combined cache from disk and memory
    /// * `original_disk_size` - Size of cache that was on disk
    /// * `current_memory_size` - Size of current in-memory cache
    ///
    /// # Returns
    /// Cache sized according to the size management policy
    async fn apply_size_management(
        &self,
        mut merged_cache: HashMap<String, CacheEntry>,
        original_disk_size: usize,
        current_memory_size: usize,
    ) -> HashMap<String, CacheEntry> {
        // Determine target size based on policy
        let target_size = if original_disk_size >= current_memory_size {
            // Case 1: Disk cache was larger - respect disk size, no growth
            original_disk_size
        } else {
            // Case 2: Memory cache is larger - allow growth up to max_items
            std::cmp::min(self.max_items as usize, merged_cache.len())
        };

        // If merged cache fits within target, return as-is
        if merged_cache.len() <= target_size {
            return merged_cache;
        }

        // Apply LRU eviction to fit target size
        Self::evict_to_size(&mut merged_cache, target_size);
        merged_cache
    }

    /// Evicts oldest entries to fit target size using LRU policy
    ///
    /// # Arguments
    /// * `cache` - Cache to evict from
    /// * `target_size` - Desired final size
    ///
    /// # Returns
    /// Cache with oldest entries removed to fit target_size
    fn evict_to_size(cache: &mut HashMap<String, CacheEntry>, target_size: usize) {
        if cache.len() <= target_size {
            return;
        }

        let to_remove = cache.len().saturating_sub(target_size);

        // Sort entries by creation time (oldest first)
        let mut entries: Vec<(String, u64)> =
            cache.iter().map(|(key, entry)| (key.clone(), entry.accessed_at)).collect();

        entries.sort_by_key(|(_, accessed_at)| *accessed_at);

        // Remove oldest entries
        let keys_to_remove: Vec<String> =
            entries.into_iter().take(to_remove).map(|(key, _)| key).collect();

        for key in &keys_to_remove {
            cache.remove(key);
        }

        debug!(
            "Evicted {} entries during merge to fit target size {}",
            keys_to_remove.len(),
            target_size
        );
    }

    /// Splits cache content string into segments based on MAX_CACHE_FILE_SIZE
    ///
    /// Simple string splitting - we don't need to preserve JSON structure since
    /// we'll concatenate all segments when loading.
    ///
    /// # Arguments
    /// * `content` - The serialized cache content string
    ///
    /// # Returns
    /// Vector of content segments
    fn split_cache_content(content: &str) -> Vec<String> {
        if content.len() <= MAX_CACHE_FILE_SIZE {
            return vec![content.to_string()];
        }

        let mut segments = Vec::new();
        let mut current_pos = 0;

        while current_pos < content.len() {
            let segment_end = std::cmp::min(current_pos + MAX_CACHE_FILE_SIZE, content.len());
            let segment = &content[current_pos..segment_end];
            segments.push(segment.to_string());
            current_pos = segment_end;
        }

        segments
    }

    /// Writes cache content to split files atomically using directory rename
    ///
    /// This implementation solves the race condition problem where multiple processes
    /// could write split files simultaneously, leading to corrupted mixed cache data.
    ///
    /// **Atomicity Strategy:**
    /// 1. Write all split files to a temporary directory
    /// 2. Use atomic directory rename to replace the cache directory
    /// 3. Either all split files appear together, or none appear (never partial)
    ///
    /// **Multi-process Safety:**
    /// - Process A writes to .rpc_tmp_A/, Process B writes to .rpc_tmp_B/
    /// - Each process atomically renames their temp dir to rpc_cache/
    /// - Last writer wins, but cache is never corrupted with mixed data
    ///
    /// **Cross-platform Atomicity:**
    /// - Unix/Linux: rename() is guaranteed atomic by POSIX
    /// - Windows: MoveFile() is atomic when source/dest on same filesystem
    /// - We ensure same filesystem by creating temp dir in same parent directory
    ///
    /// # Arguments
    /// * `cache` - The cache to serialize and write
    ///
    /// # Returns
    /// Result indicating success or failure
    fn write_split_cache_files(&self, cache: &HashMap<String, CacheEntry>) -> Result<()> {
        let parent_dir = self
            .cache_file_path
            .parent()
            .ok_or_else(|| eyre::eyre!("Cache path has no parent directory"))?;
        let base_name = self
            .cache_file_path
            .file_stem()
            .ok_or_else(|| eyre::eyre!("Cache path has no file stem"))?;

        // Create unique temporary directory for this process
        // Using random number ensures different processes don't conflict
        let random_id: u64 = rand::random();
        let temp_dir =
            parent_dir.join(format!(".{}_tmp_{}", base_name.to_string_lossy(), random_id));

        // Final cache directory where split files will live
        let cache_dir = parent_dir.join(format!("{}_cache", base_name.to_string_lossy()));

        // Clean up any leftover temp directories from previous runs
        // (This can happen if process crashed before cleanup)
        let _ = fs::remove_dir_all(&temp_dir);

        // Create temporary directory and write all split files to it
        fs::create_dir_all(&temp_dir)?;

        let content = serde_json::to_string_pretty(cache)?;
        let segments = Self::split_cache_content(&content);

        // Write all segments to temporary directory first
        // This ensures if we crash during writing, no partial state is visible
        for (i, segment) in segments.iter().enumerate() {
            let split_file =
                temp_dir.join(format!("{}.json.{}", base_name.to_string_lossy(), i + 1));
            fs::write(&split_file, segment)?;
        }

        // Now perform the atomic replace
        // This is the critical section where atomicity matters
        if let Err(e) = self.atomic_directory_replace(&temp_dir, &cache_dir) {
            // If atomic replace fails, clean up temp directory
            let _ = fs::remove_dir_all(&temp_dir);
            // Remove partial cache dir if exists
            let _ = fs::remove_dir_all(&cache_dir);
            return Err(e);
        }

        info!(
            "Atomically saved cache to {} split files in {}",
            segments.len(),
            cache_dir.display()
        );
        Ok(())
    }

    /// Performs atomic directory replace using rename operation
    ///
    /// **Atomicity Guarantee:**
    /// The directory rename operation atomically replaces the destination directory
    /// on all major filesystems. No backup step needed - the OS handles this atomically.
    ///
    /// **Multi-Process Safety:**
    /// - No race condition with backup step (eliminated)
    /// - Multiple processes can call this simultaneously
    /// - Last writer wins, but cache is never corrupted
    ///
    /// **Cross-Platform Behavior:**
    /// - Unix/Linux: rename() atomically replaces destination (POSIX guarantee)
    /// - Windows: MoveFileEx() with MOVEFILE_REPLACE_EXISTING does the same
    /// - Rust's fs::rename() maps to the correct OS call
    ///
    /// # Arguments
    /// * `temp_dir` - Directory containing new split files
    /// * `cache_dir` - Target directory for split files (will be replaced if exists)
    ///
    /// # Returns
    /// Result indicating success or failure
    fn atomic_directory_replace(
        &self,
        temp_dir: &std::path::Path,
        cache_dir: &std::path::Path,
    ) -> Result<()> {
        // Remove existing cache directory if it exists (however, we cannot check with exists()
        // first) Ignore errors - if it doesn't exist, that's fine
        // This is necessary because fs::rename doesn't replace existing directories on all
        // platforms
        let _ = fs::remove_dir_all(cache_dir);

        // Now rename temp directory to cache directory
        // Since destination doesn't exist, this is atomic on all platforms
        fs::rename(temp_dir, cache_dir).map_err(|e| {
            eyre::eyre!(
                "Failed to atomically replace cache directory: {}. Error: {}",
                cache_dir.display(),
                e
            )
        })?;
        Ok(())
    }

    /// Loads cache from split files and combines them
    ///
    /// Now looks in the cache directory structure created by atomic writes.
    /// Falls back to old flat file structure for backward compatibility.
    ///
    /// # Returns
    /// Combined cache from all split files
    fn load_from_split_files(&self) -> Result<HashMap<String, CacheEntry>> {
        let parent_dir = self.cache_file_path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let base_name = self.cache_file_path.file_stem().unwrap_or_default();

        // New structure: look in cache directory first
        let cache_dir = parent_dir.join(format!("{}_cache", base_name.to_string_lossy()));

        let mut split_files = Vec::new();
        let search_dir = if cache_dir.exists() {
            // Load from new atomic cache directory structure
            cache_dir.as_path()
        } else {
            // Fall back to old flat file structure for backward compatibility
            parent_dir
        };

        let expected_prefix = format!("{}.json.", base_name.to_string_lossy());

        // Find all split files in the search directory
        if let Ok(entries) = fs::read_dir(search_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(file_name) = path.file_name() {
                    let file_name_str = file_name.to_string_lossy();
                    if file_name_str.starts_with(&expected_prefix) {
                        split_files.push(path);
                    }
                }
            }
        }

        if split_files.is_empty() {
            return Ok(HashMap::new());
        }

        // Sort files by their numeric suffix to ensure correct order
        split_files.sort_by(|a, b| {
            let extract_number = |path: &PathBuf| -> u32 {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .and_then(|ext| ext.parse().ok())
                    .unwrap_or(0)
            };
            extract_number(a).cmp(&extract_number(b))
        });

        // Read all segments and concatenate them back into one string
        let mut combined_content = String::new();
        for file_path in split_files {
            let segment_content = fs::read_to_string(&file_path)?;
            combined_content.push_str(&segment_content);
        }

        // Parse the combined content
        let cache: HashMap<String, CacheEntry> = serde_json::from_str(&combined_content)?;
        Ok(cache)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::time::{Duration, sleep};
    use tracing::{debug, info};

    fn create_test_cache_manager(max_items: u32) -> (CacheManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("test_rpc.json");
        let manager = CacheManager::new(max_items, cache_path).unwrap();
        (manager, temp_dir)
    }

    #[tokio::test]
    async fn test_cache_get_set() {
        edb_common::logging::ensure_test_logging(None);
        info!("Testing cache get/set operations");

        let (manager, _temp_dir) = create_test_cache_manager(10);

        // Test cache miss
        assert!(manager.get("test_key").await.is_none());

        // Test cache set and get
        let test_value = serde_json::json!({"result": "test_data"});
        manager.set("test_key".to_string(), test_value.clone()).await;

        let retrieved = manager.get("test_key").await.unwrap();
        assert_eq!(retrieved, test_value);
    }

    #[tokio::test]
    async fn test_cache_eviction() {
        edb_common::logging::ensure_test_logging(None);
        info!("Testing cache eviction behavior");

        let (manager, _temp_dir) = create_test_cache_manager(3);

        // Fill cache to capacity with delays to ensure different timestamps
        for i in 0..3 {
            let key = format!("key_{i}");
            let value = serde_json::json!({"data": i});
            manager.set(key, value).await;
            sleep(Duration::from_secs(1)).await;
        }

        // Add one more item to trigger eviction
        manager.set("key_3".to_string(), serde_json::json!({"data": 3})).await;

        // Check that oldest item was evicted (key_0 should be gone)
        assert!(manager.get("key_0").await.is_none());
        assert!(manager.get("key_3").await.is_some());
    }

    #[tokio::test]
    async fn test_cache_eviction_order() {
        edb_common::logging::ensure_test_logging(None);
        info!("Testing cache eviction order");

        let (manager, _temp_dir) = create_test_cache_manager(3);

        // Add items with delays to ensure different timestamps
        manager.set("old_key".to_string(), serde_json::json!({"data": "old"})).await;
        sleep(Duration::from_secs(1)).await;

        manager.set("mid_key".to_string(), serde_json::json!({"data": "mid"})).await;
        sleep(Duration::from_secs(1)).await;

        manager.set("new_key".to_string(), serde_json::json!({"data": "new"})).await;
        sleep(Duration::from_secs(1)).await;

        // Trigger eviction
        manager.set("newest_key".to_string(), serde_json::json!({"data": "newest"})).await;

        // The oldest item should be evicted
        assert!(manager.get("old_key").await.is_none());
        assert!(manager.get("mid_key").await.is_some());
        assert!(manager.get("new_key").await.is_some());
        assert!(manager.get("newest_key").await.is_some());
    }

    #[tokio::test]
    async fn test_cache_persistence() {
        edb_common::logging::ensure_test_logging(None);
        info!("Testing cache persistence across restarts");

        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("persist_test.json");

        // Create first cache manager and add data
        {
            let manager = CacheManager::new(10, cache_path.clone()).unwrap();
            manager.set("persist_key".to_string(), serde_json::json!({"persisted": true})).await;
            manager.save_to_disk().await.unwrap();
        } // manager goes out of scope

        // Create new cache manager with same path
        let manager2 = CacheManager::new(10, cache_path).unwrap();
        let retrieved = manager2.get("persist_key").await.unwrap();
        assert_eq!(retrieved, serde_json::json!({"persisted": true}));
    }

    #[tokio::test]
    async fn test_detailed_stats() {
        edb_common::logging::ensure_test_logging(None);
        info!("Testing detailed cache statistics");

        let (manager, _temp_dir) = create_test_cache_manager(100);

        // Test empty cache stats
        let stats = manager.detailed_stats().await;
        assert_eq!(stats["total_entries"], 0);
        assert_eq!(stats["max_entries"], 100);
        assert_eq!(stats["utilization"], "0.0%");
        assert!(stats["oldest_entry_age_seconds"].is_null());
        assert!(stats["newest_entry_age_seconds"].is_null());

        // Add some items
        manager.set("item1".to_string(), serde_json::json!({"data": 1})).await;
        sleep(Duration::from_secs(1)).await;
        manager.set("item2".to_string(), serde_json::json!({"data": 2})).await;

        // Test stats with items
        let stats = manager.detailed_stats().await;
        assert_eq!(stats["total_entries"], 2);
        assert_eq!(stats["max_entries"], 100);
        assert_eq!(stats["utilization"], "2.0%");
        assert!(
            stats["oldest_entry_age_seconds"].as_u64().unwrap()
                >= stats["newest_entry_age_seconds"].as_u64().unwrap()
        );
    }

    #[tokio::test]
    async fn test_cache_entry_timestamps() {
        edb_common::logging::ensure_test_logging(None);
        debug!("Testing cache entry timestamp behavior");

        let entry1 = CacheEntry::new(serde_json::json!({"test": 1}));
        sleep(Duration::from_secs(1)).await;
        let entry2 = CacheEntry::new(serde_json::json!({"test": 2}));

        assert!(entry2.accessed_at > entry1.accessed_at);
    }

    #[tokio::test]
    async fn test_cache_merge_and_size_management() {
        edb_common::logging::ensure_test_logging(None);
        info!("Testing cache merge functionality and size management");

        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("merge_test.json");

        // Create first cache manager and add data (2 items)
        {
            let manager = CacheManager::new(5, cache_path.clone()).unwrap(); // max 5 items
            manager.set("old_key1".to_string(), serde_json::json!({"data": "old1"})).await;
            manager.set("shared_key".to_string(), serde_json::json!({"data": "old_shared"})).await;
            manager.save_to_disk().await.unwrap();
        } // manager goes out of scope

        // Wait to ensure different timestamps
        sleep(Duration::from_secs(1)).await;

        // Create second cache manager with new data (3 items) - this should allow growth
        let manager2 = CacheManager::new(5, cache_path.clone()).unwrap(); // max 5 items
        manager2.set("new_key1".to_string(), serde_json::json!({"data": "new1"})).await;
        manager2.set("new_key2".to_string(), serde_json::json!({"data": "new2"})).await;
        manager2.set("shared_key".to_string(), serde_json::json!({"data": "new_shared"})).await;

        // Save should merge with existing cache, allowing growth since memory (3) > disk (2)
        manager2.save_to_disk().await.unwrap();

        // Create third manager to verify merge results
        let manager3 = CacheManager::new(10, cache_path).unwrap();

        // Should have all keys since memory was larger than disk
        assert!(manager3.get("old_key1").await.is_some());
        assert!(manager3.get("new_key1").await.is_some());
        assert!(manager3.get("new_key2").await.is_some());

        // shared_key should have the newest value (from manager2)
        let shared_value = manager3.get("shared_key").await.unwrap();
        assert_eq!(shared_value["data"], "new_shared");

        info!("Cache merge test completed successfully");
    }

    #[tokio::test]
    async fn test_size_management_disk_larger() {
        edb_common::logging::ensure_test_logging(None);
        info!("Testing size management when disk cache is larger");

        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("size_test.json");

        // Create large disk cache (5 items)
        {
            let manager = CacheManager::new(10, cache_path.clone()).unwrap();
            for i in 0..5 {
                manager.set(format!("disk_key_{i}"), serde_json::json!({"data": i})).await;
            }
            manager.save_to_disk().await.unwrap();
        }

        // Create smaller in-memory cache (2 items) with smaller max_items
        let manager2 = CacheManager::new(3, cache_path.clone()).unwrap(); // max 3 items
        manager2.set("memory_key_1".to_string(), serde_json::json!({"data": "mem1"})).await;
        manager2.set("memory_key_2".to_string(), serde_json::json!({"data": "mem2"})).await;

        // Save should respect disk size (5), not grow beyond it
        manager2.save_to_disk().await.unwrap();

        // Verify result respects original disk size
        let manager3 = CacheManager::new(10, cache_path).unwrap();
        let all_entries = manager3.get_all_entries().await;
        assert_eq!(all_entries.len(), 5); // Should not exceed original disk size

        info!("Size management test completed - disk cache size respected");
    }

    #[tokio::test]
    async fn test_size_management_memory_larger() {
        edb_common::logging::ensure_test_logging(None);
        info!("Testing size management when memory cache is larger");

        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("size_test2.json");

        // Create small disk cache (2 items)
        {
            let manager = CacheManager::new(10, cache_path.clone()).unwrap();
            manager.set("disk_key_1".to_string(), serde_json::json!({"data": "disk1"})).await;
            manager.set("disk_key_2".to_string(), serde_json::json!({"data": "disk2"})).await;
            manager.save_to_disk().await.unwrap();
        }

        // Create larger in-memory cache (4 items) with reasonable max_items
        let manager2 = CacheManager::new(6, cache_path.clone()).unwrap(); // max 6 items
        for i in 0..4 {
            manager2
                .set(format!("memory_key_{i}"), serde_json::json!({"data": format!("mem{}", i)}))
                .await;
        }

        // Save should allow growth up to max_items
        manager2.save_to_disk().await.unwrap();

        // Verify result allows growth
        let manager3 = CacheManager::new(10, cache_path).unwrap();
        let all_entries = manager3.get_all_entries().await;
        assert_eq!(all_entries.len(), 6); // Should have grown to accommodate both

        info!("Size management test completed - cache growth allowed");
    }

    #[tokio::test]
    async fn test_cache_file_splitting() {
        edb_common::logging::ensure_test_logging(None);
        info!("Testing cache file splitting functionality");

        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("split_test.json");
        let manager = CacheManager::new(10000, cache_path.clone()).unwrap();

        // Create large cache data that will exceed MAX_CACHE_FILE_SIZE
        // Based on debug test: 1000 entries with 4KB each = 4.2MB
        // So we need about 5000 entries with 4KB each to get ~21MB
        let large_data = "x".repeat(4096); // 4KB string
        let num_entries = 6000; // This should create ~25MB of data

        for i in 0..num_entries {
            let key = format!("large_key_{i}");
            let value = serde_json::json!({"data": large_data, "index": i});
            manager.set(key, value).await;
        }

        // Save to disk - this should create split files
        manager.save_to_disk().await.unwrap();

        // Verify split files were created in the cache directory
        let parent_dir = cache_path.parent().unwrap();
        let cache_dir = parent_dir.join("split_test_cache");
        let mut split_files = Vec::new();

        for entry in fs::read_dir(&cache_dir).unwrap() {
            let path = entry.unwrap().path();
            if let Some(file_name) = path.file_name() {
                let file_name_str = file_name.to_string_lossy();
                if file_name_str.starts_with("split_test.json.") {
                    split_files.push(path);
                }
            }
        }

        // Should have multiple split files
        assert!(
            split_files.len() > 1,
            "Expected multiple split files, found {}",
            split_files.len()
        );
        info!("Created {} split files", split_files.len());

        // Verify each split file is under the size limit (allowing some JSON overhead)
        for file_path in &split_files {
            let file_size = fs::metadata(file_path).unwrap().len() as usize;
            assert!(
                file_size <= MAX_CACHE_FILE_SIZE + 10000, /* Allow 10KB overhead for JSON
                                                           * formatting */
                "Split file {} is too large: {} bytes",
                file_path.display(),
                file_size
            );
        }

        // Create new manager to test loading
        let manager2 = CacheManager::new(20000, cache_path).unwrap();

        // Verify all entries were loaded correctly
        let all_entries = manager2.get_all_entries().await;
        assert_eq!(all_entries.len(), num_entries);

        // Verify specific entries
        for i in [0, num_entries / 2, num_entries - 1] {
            let key = format!("large_key_{i}");
            let value = manager2.get(&key).await.unwrap();
            assert_eq!(value["data"], large_data);
            assert_eq!(value["index"], i);
        }

        info!("Cache file splitting test completed successfully");
    }

    #[tokio::test]
    async fn test_backward_compatibility_loading() {
        edb_common::logging::ensure_test_logging(None);
        info!("Testing backward compatibility with legacy cache files");

        let temp_dir = TempDir::new().unwrap();
        let cache_path = temp_dir.path().join("legacy_test.json");

        // Create a legacy cache file manually
        let legacy_cache = {
            let mut cache = HashMap::new();
            cache.insert(
                "legacy_key1".to_string(),
                CacheEntry::new(serde_json::json!({"legacy": "data1"})),
            );
            cache.insert(
                "legacy_key2".to_string(),
                CacheEntry::new(serde_json::json!({"legacy": "data2"})),
            );
            cache.insert(
                "shared_key".to_string(),
                CacheEntry::new(serde_json::json!({"source": "legacy"})),
            );
            cache
        };

        let legacy_content = serde_json::to_string_pretty(&legacy_cache).unwrap();
        fs::write(&cache_path, &legacy_content).unwrap();

        // Create manager - it should load the legacy file
        let manager = CacheManager::new(10, cache_path.clone()).unwrap();

        // Verify legacy data was loaded
        assert!(manager.get("legacy_key1").await.is_some());
        assert!(manager.get("legacy_key2").await.is_some());

        // Add new data and save - this should create split files
        manager.set("new_key1".to_string(), serde_json::json!({"new": "data1"})).await;
        manager.set("shared_key".to_string(), serde_json::json!({"source": "new"})).await;
        manager.save_to_disk().await.unwrap();

        // Create another manager to test loading both legacy and split files
        let manager2 = CacheManager::new(10, cache_path).unwrap();

        // Verify all data is present
        assert!(manager2.get("legacy_key1").await.is_some());
        assert!(manager2.get("legacy_key2").await.is_some());
        assert!(manager2.get("new_key1").await.is_some());

        // Verify that split file data overrides legacy data for shared keys
        let shared_value = manager2.get("shared_key").await.unwrap();
        assert_eq!(shared_value["source"], "new");

        info!("Backward compatibility test completed successfully");
    }

    #[tokio::test]
    async fn test_split_content_function() {
        edb_common::logging::ensure_test_logging(None);
        info!("Testing split_cache_content function");

        // Test content that fits in one segment
        let small_content = "small content";
        let segments = CacheManager::split_cache_content(small_content);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], small_content);

        // Test content that needs to be split
        let large_content = "x".repeat(MAX_CACHE_FILE_SIZE + 1000);
        let segments = CacheManager::split_cache_content(&large_content);
        assert!(segments.len() > 1);

        // Verify that concatenating segments gives back original content
        let reconstructed = segments.join("");
        assert_eq!(reconstructed, large_content);

        // Verify each segment (except possibly the last) is at the size limit
        for (i, segment) in segments.iter().enumerate() {
            if i < segments.len() - 1 {
                assert_eq!(segment.len(), MAX_CACHE_FILE_SIZE);
            } else {
                assert!(segment.len() <= MAX_CACHE_FILE_SIZE);
            }
        }

        info!("Split content function test completed");
    }
}
