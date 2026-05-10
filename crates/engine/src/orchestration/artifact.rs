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

//! Orchestration module that handles downloading verified source code,
//! instrumenting it, and generating snapshots for time travel debugging.
use std::{collections::HashMap, env, fs, time::Duration};

use alloy_primitives::Address;
use edb_common::{CachePath, DEFAULT_ETHERSCAN_CACHE_TTL, EdbCachePath};
use eyre::{Result, bail};
use foundry_block_explorers::Client;
use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use semver::Version;
use tracing::{debug, error, info, warn};

use crate::{
    Artifact, EngineConfig, OnchainCompiler, TraceReplayResult, analysis::AnalysisResult,
    dump_source_for_debugging, find_or_install_solc, format_compiler_errors, instrument,
};

/// Download and compile verified source code for each contract
pub async fn download_verified_source_code(
    config: &EngineConfig,
    replay_result: &TraceReplayResult,
    chain_id: u64,
) -> Result<HashMap<Address, Artifact>> {
    info!("Downloading verified source code for touched contracts");

    let compiler_cache_root = EdbCachePath::new(env::var(edb_common::env::EDB_CACHE_DIR).ok())
        .compiler_chain_cache_dir(chain_id);
    let compiler = OnchainCompiler::new(compiler_cache_root)?;

    let etherscan_cache_root = EdbCachePath::new(env::var(edb_common::env::EDB_CACHE_DIR).ok())
        .etherscan_chain_cache_dir(chain_id);

    let addresses: Vec<_> = replay_result.visited_addresses.keys().copied().collect();
    let total_contracts = addresses.len();

    let console_bar = std::sync::Arc::new(ProgressBar::new(total_contracts as u64));
    console_bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} 📜 Downloading & compiling contracts [{bar:40.cyan/blue}] {pos:>3}/{len:3} 🔧 {msg}"
            )?
            .progress_chars("🟩🟦⬜")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
        );

    let cache_ttl = env::var(edb_common::env::EDB_ETHERSCAN_CACHE_TTL)
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_ETHERSCAN_CACHE_TTL);

    // Create all download futures
    let download_futures = addresses.iter().map(|address| {
        let pb = console_bar.clone();
        let api_key = config.get_etherscan_api_key();
        let etherscan_cache_root = etherscan_cache_root.clone();
        let compiler = compiler.clone();

        async move {
            let short_addr = &address.to_string()[2..10]; // Skip 0x, take 8 chars
            pb.set_message(format!("Downloading: 0x{short_addr}..."));

            let etherscan = Client::builder()
                .with_api_key(api_key)
                .with_cache(etherscan_cache_root, Duration::from_secs(cache_ttl))
                .chain(chain_id.into())?
                .build()?;

            let result = match compiler.compile(&etherscan, *address).await {
                Ok(Some(artifact)) => {
                    pb.set_message(format!("✅ 0x{short_addr}... compiled"));
                    Some(artifact)
                }
                Ok(None) => {
                    let sourcify_result =
                        crate::utils::fetch_artifact_from_sourcify(chain_id, *address).await;
                    match sourcify_result {
                        Ok(Some(artifact)) => {
                            pb.set_message(format!("✅ 0x{short_addr}... sourcify"));
                            Some(artifact)
                        }
                        Ok(None) => {
                            pb.set_message(format!("⚠️  0x{short_addr}... no source"));
                            debug!("No source code available for contract {}", address);
                            None
                        }
                        Err(e) => {
                            pb.set_message(format!("⚠️  0x{short_addr}... no source"));
                            warn!("Sourcify fetch failed for {}: {:?}", address, e);
                            None
                        }
                    }
                }
                Err(e) => {
                    pb.set_message(format!("❌ 0x{short_addr}... failed"));
                    warn!("Failed to compile contract {}: {:?}", address, e);
                    None
                }
            };

            pb.inc(1);
            Ok::<(Address, Option<Artifact>), eyre::Error>((*address, result))
        }
    });

    // Wait for all downloads to complete
    let results = join_all(download_futures).await;

    // Process results into HashMap
    let mut artifacts = HashMap::new();
    for result in results {
        if let Ok((address, Some(artifact))) = result {
            artifacts.insert(address, artifact);
        } else if let Err(e) = result {
            error!("Error during source code download: {:?}", e);
        }
    }

    console_bar.finish_with_message(format!(
        "✨ Done! Compiled {} out of {} contracts",
        artifacts.len(),
        total_contracts
    ));

    Ok(artifacts)
}

