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

//! Comprehensive JSON-RPC API tests using shared analysis logic
//!
//! This test suite uses the same analysis functions for both baseline capture and validation,
//! eliminating any possibility of implementation discrepancies.

use std::{collections::HashMap, fs, io::ErrorKind, time::Instant};

use edb_integration_tests::{
    rpc_test_utils::{
        BaselineLoader, BaselineMetadata, ComprehensiveBaseline, analyze_transaction_comprehensive,
        create_summary, get_or_create_fixture, get_or_create_fixtures, test_transactions,
    },
    test_utils::{init, paths, proxy},
};
use once_cell::sync::Lazy;
use tokio::sync::Mutex;
use tracing::info;

static COMPREHENSIVE_TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn local_proxy_bind_is_restricted(error: &eyre::Report) -> bool {
    error.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io_error| {
            matches!(io_error.kind(), ErrorKind::PermissionDenied)
                || io_error.raw_os_error() == Some(1)
        })
    }) || error.to_string().contains("Operation not permitted")
}

/// Comprehensive baseline capture with detailed analysis for a single transaction
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Run manually to capture new baselines"]
async fn capture_single_baseline() {
    let tx_name = test_transactions::UNISWAP_V4.0; // Change this to a fixture name from test_transactions::all()

    // Initialize test environment if needed
    init::init_test_environment(false);

    // Create fixture
    let fixture = match get_or_create_fixture(tx_name).await {
        Ok(fixture) => fixture,
        Err(error) if local_proxy_bind_is_restricted(&error) => {
            info!("Skipping {} because loopback binds are restricted: {error}", tx_name);
            return;
        }
        Err(error) => panic!("Failed to create {tx_name} transaction fixture: {error:?}"),
    };

    info!("=== STARTING BASELINE CAPTURE FOR '{}' TRANSACTION ===", tx_name);

    // Create output directory
    let output_dir = paths::get_baseline_dir();
    if let Err(e) = fs::create_dir_all(&output_dir) {
        println!("Creating output directory: {e:?}");
    }

    let start_time = Instant::now();

    // Analyze the transaction comprehensively
    let baseline = analyze_transaction_comprehensive(&fixture).await;

    let capture_time = start_time.elapsed();
    println!("✓ {} analysis completed in {:.2}s", tx_name, capture_time.as_secs_f64());

    // Save individual transaction file
    let tx_file = output_dir.join(format!("{tx_name}_baseline.json"));
    let tx_json =
        serde_json::to_string_pretty(&baseline).expect("Failed to serialize transaction data");
    fs::write(&tx_file, tx_json).expect("Failed to write transaction file");
    println!("💾 Saved {tx_name} baseline to: {tx_file:?}");

    println!("=== BASELINE CAPTURE COMPLETE ===");
    println!("Total time: {:.2}s", capture_time.as_secs_f64());
    println!("Files saved to: {output_dir:?}");

    // Close the proxy to ensure cache files are written
    proxy::shutdown_test_proxy(&fixture.proxy_url).await.ok();
}

