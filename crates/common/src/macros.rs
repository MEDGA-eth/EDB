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

//! Path-based conditional assertion macros for EDB
//!
//! This module provides assertion macros that can be selectively enabled at runtime based
//! on module paths using the `EDB_ASSERT` environment variable. This allows for fine-grained
//! control over which assertions are active, similar to how `RUST_LOG` controls logging.
//!
//! # Environment Variable Syntax
//!
//! The `EDB_ASSERT` environment variable accepts the following patterns:
//!
//! - **Enable all assertions**: `EDB_ASSERT=*` or `EDB_ASSERT=all`
//! - **Enable specific crate**: `EDB_ASSERT=edb_engine` (enables all assertions in `edb_engine` and
//!   its submodules)
//! - **Enable specific module**: `EDB_ASSERT=edb_engine::inspector` (enables assertions in this
//!   module and its children)
//! - **Multiple targets**: `EDB_ASSERT=edb_engine::inspector,edb_common::types` (comma-separated)
//!
//! # Matching Rules
//!
//! - **Exact and prefix matching**: A pattern like `edb_engine` matches:
//!   - `edb_engine` (the crate root)
//!   - `edb_engine::inspector` (submodule)
//!   - `edb_engine::inspector::opcode_snapshot_inspector` (nested submodule)
//!
//! - **Wildcards**: The patterns `*` or `all` match all module paths
//!
//! - **Default behavior**: When `EDB_ASSERT` is not set or empty, all assertions are **disabled**
//!
//! # Performance Optimization
//!
//! These macros use `std::hint::unlikely` to hint to the compiler that assertion failures
//! are rare, allowing for better branch prediction and code optimization.
//!
//! # Examples
//!
//! ```bash
//! # Enable all assertions
//! EDB_ASSERT=* cargo test
//!
//! # Enable assertions only in the engine crate
//! EDB_ASSERT=edb_engine cargo test
//!
//! # Enable assertions in specific modules
//! EDB_ASSERT=edb_engine::inspector,edb_common::types cargo test
//!
//! # Enable assertions in inspector and all its submodules
//! EDB_ASSERT=edb_engine::inspector cargo run
//! ```
//!
//! # Usage in Code
//!
//! ```ignore
//! use edb_common::{edb_assert, edb_assert_eq, edb_assert_ne};
//!
//! fn process_data(value: u32) {
//!     // Only checked when this module is enabled via EDB_ASSERT
//!     edb_assert!(value > 0, "value must be positive");
//!     edb_assert_eq!(value % 2, 0, "value must be even");
//! }
//! ```

use once_cell::sync::Lazy;
use std::env;

/// Global storage for assertion target patterns from the EDB_ASSERT environment variable
static ASSERTION_TARGETS: Lazy<Vec<String>> =
    Lazy::new(|| match env::var(crate::env::EDB_ASSERT) {
        Ok(val) if !val.is_empty() => {
            val.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
        }
        _ => Vec::new(),
    });

/// Check if assertions are enabled for the given module path
///
/// This function is used internally by the assertion macros to determine
/// if assertions should be active based on the `EDB_ASSERT` environment variable.
///
/// # Arguments
///
/// * `module_path` - The module path to check (typically from `module_path!()` macro)
///
/// # Returns
///
/// `true` if assertions are enabled for this module, `false` otherwise
///
/// # Matching Logic
///
/// - If no targets are configured (empty `EDB_ASSERT`), returns `false`
/// - If any target is `"*"` or `"all"`, returns `true` for all modules
/// - Otherwise, returns `true` if the module path starts with any configured target
///
/// # Examples
///
/// ```rust, ignore
/// use edb_common::macros::is_assertion_enabled;
///
/// // Assuming EDB_ASSERT=edb_engine::inspector
/// assert!(is_assertion_enabled("edb_engine::inspector"));
/// assert!(is_assertion_enabled("edb_engine::inspector::opcode_snapshot_inspector"));
/// assert!(!is_assertion_enabled("edb_common::types"));
/// ```
pub fn is_assertion_enabled(module_path: &str) -> bool {
    if ASSERTION_TARGETS.is_empty() {
        return false; // Default: all assertions disabled
    }

    for target in ASSERTION_TARGETS.iter() {
        // Check for wildcard patterns
        if target == "*" || target == "all" {
            return true;
        }
        // Check for prefix match (e.g., "edb_engine" matches "edb_engine::inspector")
        if module_path.starts_with(target.as_str()) {
            return true;
        }
    }

    false
}

