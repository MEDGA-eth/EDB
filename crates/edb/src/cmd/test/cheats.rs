// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! Build the foundry-cheatcodes inspector with EDB's per-cheatcode
//! configuration. Boundary-cheatcode rejection lands in a later task.

use std::{path::PathBuf, sync::Arc};

use foundry_cheatcodes::Cheatcodes;
use foundry_evm_core::evm::{EthEvmNetwork, FoundryContextFor, FoundryEvmNetwork};

/// EDB-side configuration for the cheatcodes inspector.
#[allow(dead_code)] // consumed by downstream tasks (5.3+)
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

/// EDB wrapper around foundry's `Cheatcodes<FEN>` inspector. Forwards all
/// Inspector hooks to the inner implementation. Boundary-cheatcode rejection
/// comes in Task 7.1.
#[allow(dead_code)] // consumed by downstream tasks (5.3+)
pub struct EdbCheatcodes<FEN: FoundryEvmNetwork = EthEvmNetwork> {
    inner: Cheatcodes<FEN>,
}

impl<FEN: FoundryEvmNetwork> EdbCheatcodes<FEN> {
    #[allow(dead_code)] // consumed by downstream tasks (5.3+)
    pub fn new(inner: Cheatcodes<FEN>) -> Self {
        Self { inner }
    }
}

// Implement revm::Inspector by forwarding every hook to the inner
// `Cheatcodes<FEN>`.  The CTX is the concrete `FoundryContextFor<'_, FEN>`
// that `Cheatcodes<FEN>` already implements `Inspector` for.
impl<'db, FEN: FoundryEvmNetwork> revm::Inspector<FoundryContextFor<'db, FEN>>
    for EdbCheatcodes<FEN>
where
    Cheatcodes<FEN>: revm::Inspector<FoundryContextFor<'db, FEN>>,
{
    fn initialize_interp(
        &mut self,
        interp: &mut revm::interpreter::Interpreter,
        ctx: &mut FoundryContextFor<'db, FEN>,
    ) {
        self.inner.initialize_interp(interp, ctx);
    }

    fn step(
        &mut self,
        interp: &mut revm::interpreter::Interpreter,
        ctx: &mut FoundryContextFor<'db, FEN>,
    ) {
        self.inner.step(interp, ctx);
    }

    fn step_end(
        &mut self,
        interp: &mut revm::interpreter::Interpreter,
        ctx: &mut FoundryContextFor<'db, FEN>,
    ) {
        self.inner.step_end(interp, ctx);
    }

    fn log(&mut self, ctx: &mut FoundryContextFor<'db, FEN>, log: alloy_primitives::Log) {
        self.inner.log(ctx, log);
    }

    fn log_full(
        &mut self,
        interp: &mut revm::interpreter::Interpreter,
        ctx: &mut FoundryContextFor<'db, FEN>,
        log: alloy_primitives::Log,
    ) {
        self.inner.log_full(interp, ctx, log);
    }

    fn frame_start(
        &mut self,
        ctx: &mut FoundryContextFor<'db, FEN>,
        frame_input: &mut revm::interpreter::FrameInput,
    ) -> Option<revm::handler::FrameResult> {
        self.inner.frame_start(ctx, frame_input)
    }

    fn frame_end(
        &mut self,
        ctx: &mut FoundryContextFor<'db, FEN>,
        frame_input: &revm::interpreter::FrameInput,
        frame_result: &mut revm::handler::FrameResult,
    ) {
        self.inner.frame_end(ctx, frame_input, frame_result);
    }

    fn call(
        &mut self,
        ctx: &mut FoundryContextFor<'db, FEN>,
        inputs: &mut revm::interpreter::CallInputs,
    ) -> Option<revm::interpreter::CallOutcome> {
        self.inner.call(ctx, inputs)
    }

    fn call_end(
        &mut self,
        ctx: &mut FoundryContextFor<'db, FEN>,
        inputs: &revm::interpreter::CallInputs,
        outcome: &mut revm::interpreter::CallOutcome,
    ) {
        self.inner.call_end(ctx, inputs, outcome);
    }

    fn create(
        &mut self,
        ctx: &mut FoundryContextFor<'db, FEN>,
        inputs: &mut revm::interpreter::CreateInputs,
    ) -> Option<revm::interpreter::CreateOutcome> {
        self.inner.create(ctx, inputs)
    }

    fn create_end(
        &mut self,
        ctx: &mut FoundryContextFor<'db, FEN>,
        inputs: &revm::interpreter::CreateInputs,
        outcome: &mut revm::interpreter::CreateOutcome,
    ) {
        self.inner.create_end(ctx, inputs, outcome);
    }

    fn selfdestruct(
        &mut self,
        contract: alloy_primitives::Address,
        target: alloy_primitives::Address,
        value: alloy_primitives::U256,
    ) {
        self.inner.selfdestruct(contract, target, value);
    }
}

/// Build a factory yielding fresh `EdbCheatcodes<EthEvmNetwork>` per call.
/// Each pass of the engine pipeline (tracer / opcode / hook) gets a clean
/// cheatcodes inspector so `vm.prank`/`vm.mockCall`/etc state doesn't bleed
/// between passes.
#[allow(dead_code)] // consumed by downstream tasks (5.3+)
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
