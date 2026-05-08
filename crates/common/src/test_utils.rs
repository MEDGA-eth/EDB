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

//! Test utilities for configuring the EDB test environment.
//!
//! Provides helpers for setting up cache directories and environment variables
//! needed for testing, with support for both shared and isolated test environments.

use std::{env, path::PathBuf};

use tracing::info;

/// Get the testdata cache directory root path
pub fn get_testdata_cache_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent() // crates/
        .and_then(|p| p.parent()) // workspace root
        .expect("Failed to find workspace root")
        .join("testdata")
        .join("cache")
}

/// Creates a temporary cache directory and copies the testdata cache into it.
///
/// This is a utility function for test isolation, ensuring each test can have its own
/// independent cache directory without polluting the shared testdata cache.
///
/// # Returns
///
/// Returns the path to the newly created temporary cache directory.
///
/// # Behavior
///
/// - Creates a temporary directory with a random suffix in the system temp location
/// - Copies all contents from the testdata cache to the temporary directory
/// - If the testdata cache doesn't exist, creates an empty temporary directory
///
/// # Examples
///
/// ```
/// use edb_common::test_utils::create_temp_cache_dir;
///
/// let temp_cache = create_temp_cache_dir();
/// // Use temp_cache for isolated testing
/// ```
pub fn create_temp_cache_dir() -> PathBuf {
    let random_suffix: u32 = rand::random();
    let temp_dir = env::temp_dir().join(format!("edb-test-cache-{random_suffix:08x}"));
    let testdata_cache = get_testdata_cache_root();

    if testdata_cache.exists() {
        copy_dir_all(&testdata_cache, &temp_dir)
            .expect("Failed to copy testdata cache to temp directory");
    } else {
        std::fs::create_dir_all(&temp_dir).expect("Failed to create temp cache directory");
    }

    info!("Created temporary test cache directory: {}", temp_dir.display());
    temp_dir
}

/// Global state to track whether the test environment has been initialized.
static TEST_ENV_INITIALIZED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Sets up the test environment with appropriate cache directory and configuration.
///
/// This function configures the [`EDB_CACHE_DIR`](crate::env::EDB_CACHE_DIR) and
/// [`EDB_ETHERSCAN_CACHE_TTL`](crate::env::EDB_ETHERSCAN_CACHE_TTL) environment
/// variables for testing purposes.
///
/// **Important:** This function should only be called once per process. Subsequent calls
/// with the same `use_temp` value will be ignored. Calls with a different `use_temp`
/// value will panic to prevent inconsistent test environments.
///
/// # Arguments
///
/// * `use_temp` - If `true`, copies the testdata cache to a temporary directory for test isolation.
///   This is useful for tests that may modify cache contents and need to avoid polluting the shared
///   testdata cache. If `false`, uses the shared testdata cache directory directly (read-only usage
///   recommended).
///
/// # Behavior
///
/// - Sets `EDB_CACHE_DIR` to either a temporary copy or the shared testdata cache
/// - Sets `EDB_ETHERSCAN_CACHE_TTL` to `u32::MAX` (effectively infinite) to avoid cache expiration
///   during tests
/// - Creates the cache directory if it doesn't exist
/// - Ensures setup only happens once per process
///
/// # Panics
///
/// Panics if called multiple times with different `use_temp` values in the same process.
///
/// # Temporary Directory
///
/// When `use_temp=true`, the temporary directory is created in the system's temp
/// location with a random suffix for isolation. The testdata cache is recursively
/// copied to this temporary location. The directory will be automatically cleaned
/// up by the OS.
///
/// # Examples
///
/// ```rust, no_run
/// use edb_common::test_utils::setup_test_environment;
///
/// // Use shared testdata cache (read-only tests)
/// setup_test_environment(false);
///
/// // Use isolated temporary cache (tests that modify cache)
/// setup_test_environment(true);
/// ```
pub fn setup_test_environment(use_temp: bool) {
    // Use get_or_init for atomic, thread-safe initialization
    let previous_use_temp = TEST_ENV_INITIALIZED.get_or_init(|| {
        // This closure runs exactly once, even with concurrent calls
        let cache_dir = if use_temp {
            create_temp_cache_dir()
        } else {
            let cache_dir = get_testdata_cache_root();

            // Create the directory if it doesn't exist
            if !cache_dir.exists() {
                std::fs::create_dir_all(&cache_dir).expect("Failed to create cache directory");
            }

            info!("Using shared test cache directory: {}", cache_dir.display());
            cache_dir
        };

        // SAFETY: edition 2024 marks env::set_var as unsafe (RFC 3445); this runs once
        // during test setup, before threads accessing the env have been spawned.
        unsafe {
            env::set_var(
                crate::env::EDB_CACHE_DIR,
                cache_dir.to_str().expect("Invalid cache path"),
            );
            env::set_var(crate::env::EDB_ETHERSCAN_CACHE_TTL, format!("{}", u32::MAX));
        }

        // Return the use_temp value to store in OnceLock
        use_temp
    });

    // Check if called with different parameters
    if *previous_use_temp != use_temp {
        panic!(
            "Test environment already initialized with use_temp={previous_use_temp}, but called again with use_temp={use_temp}"
        );
    }

    // If we reach here, either we just initialized or it was already initialized with same params
    if TEST_ENV_INITIALIZED.get().is_some() && *previous_use_temp == use_temp {
        info!("Test environment already initialized, skipping setup");
    }
}

/// Helper function to recursively copy directories.
///
/// This is a simple implementation for copying directory trees, used for test isolation.
fn copy_dir_all(
    src: impl AsRef<std::path::Path>,
    dst: impl AsRef<std::path::Path>,
) -> std::io::Result<()> {
    std::fs::create_dir_all(&dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