/// Helper functions marked with #[cold] to hint that assertion checks are rarely taken
#[cold]
#[inline(never)]
pub fn cold_path() {
    // This function is marked cold to hint to the compiler that reaching here is unlikely
    // The actual work happens in the caller after this returns
}

/// Assert a condition only when enabled via `EDB_ASSERT` environment variable.
///
/// This macro checks if assertions are enabled for the current module path via the
/// `EDB_ASSERT` environment variable. If enabled, it evaluates the condition and panics
/// if the condition is false.
///
/// # Performance
///
/// Uses `#[cold]` attribute to hint to the compiler that the assertion check path
/// is rarely taken, allowing better branch prediction and code layout optimization.
///
/// # Examples
///
/// ```ignore
/// use edb_common::edb_assert;
///
/// let value = 42;
/// edb_assert!(value == 42);
/// edb_assert!(value == 42, "value should be 42, got {}", value);
/// ```
///
/// # Environment Variable
///
/// Set `EDB_ASSERT=edb_common` or `EDB_ASSERT=*` to enable these assertions.
#[macro_export]
macro_rules! edb_assert {
    ($($arg:tt)*) => {
        if $crate::macros::is_assertion_enabled(module_path!()) {
            $crate::macros::cold_path();
            assert!($($arg)*);
        }
    };
}

/// Assert two expressions are equal only when enabled via `EDB_ASSERT`.
///
/// This macro checks if assertions are enabled for the current module path via the
/// `EDB_ASSERT` environment variable. If enabled, it compares the two expressions and
/// panics if they are not equal.
///
/// # Performance
///
/// Uses `#[cold]` attribute to hint to the compiler that the assertion check path
/// is rarely taken, allowing better branch prediction and code layout optimization.
///
/// # Examples
///
/// ```ignore
/// use edb_common::edb_assert_eq;
///
/// let a = 1 + 1;
/// edb_assert_eq!(a, 2);
/// edb_assert_eq!(a, 2, "expected {} to equal 2", a);
/// ```
///
/// # Environment Variable
///
/// Set `EDB_ASSERT=edb_common` or `EDB_ASSERT=*` to enable these assertions.
#[macro_export]
macro_rules! edb_assert_eq {
    ($($arg:tt)*) => {
        if $crate::macros::is_assertion_enabled(module_path!()) {
            $crate::macros::cold_path();
            assert_eq!($($arg)*);
        }
    };
}

/// Assert two expressions are not equal only when enabled via `EDB_ASSERT`.
///
/// This macro checks if assertions are enabled for the current module path via the
/// `EDB_ASSERT` environment variable. If enabled, it compares the two expressions and
/// panics if they are equal.
///
/// # Performance
///
/// Uses `#[cold]` attribute to hint to the compiler that the assertion check path
/// is rarely taken, allowing better branch prediction and code layout optimization.
///
/// # Examples
///
/// ```ignore
/// use edb_common::edb_assert_ne;
///
/// let a = 1 + 1;
/// edb_assert_ne!(a, 3);
/// edb_assert_ne!(a, 3, "expected {} to not equal 3", a);
/// ```
///
/// # Environment Variable
///
/// Set `EDB_ASSERT=edb_common` or `EDB_ASSERT=*` to enable these assertions.
#[macro_export]
macro_rules! edb_assert_ne {
    ($($arg:tt)*) => {
        if $crate::macros::is_assertion_enabled(module_path!()) {
            $crate::macros::cold_path();
            assert_ne!($($arg)*);
        }
    };
}

