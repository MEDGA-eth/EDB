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

//! On-chain contract compilation utilities.
//!
//! This module provides utilities for compiling smart contracts from verified
//! source code obtained from blockchain explorers like Etherscan. It handles
//! the complete compilation workflow including source retrieval, compiler
//! configuration, and artifact generation.
//!
//! # Core Features
//!
//! - **Source Retrieval**: Fetch verified source code from Etherscan
//! - **Compiler Configuration**: Set up Solidity compiler with proper settings
//! - **Multi-file Compilation**: Handle complex projects with dependencies
//! - **Library Support**: Manage library dependencies and linking
//! - **Caching**: Cache compilation results for performance
//!
//! # Workflow
//!
//! 1. Retrieve contract metadata and source code from Etherscan
//! 2. Configure Solidity compiler with matching settings
//! 3. Compile the contract with all dependencies
//! 4. Generate artifact with metadata and compilation output

use std::{collections::HashSet, env, path::PathBuf, sync::Mutex, thread, time::Duration};

use alloy_primitives::Address;
use edb_common::{Cache, EdbCache};
use eyre::Result;
use foundry_block_explorers::{Client, contract::Metadata, errors::EtherscanError};
use foundry_compilers::{
    artifacts::{Libraries, SolcInput, Source, Sources, output_selection::OutputSelection},
    solc::{Solc, SolcLanguage},
};
use itertools::Itertools;
use once_cell::sync::Lazy;
use semver::Version;
use tracing::{debug, error, info, trace};

use crate::{Artifact, etherscan_rate_limit_guard};

/// Onchain compiler.
#[derive(Debug, Clone)]
pub struct OnchainCompiler {
    /// Cache for the compiled contracts.
    pub cache: Option<EdbCache<Option<Artifact>>>,
}

impl OnchainCompiler {
    /// New onchain compiler.
    pub fn new(cache_root: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            // None for no expiry
            cache: EdbCache::new(cache_root, None)?,
        })
    }

    /// Compile the contract at the given address.
    /// Returns `Some`` if the contract is successfully compiled.
    /// Returns `None` if the contract is not verified, is a Vyper contract, or it is a Solidity
    /// 0.4.x contract which does not support --stand-json option.
    pub async fn compile(&self, etherscan: &Client, addr: Address) -> Result<Option<Artifact>> {
        // Get the cache_root. If not provided, use the default cache directory.
        if let Some(output) = self.cache.load_cache(addr.to_string()) {
            Ok(output)
        } else {
            if env::var(edb_common::env::EDB_TEST_ETHERSCAN_MODE)
                .is_ok_and(|ref v| v == "cache-only")
            {
                debug!(address=?addr, "skipping on-chain compilation in cache-only mode");
                return Ok(None);
            }

            let mut meta =
                match etherscan_rate_limit_guard!(etherscan.contract_source_code(addr).await) {
                    Ok(meta) => meta,
                    Err(EtherscanError::ContractCodeNotVerified(_)) => {
                        // We do not cache the fact that the contract is not verified, since it may
                        // be verified later.
                        info!(address=?addr, "contract is not verified");
                        return Ok(None);
                    }
                    Err(e) => {
                        // We do not cache since it could be caused by network issues.
                        error!(address=?addr, "failed to query Etherscan: {e}");
                        return Ok(None);
                    }
                };
            eyre::ensure!(meta.items.len() == 1, "contract not found or ill-formed");
            let meta = meta.items.remove(0);

            if meta.is_vyper() {
                // We can safely cache since we cannot deal with vyper
                let none = None;
                self.cache.save_cache(addr.to_string(), &none)?;
                return Ok(None);
            }

            let input = get_compilation_input_from_metadata(&meta, addr)?;

            // prepare the compiler
            let version = meta.compiler_version()?;
            let compiler = find_or_install_solc(&version)?;
            trace!(addr=?addr, compiler=?compiler, "using compiler");

            // compile the source code
            let output = match compiler.compile_exact(&input) {
                Ok(output) => Some(Artifact { meta, input, output }),
                Err(_) if version.major == 0 && version.minor == 4 => None,
                Err(e) => {
                    return Err(eyre::eyre!("failed to compile contract: {}", e));
                }
            };

            self.cache.save_cache(addr.to_string(), &output)?;
            Ok(output)
        }
    }
}

/// Prepare the input for solc using metadate downloaded from Etherscan.
pub fn get_compilation_input_from_metadata(meta: &Metadata, addr: Address) -> Result<SolcInput> {
    let mut settings = meta.settings()?;

    // Enforce compiler output all possible outputs
    settings.output_selection = OutputSelection::complete_output_selection();
    trace!(addr=?addr, settings=?settings, "using settings");

    // Prepare the sources
    let sources: Sources =
        meta.sources().into_iter().map(|(k, v)| (k.into(), Source::new(v.content))).collect();

    // Check library
    if !meta.library.is_empty() {
        let prefix = if sources.keys().unique().count() == 1 {
            sources.keys().next().unwrap().to_string_lossy().to_string()
        } else {
            // When multiple source files are present, the library string should include the path.
            String::new()
        };

        let libs = meta
            .library
            .split(';')
            .filter_map(|lib| {
                debug!(lib=?lib, addr=?addr, "parsing library");
                let mut parts = lib.split(':');

                let file =
                    if parts.clone().count() == 2 { prefix.as_str() } else { parts.next()? };
                let name = parts.next()?;
                let addr = parts.next()?;

                if addr.starts_with("0x") {
                    Some(format!("{file}:{name}:{addr}"))
                } else {
                    Some(format!("{file}:{name}:0x{addr}"))
                }
            })
            .collect::<Vec<_>>();

        settings.libraries = Libraries::parse(&libs)?;
    }

    let input = SolcInput::new(SolcLanguage::Solidity, sources, settings);

    Ok(input)
}

/// Global lock map for solc installation (per-version locking for multi-threaded safety).
static INSTALL_LOCKS: Lazy<Mutex<HashSet<Version>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// Find or install the specified solc version (lock when concurrent).
pub fn find_or_install_solc(version: &Version) -> Result<Solc> {
    // Acquire global lock to check/insert version lock
    let mut locks = INSTALL_LOCKS.lock().unwrap();

    // Wait if this version is being installed
    while locks.contains(version) {
        drop(locks);
        thread::sleep(Duration::from_millis(100));
        locks = INSTALL_LOCKS.lock().unwrap();
    }

    // Mark this version as being installed
    locks.insert(version.clone());
    drop(locks);

    // Do the actual find/install
    let result = Solc::find_or_install(version);

    // Remove the lock
    INSTALL_LOCKS.lock().unwrap().remove(version);

    result.map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, time::Duration};

    use alloy_chains::Chain;
    use serial_test::serial;

    use crate::utils::next_etherscan_api_key;

    use super::*;

    async fn run_compile(chain_id: Chain, addr: &str) -> eyre::Result<Option<Artifact>> {
        let etherscan_cache_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/cache/etherscan")
            .join(chain_id.to_string());
        let etherscan = Client::builder()
            .with_api_key(next_etherscan_api_key())
            .with_cache(Some(etherscan_cache_root), Duration::from_secs(24 * 60 * 60)) // 24 hours
            .chain(chain_id)?
            .build()?;

        // We disable the cache for testing.
        let compiler = OnchainCompiler::new(None)?;
        compiler.compile(&etherscan, Address::from_str(addr)?).await
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_tailing_slash() {
        run_compile(Chain::mainnet(), "0x22F9dCF4647084d6C31b2765F6910cd85C178C18").await.unwrap();
    }
}
