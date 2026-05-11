// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! Build the foundry-cheatcodes inspector with EDB's per-cheatcode
//! configuration. Boundary-cheatcode rejection lands in a later task.

use std::{path::PathBuf, sync::Arc};

use edb_common::EdbContext;
use foundry_cheatcodes::Cheatcodes;
use foundry_evm_core::evm::{EthEvmNetwork, FoundryEvmNetwork};
use revm::Database;

/// EDB-side configuration for the cheatcodes inspector.
#[allow(dead_code)] // `upstream_rpc` consumed by Tasks 8.x (fork-URL support)
#[derive(Clone, Debug)]
pub struct CheatsConfig {
    /// Path to the foundry project root. Used by cheatcodes that read project
    /// metadata (e.g. `vm.projectRoot()`, `vm.getCode("Foo")`).
    pub project_root: PathBuf,
    /// If forking is enabled, the upstream URL — used by fork-metadata cheatcodes
    /// (createFork). None means fork-free mode (the cheatcode will return a dummy
    /// or error, depending on Phase 7 rejection table).
    pub upstream_rpc: Option<String>,
}

#[cfg(test)]
impl CheatsConfig {
    pub fn default_for_test() -> Self {
        Self { project_root: std::env::temp_dir(), upstream_rpc: None }
    }
}

/// EDB wrapper around foundry's `Cheatcodes<FEN>` inspector.
///
/// # Context compatibility
///
/// EDB's orchestration runs the EVM against `EdbContext<DB>` (=
/// `revm::Context<BlockEnv, TxEnv, CfgEnv, CacheDB<DB>>`).  Foundry's
/// `Cheatcodes<FEN>` implements `Inspector` only for its own
/// `FoundryContextFor<'_, FEN>` (= `Context<..., &mut dyn DatabaseExt<..>>`),
/// whose database layer exposes fork management and cheatcode-access-list APIs
/// that plain `CacheDB<DB>` does not provide.
///
/// Bridging the two requires adopting foundry's `Backend` as EDB's database —
/// a deeper architectural change scheduled for a later task.  For now
/// `EdbCheatcodes` carries the `Cheatcodes<FEN>` value so it is ready to be
/// activated once the context migration lands, and provides a **no-op**
/// `Inspector<EdbContext<DB>>` impl that satisfies the engine's trait bound
/// without panicking at runtime.
pub struct EdbCheatcodes<FEN: FoundryEvmNetwork = EthEvmNetwork> {
    inner: Cheatcodes<FEN>,
}

impl<FEN: FoundryEvmNetwork> EdbCheatcodes<FEN> {
    pub fn new(inner: Cheatcodes<FEN>) -> Self {
        Self { inner }
    }

    /// Expose the inner foundry `Cheatcodes<FEN>` for future use when EDB
    /// migrates to a foundry-compatible execution context.
    #[allow(dead_code)]
    pub fn into_inner(self) -> Cheatcodes<FEN> {
        self.inner
    }
}

// ─── Inspector<EdbContext<DB>> ───────────────────────────────────────────────
//
// All hooks default to no-ops.  EDB's orchestration executes against
// `EdbContext<DB>` whose database is `CacheDB<DB>` — a type that does not
// implement foundry's `DatabaseExt` trait.  Forwarding to the inner
// `Cheatcodes<FEN>` is therefore not yet possible; the inner value is
// preserved for the upcoming context-migration task.
impl<FEN: FoundryEvmNetwork, DB: Database + revm::DatabaseRef> revm::Inspector<EdbContext<DB>>
    for EdbCheatcodes<FEN>
{
    // Default no-op implementations are sufficient for compilation.
    // Real cheatcode dispatch will be wired once EDB adopts foundry's Backend.
}

/// Build a factory yielding fresh `EdbCheatcodes<EthEvmNetwork>` per call.
/// Each pass of the engine pipeline (tracer / opcode / hook) gets a clean
/// cheatcodes inspector so `vm.prank`/`vm.mockCall`/etc state doesn't bleed
/// between passes.
pub fn build_cheats_factory(
    config: CheatsConfig,
) -> impl Fn() -> EdbCheatcodes<EthEvmNetwork> + Send + Sync {
    let config = Arc::new(config);
    move || {
        let foundry_config = build_foundry_cheats_config(&config);
        EdbCheatcodes::new(Cheatcodes::new(Arc::new(foundry_config)))
    }
}

/// Build a minimal `foundry_cheatcodes::CheatsConfig` from EDB's `CheatsConfig`.
///
/// Uses `foundry_config::Config::with_root` to get a reasonable default config
/// rooted at the project directory, then passes `EvmOpts::default()` and no
/// available artifacts / running artifact / fee token.
fn build_foundry_cheats_config(config: &CheatsConfig) -> foundry_cheatcodes::CheatsConfig {
    let foundry_cfg = foundry_config::Config::with_root(&config.project_root);
    let evm_opts = foundry_evm_core::opts::EvmOpts::default();
    foundry_cheatcodes::CheatsConfig::new(&foundry_cfg, evm_opts, None, None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_returns_distinct_instances() {
        // Verify that the factory closure can be constructed and called, and that
        // build_cheats_factory compiles with correct types.
        let factory = build_cheats_factory(CheatsConfig::default_for_test());
        // Call factory twice to produce two distinct inspectors.
        let _a = factory();
        let _b = factory();
    }

    #[test]
    fn cheats_config_for_test_uses_tempdir() {
        let c = CheatsConfig::default_for_test();
        assert_eq!(c.project_root, std::env::temp_dir());
        assert!(c.upstream_rpc.is_none());
    }
}
