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

//! Fancy logging configuration for EDB components
//!
//! Provides centralized logging setup with:
//! - Colorful console output with structured formatting
//! - File logging to temporary directory
//! - Environment variable support (RUST_LOG)
//! - Default INFO level with beautiful styling

use eyre::Result;
use std::{env, fs, path::PathBuf, sync::Once};
use tracing::Level;
use tracing_appender::{non_blocking, rolling};
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, format::FmtSpan, time::LocalTime},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

/// Initialize fancy logging for EDB components
///
/// This function sets up:
/// - Colorful, structured console logging with timestamps
/// - File logging to a temporary directory with daily rotation
/// - Environment variable support for log levels (RUST_LOG)
/// - Default INFO level if no RUST_LOG is set
/// - Beautiful formatting with component names and spans
///
/// # Arguments
/// * `component_name` - Name of the component (e.g., "edb", "edb-rpc-proxy")
/// * `enable_file_logging` - Whether to enable file logging (default: true)
///
/// # Returns
/// * `Result<()>` - Success or error from logging initialization
///
/// # Examples
/// ```rust
/// use edb_common::logging;
///
/// #[tokio::main]
/// async fn main() -> eyre::Result<()> {
///     // Initialize logging for the main EDB binary
///     logging::init_logging("edb", true)?;
///
///     tracing::info!("Application started");
///     Ok(())
/// }
/// ```
pub fn init_logging(component_name: &str, enable_file_logging: bool) -> Result<()> {
    // Create environment filter with default WARN level
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(Level::WARN.as_str()))
        .expect("Failed to create environment filter");

    // Create beautiful console layer with colors and formatting
    let console_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_timer(LocalTime::rfc_3339())
        .with_ansi(true) // Enable colors
        .pretty(); // Use pretty formatting

    if enable_file_logging {
        // Create log directory in temp folder
        let log_dir = create_log_directory(component_name)?;

        // Create file appender with daily rotation
        let file_appender = rolling::daily(&log_dir, format!("{component_name}.log"));
        let (non_blocking_appender, guard) = non_blocking(file_appender);

        // Store guard to prevent it from being dropped
        std::mem::forget(guard);

        // Create file layer (without colors for file output)
        let file_layer = fmt::layer()
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_file(true)
            .with_line_number(true)
            .with_span_events(FmtSpan::CLOSE)
            .with_timer(LocalTime::rfc_3339())
            .with_ansi(false) // No colors in files
            .with_writer(non_blocking_appender);

        // Initialize subscriber with both console and file layers
        tracing_subscriber::registry()
            .with(env_filter)
            .with(console_layer)
            .with(file_layer)
            .try_init()
            .map_err(|e| eyre::eyre!("Failed to initialize tracing subscriber: {}", e))?;

        tracing::info!(
            component = component_name,
            log_dir = %log_dir.display(),
            "Logging initialized with console and file output"
        );
    } else {
        // Initialize subscriber with only console layer
        tracing_subscriber::registry()
            .with(env_filter)
            .with(console_layer)
            .try_init()
            .map_err(|e| eyre::eyre!("Failed to initialize tracing subscriber: {}", e))?;

        tracing::info!(component = component_name, "Logging initialized with console output only");
    }

    // Log some useful information
    log_environment_info(component_name);

    Ok(())
}

/// Create log directory in system temp folder
fn create_log_directory(component_name: &str) -> Result<PathBuf> {
    let temp_dir = env::temp_dir();
    let log_dir = temp_dir.join("edb-logs").join(component_name);

    fs::create_dir_all(&log_dir)?;

    Ok(log_dir)
}

/// Log useful environment and system information
fn log_environment_info(component_name: &str) {
    let rust_log = env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let args: Vec<String> = env::args().collect();

    tracing::info!(
        component = component_name,
        rust_log = %rust_log,
        args = ?args,
        "Environment information"
    );

    if let Ok(current_dir) = env::current_dir() {
        tracing::debug!(
            working_directory = %current_dir.display(),
            "Working directory"
        );
    }
}