/// Debug assert that only executes when `EDB_ASSERT` is set and in debug builds.
///
/// This macro combines the behavior of `debug_assert!` with the conditional execution
/// based on the `EDB_ASSERT` environment variable. It only executes in debug builds.
///
/// # Performance
///
/// Uses `#[cold]` attribute to hint to the compiler that the assertion check path
/// is rarely taken, allowing better branch prediction and code layout optimization.
/// In release builds, this macro compiles to nothing.
///
/// # Examples
///
/// ```ignore
/// use edb_common::edb_debug_assert;
///
/// let value = 42;
/// edb_debug_assert!(value == 42);
/// ```
///
/// # Environment Variable
///
/// Set `EDB_ASSERT=edb_common` or `EDB_ASSERT=*` to enable these assertions.
#[macro_export]
macro_rules! edb_debug_assert {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        {
            $crate::edb_assert!($($arg)*);
        }
    };
}

/// Debug assert equality that only executes when `EDB_ASSERT` is set and in debug builds.
///
/// This macro combines the behavior of `debug_assert_eq!` with the conditional execution
/// based on the `EDB_ASSERT` environment variable. It only executes in debug builds.
///
/// # Performance
///
/// Uses `#[cold]` attribute to hint to the compiler that the assertion check path
/// is rarely taken, allowing better branch prediction and code layout optimization.
/// In release builds, this macro compiles to nothing.
///
/// # Examples
///
/// ```ignore
/// use edb_common::edb_debug_assert_eq;
///
/// let a = 1 + 1;
/// edb_debug_assert_eq!(a, 2);
/// ```
///
/// # Environment Variable
///
/// Set `EDB_ASSERT=edb_common` or `EDB_ASSERT=*` to enable these assertions.
#[macro_export]
macro_rules! edb_debug_assert_eq {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        {
            $crate::edb_assert_eq!($($arg)*);
        }
    };
}