/// Comprehensive baseline capture with detailed analysis
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Run manually to capture new baselines"]
async fn capture_all_baselines() {
    // Initialize test environment if needed
    init::init_test_environment(false);

    let overall_start = Instant::now();
    let fixtures = get_or_create_fixtures().await.expect("Failed to get fixtures");

    println!("=== STARTING COMPREHENSIVE BASELINE CAPTURE ===");
    println!("Transactions to analyze: {}", fixtures.len());

    // Create output directory
    let output_dir = paths::get_baseline_dir();
    if let Err(e) = fs::create_dir_all(&output_dir) {
        println!("Creating output directory: {e:?}");
    }

    let mut comprehensive_baseline = ComprehensiveBaseline {
        metadata: BaselineMetadata {
            capture_timestamp: chrono::Utc::now().to_rfc3339(),
            total_transactions: fixtures.len(),
            edb_version: env!("CARGO_PKG_VERSION").to_string(),
            test_environment: "comprehensive_integration_test".to_string(),
            total_capture_time_ms: 0, // Will be filled later
        },
        transactions: HashMap::new(),
    };

    // Analyze each transaction comprehensively
    for (tx_name, fixture) in fixtures.iter() {
        let tx_start = Instant::now();
        println!("=== ANALYZING {} TRANSACTION ===", tx_name.to_uppercase());

        let baseline = analyze_transaction_comprehensive(fixture).await;
        comprehensive_baseline.transactions.insert(tx_name.clone(), baseline);

        let tx_time = tx_start.elapsed();
        println!("✓ {} analysis completed in {:.2}s", tx_name, tx_time.as_secs_f64());

        // Save individual transaction file
        let tx_file = output_dir.join(format!("{tx_name}_baseline.json"));
        let tx_json = serde_json::to_string_pretty(&comprehensive_baseline.transactions[tx_name])
            .expect("Failed to serialize transaction data");
        fs::write(&tx_file, tx_json).expect("Failed to write transaction file");
        println!("💾 Saved {tx_name} baseline to: {tx_file:?}");
    }

    // Finalize metadata
    comprehensive_baseline.metadata.total_capture_time_ms = overall_start.elapsed().as_millis();

    let total_time = overall_start.elapsed();

    println!("=== COMPREHENSIVE CAPTURE COMPLETE ===");
    println!("Total time: {:.2}s", total_time.as_secs_f64());
    println!("Files saved to: {output_dir:?}");
    println!("📊 Summary: {} transactions analyzed", fixtures.len());

    let summary = create_summary(&comprehensive_baseline);
    println!("=== ANALYSIS SUMMARY ===\n{}", serde_json::to_string_pretty(&summary).unwrap());

    // Close the proxy to ensure cache files are written
    for fixture in fixtures.values() {
        proxy::shutdown_test_proxy(&fixture.proxy_url).await.ok();
    }
}

/// Test comprehensive RPC analysis for simple transaction using shared logic
#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_comprehensive_simple() {
    test_comprehensive_transaction("simple").await;
}

/// Test comprehensive RPC analysis for large transaction using shared logic
#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_comprehensive_large() {
    test_comprehensive_transaction("large").await;
}

/// Test comprehensive RPC analysis for Uniswap V3 transaction using shared logic
#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_comprehensive_uniswap_v3() {
    test_comprehensive_transaction("uniswap_v3").await;
}

/// Test comprehensive RPC analysis for Another Uniswap V3 transaction using shared logic
#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_comprehensive_another_uniswap_v3() {
    test_comprehensive_transaction("uniswap_v3_alt").await;
}

/// Test comprehensive RPC analysis for Uniswap V4 transaction using shared logic
#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_comprehensive_uniswap_v4() {
    test_comprehensive_transaction("uniswap_v4").await;
}

/// Test comprehensive RPC analysis for Out-Of-Gas tweak transaction using shared logic
#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_comprehensive_oog_tweak() {
    test_comprehensive_transaction("oog_tweak").await;
}

/// Core test function for comprehensive transaction analysis
async fn test_comprehensive_transaction(tx_name: &str) {
    let _guard = COMPREHENSIVE_TEST_LOCK.lock().await;

    info!("Starting comprehensive RPC test for {} transaction using shared logic...", tx_name);

    // Initialize test environment if needed
    init::init_test_environment(true);

    // Verify baseline exists
    let baseline_loader = BaselineLoader::new();
    assert!(
        baseline_loader.baseline_files_exist(),
        "Baseline files must exist for comprehensive testing"
    );

    // Load expected baseline data as ComprehensiveAnalysisResult for direct comparison
    let expected_analysis = baseline_loader
        .load_comprehensive_analysis_result(tx_name)
        .unwrap_or_else(|_| panic!("Failed to load baseline analysis for {tx_name}"));

    // Create fixture
    let fixture = match get_or_create_fixture(tx_name).await {
        Ok(fixture) => fixture,
        Err(error) if local_proxy_bind_is_restricted(&error) => {
            info!("Skipping {} because loopback binds are restricted: {error}", tx_name);
            return;
        }
        Err(error) => panic!("Failed to create {tx_name} transaction fixture: {error:?}"),
    };

    // Perform comprehensive analysis using the SAME logic as baseline capture
    let actual_analysis = analyze_transaction_comprehensive(&fixture).await;

    // Compare actual vs expected using direct struct comparison (excludes performance metrics)
    if actual_analysis != expected_analysis {
        panic!("Comprehensive analysis mismatch for {tx_name}");
    }

    info!("Comprehensive analysis matches baseline for {}", tx_name);

    info!("Comprehensive RPC test for {} completed successfully!", tx_name);

    proxy::shutdown_test_proxy(&fixture.proxy_url).await.ok();
}