/// Initialize file-only logging for TUI applications
///
/// This function sets up logging that only writes to a file, not to stdout/stderr.
/// This is essential for TUI applications that need full control over the terminal.
///
/// # Arguments
/// * `component_name` - Name of the component (e.g., "edb-tui")
///
/// # Returns
/// * `Result<PathBuf>` - The path to the log file on success
///
/// # Examples
/// ```rust
/// use edb_common::logging;
///
/// #[tokio::main]
/// async fn main() -> eyre::Result<()> {
///     // Initialize file-only logging for TUI
///     let log_path = logging::init_file_only_logging("edb-tui")?;
///     eprintln!("Logs are being written to: {}", log_path.display());
///
///     tracing::info!("TUI started");
///     Ok(())
/// }
/// ```
pub fn init_file_only_logging(component_name: &str) -> Result<PathBuf> {
    // Create environment filter with default INFO level
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(Level::INFO.as_str()))
        .expect("Failed to create environment filter");

    // Create log directory in temp folder
    let log_dir = create_log_directory(component_name)?;
    let log_file_path = log_dir.join(format!("{component_name}.log"));

    // Create file appender with daily rotation
    let file_appender = rolling::daily(&log_dir, format!("{component_name}.log"));
    let (non_blocking_appender, guard) = non_blocking(file_appender);

    // Store guard to prevent it from being dropped
    std::mem::forget(guard);

    // Create file-only layer (no console output)
    let file_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(false) // Less verbose for TUI logs
        .with_thread_names(false)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_timer(LocalTime::rfc_3339())
        .with_ansi(false) // No colors in files
        .with_writer(non_blocking_appender);

    // Initialize subscriber with only file layer (no console output)
    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .try_init()
        .map_err(|e| eyre::eyre!("Failed to initialize TUI file logging: {}", e))?;

    tracing::info!(
        component = component_name,
        log_file = %log_file_path.display(),
        "File-only logging initialized"
    );

    // Log some useful information
    log_environment_info(component_name);

    Ok(log_file_path)
}

/// Initialize simple logging (console only, no fancy formatting)
///
/// This is useful for tests or simple utilities that don't need
/// the full fancy logging setup.
///
/// # Arguments  
/// * `level` - The default log level to use
pub fn init_simple_logging(level: Level) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(level.as_str()))
        .expect("Failed to create environment filter");

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .compact()
        .try_init()
        .map_err(|e| eyre::eyre!("Failed to initialize simple logging: {}", e))?;

    Ok(())
}

// Global test logging initialization - ensures logging is only set up once across all tests
static TEST_LOGGING_INIT: Once = Once::new();

/// Safe logging initialization for tests - can be called multiple times without crashing
///
/// This function provides a safe way for tests to enable logging without worrying about
/// whether a tracing subscriber has already been initialized. It uses `std::sync::Once`
/// to ensure initialization happens only once per test process.
///
/// Features:
/// - Console-only output (no file logging for tests)
/// - DEBUG level by default, but respects RUST_LOG environment variable
/// - Can be called from any test file safely
/// - Idempotent - multiple calls are safe and efficient
///
/// # Usage
/// ```rust,ignore
/// use edb_common::logging;
/// use tracing::info;
///
/// #[test]
/// fn my_test() {
///     logging::ensure_test_logging();
///     info!("This will work safely in any test!");
///     // ... rest of test
/// }
/// ```
pub fn ensure_test_logging(default_level: Option<Level>) {
    TEST_LOGGING_INIT.call_once(|| {
        // Initialize simple console-only logging for tests
        // Default to INFO but respect RUST_LOG if set
        let default_level = default_level.unwrap_or(Level::INFO);
        let _ = init_simple_logging(default_level);
        // Ignore any errors - if initialization fails, that's usually because
        // a subscriber is already set up, which is fine for tests
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::{debug, error, info, warn};

    // Use the public ensure_test_logging function
    fn init_test_logging() {
        ensure_test_logging(None);
    }

    #[test]
    fn test_logging_functions_work() {
        // This test ensures logging functions work without panicking
        init_test_logging();

        // Test that we can log without errors
        info!("Test info message");
        warn!("Test warning message");
        debug!("Test debug message");
        error!("Test error message");

        // Test passes if no panic occurs
    }

    #[test]
    fn test_log_directory_creation() {
        // Test that we can create log directories
        let result = create_log_directory("test-component");
        assert!(result.is_ok());

        let log_dir = result.unwrap();
        assert!(log_dir.exists());
        assert!(log_dir.to_string_lossy().contains("edb-logs"));
        assert!(log_dir.to_string_lossy().contains("test-component"));
    }

    #[test]
    fn test_fancy_logging_initialization_safety() {
        // Test that fancy logging handles multiple initialization attempts gracefully
        init_test_logging(); // Ensure something is already initialized

        // These calls should not panic, even if subscriber is already initialized
        let result1 = init_logging("test-fancy-1", false);
        let result2 = init_logging("test-fancy-2", false);

        // One or both may fail due to already initialized subscriber, but should not panic
        // The important thing is that the function handles the error case gracefully
        match (result1, result2) {
            (Ok(_), _) => {}       // First succeeded
            (Err(_), Ok(_)) => {}  // Second succeeded
            (Err(_), Err(_)) => {} // Both failed gracefully
        }

        // Verify logging still works after initialization attempts
        info!("Test logging after fancy init attempts");
    }
}