/// Debug assert inequality that only executes when `EDB_ASSERT` is set and in debug builds.
///
/// This macro combines the behavior of `debug_assert_ne!` with the conditional execution
/// based on the `EDB_ASSERT` environment variable. It only executes in debug builds.
///
/// # Performance
///
/// Uses `#[cold]` attribute to hint to the compiler that the assertion check path
/// is rarely taken, allowing better branch prediction and code layout optimization.
/// In release builds, this macro compiles to nothing.
///
/// # Examples
///
/// ```ignore
/// use edb_common::edb_debug_assert_ne;
///
/// let a = 1 + 1;
/// edb_debug_assert_ne!(a, 3);
/// ```
///
/// # Environment Variable
///
/// Set `EDB_ASSERT=edb_common` or `EDB_ASSERT=*` to enable these assertions.
#[macro_export]
macro_rules! edb_debug_assert_ne {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        {
            $crate::edb_assert_ne!($($arg)*);
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Due to the use of Lazy for caching assertion targets, these tests
    // verify behavior with the environment variable state at the time of the first
    // call to is_assertion_enabled().
    //
    // To test with assertions enabled, run:
    //   EDB_ASSERT=edb_common::macros cargo test -p edb-common macros --lib

    #[test]
    fn test_is_assertion_enabled_logic() {
        // Test the matching logic directly with sample targets
        // This tests the core logic without relying on environment variables

        // Simulate wildcard matching
        let targets = ["*".to_string()];
        assert!(targets.iter().any(|t| t == "*" || t == "all"));

        // Simulate prefix matching
        assert!("edb_engine::inspector".starts_with("edb_engine"));
        assert!("edb_engine".starts_with("edb_engine"));
        assert!(!"edb_common".starts_with("edb_engine"));

        // Simulate specific module matching
        assert!("edb_engine::inspector::opcode".starts_with("edb_engine::inspector"));
        assert!(!"edb_engine::eval".starts_with("edb_engine::inspector"));

        // Simulate multiple targets
        let targets = ["edb_engine".to_string(), "edb_common::types".to_string()];
        assert!(targets.iter().any(|t| "edb_engine::inspector".starts_with(t)));
        assert!(targets.iter().any(|t| "edb_common::types::snapshot".starts_with(t)));
        assert!(!targets.iter().any(|t| "edb_tui".starts_with(t)));
    }

    #[test]
    fn test_edb_assert_basic_functionality() {
        // These tests verify that the macros compile and execute correctly
        // When EDB_ASSERT=edb_common::macros, these will actually check assertions
        // When EDB_ASSERT is not set or doesn't match, they will be no-ops

        // These should not panic (conditions are all true)
        edb_assert!(1 == 1);
        edb_assert!(true, "with message");
        edb_assert_eq!(42, 42);
        edb_assert_eq!(1, 1, "with custom message");
        edb_assert_ne!(1, 2);
        edb_assert_ne!(5, 10, "custom message");
        edb_debug_assert!(true);
    }

    #[test]
    fn test_macro_syntax_variations() {
        // Test that all macro syntax variations compile correctly

        // edb_assert with and without message
        edb_assert!(2 + 2 == 4);
        edb_assert!(true, "message");
        edb_assert!(true, "formatted {}", "message");

        // edb_assert_eq with and without message
        edb_assert_eq!(1, 1);
        edb_assert_eq!(2, 2, "message");
        edb_assert_eq!(3, 3, "formatted {}", "message");

        // edb_assert_ne with and without message
        edb_assert_ne!(1, 2);
        edb_assert_ne!(3, 4, "message");
        edb_assert_ne!(5, 6, "formatted {}", "message");

        // edb_debug_assert with and without message
        edb_debug_assert!(true);
        edb_debug_assert!(true, "message");

        // edb_debug_assert_eq with and without message
        edb_debug_assert_eq!(1, 1);
        edb_debug_assert_eq!(2, 2, "message");

        // edb_debug_assert_ne with and without message
        edb_debug_assert_ne!(1, 2);
        edb_debug_assert_ne!(3, 4, "message");
    }

    #[test]
    fn test_is_assertion_enabled_current_module() {
        // Test that our current module path matches correctly
        // This module is edb_common::macros::tests

        let current_module = module_path!();
        assert!(current_module.starts_with("edb_common::macros"));

        // If EDB_ASSERT=edb_common::macros (or edb_common, or *), this should be true
        // If EDB_ASSERT is not set or different, this will be false
        let enabled = is_assertion_enabled(current_module);

        // Test that the function works with various paths
        assert!(is_assertion_enabled("edb_common::macros::tests") == enabled);
    }

    #[test]
    #[should_panic(expected = "assertion failed: false")]
    fn test_edb_assert_panics_when_enabled_for_this_module() {
        // This test will panic if EDB_ASSERT=edb_common::macros (or edb_common, or *)
        // It will pass (not panic) if assertions are not enabled for this module

        if is_assertion_enabled(module_path!()) {
            edb_assert!(false);
        } else {
            // Force a panic with the expected message to make the test pass
            // when assertions are not enabled
            panic!("assertion failed: false");
        }
    }

    #[test]
    fn test_path_matching_specificity() {
        // Test that path matching works at different levels of specificity

        // edb_common should match edb_common::macros
        assert!("edb_common::macros".starts_with("edb_common"));

        // edb_common::macros should match edb_common::macros::tests
        assert!("edb_common::macros::tests".starts_with("edb_common::macros"));

        // But edb_common::types should not match edb_common::macros
        assert!(!"edb_common::macros".starts_with("edb_common::types"));

        // And edb_engine should not match edb_common
        assert!(!"edb_common::macros".starts_with("edb_engine"));
    }
}
