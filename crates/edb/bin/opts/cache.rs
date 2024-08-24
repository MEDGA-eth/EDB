use std::path::PathBuf;

use clap::Parser;
use edb_utils::cache::EDBCachePath;
use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize, Parser)]
pub struct CacheOpts {
    /// The root directory for the cache. If not provided, the default is `~/.edb/cache`.
    #[clap(long, env = "EDB_CACHE_ROOT", conflicts_with = "no_cache")]
    pub cache_root: Option<PathBuf>,

    /// Do not use the cache.
    #[clap(long, conflicts_with = "cache_root")]
    pub no_cache: bool,
}

impl CacheOpts {
    pub fn cache_path(&self) -> Option<EDBCachePath> {
        if self.no_cache {
            None
        } else {
            Some(EDBCachePath::new(self.cache_root.clone()))
        }
    }
}