/// Instrument and recompile the source code
pub fn instrument_and_recompile_source_code(
    artifacts: &HashMap<Address, Artifact>,
    analysis_result: &HashMap<Address, AnalysisResult>,
) -> Result<HashMap<Address, Artifact>> {
    info!("Instrumenting source code based on analysis results");

    let progress_bar = std::sync::Arc::new(ProgressBar::new(artifacts.len() as u64));
    progress_bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} 🔧 Instrumenting & recompiling contracts [{bar:40.cyan/blue}] {pos:>3}/{len:3} {msg}"
            )?
            .progress_chars("🟩🟦⬜")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
        );

    // Parallel process all contracts
    let results: Vec<_> = artifacts
            .par_iter()
            .map(|(address, artifact)| {
                let pb = progress_bar.clone();
                let short_addr = &address.to_string()[2..10]; // Skip 0x, take 8 chars
                pb.set_message(format!("Recompiling: 0x{short_addr}..."));

                let result = (|| -> Result<Artifact> {
                    let compiler_version =
                        Version::parse(artifact.compiler_version().trim_start_matches('v'))?;

                    let analysis = analysis_result
                        .get(address)
                        .ok_or_else(|| eyre::eyre!("No analysis result found for address {}", address))?;

                    let input = instrument(&compiler_version, &artifact.input, analysis)?;
                    let meta = artifact.meta.clone();

                    // prepare the compiler
                    let version = meta.compiler_version()?;
                    let compiler = find_or_install_solc(&version)?;

                    // compile the source code
                    let output = match compiler.compile_exact(&input) {
                        Ok(output) => output,
                        Err(compiler_error) => {
                            // Dump source code immediately for debugging
                            let (original_dir, instrumented_dir) =
                                dump_source_for_debugging(address, &artifact.input, &input)?;

                            // Write compiler error to file
                            let error_file = instrumented_dir.parent()
                                .unwrap_or(&instrumented_dir)
                                .join("compilation_errors.txt");

                            let error_content = format!(
                                "Compiler Error for Contract {}\n{}\n\n{}",
                                address,
                                "=".repeat(60),
                                compiler_error
                            );

                            fs::write(&error_file, &error_content)?;

                            bail!(
                                "Compilation failed\n  Error details: {error_file:?}\n  Original source: {original_dir:?}\n  Instrumented source: {instrumented_dir:?}",
                            );
                        }
                    };

                    // Check for compilation errors
                    if output.errors.iter().any(|e| e.is_error()) {
                        // Dump source code immediately for debugging
                        let (original_dir, instrumented_dir) =
                            dump_source_for_debugging(address, &artifact.input, &input)?;

                        // Format errors with better source location info
                        let formatted_errors = format_compiler_errors(&output.errors, &instrumented_dir);

                        // Write formatted errors to file
                        let error_file = instrumented_dir.parent()
                            .unwrap_or(&instrumented_dir)
                            .join("compilation_errors.txt");

                        let error_content = format!(
                            "Compilation Errors for Contract {address}\n{}\n\n{formatted_errors}",
                            "=".repeat(60),
                        );

                        fs::write(&error_file, &error_content)?;

                        bail!(
                            "Compilation failed\n  Error details: {error_file:?}\n  Original source: {original_dir:?}\n  Instrumented source: {instrumented_dir:?}",
                        );
                    }

                    debug!(
                        "Recompiled Contract {}: {} vs {}",
                        address,
                        artifact.output.contracts.len(),
                        output.contracts.len()
                    );

                    Ok(Artifact { meta, input, output })
                })();

                match &result {
                    Ok(_) => pb.set_message(format!("✅ 0x{short_addr}... instrumented")),
                    Err(_) => pb.set_message(format!("❌ 0x{short_addr}... failed")),
                }

                pb.inc(1);
                (*address, result)
            })
            .collect();

    progress_bar.finish_with_message("✨ Instrumentation complete!");

    // Process results and collect errors
    let mut recompiled_artifacts = HashMap::new();
    let mut all_errors = Vec::new();

    for (address, result) in results {
        match result {
            Ok(artifact) => {
                recompiled_artifacts.insert(address, artifact);
            }
            Err(e) => {
                all_errors.push((address, e));
            }
        }
    }

    // If any errors occurred, create a comprehensive error message
    if !all_errors.is_empty() {
        let mut error_msg = format!(
            "Failed to instrument {} contract(s). Debug information saved to:\n\n",
            all_errors.len()
        );

        for (i, (addr, err)) in all_errors.iter().enumerate() {
            // This already contains the paths from the error creation above
            error_msg.push_str(&format!("{}. Contract {addr}:\n{err}\n\n", i + 1,));
        }

        error_msg.push_str("Please check the error details files for full compilation errors.");

        bail!("{error_msg}");
    }

    Ok(recompiled_artifacts)
}
