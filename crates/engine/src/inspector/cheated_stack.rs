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

//! Two-element inspector stack: an optional foundry-cheatcodes-style inspector
//! composed in front of one of EDB's existing inspectors.
//!
//! Forwarding order:
//! - `call` / `create` / `log`: cheats first (it can short-circuit calls for
//!   `mockCall`, `expectRevert`, EDB's rejection list, etc.). On a `None`
//!   return from cheats, forward to the inner inspector.
//! - `call_end` / `create_end`: cheats first (adjusts outcomes for
//!   `expectRevert`, `expectEmit`, gas metering), then inner.
//! - `step` / `step_end`: forward to BOTH, inner first. Cheatcodes only needs
//!   step hooks for gas metering; running inner first keeps EDB's snapshot
//!   ordering deterministic.
//! - `selfdestruct`: forward to both, inner first.

use alloy_primitives::{Address, Log, U256};
use revm::{
    Inspector,
    handler::FrameResult,
    interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, FrameInput, Interpreter},
};

/// Wraps an EDB inspector with an optional cheatcodes-style inspector.
///
/// `Cheats` is generic so this file has no direct dependency on
/// `foundry-cheatcodes`. The `edb` crate instantiates it concretely.
pub struct CheatedStack<'a, Cheats, Inner> {
    cheats: Option<&'a mut Cheats>,
    inner: &'a mut Inner,
}

impl<'a, Cheats, Inner> CheatedStack<'a, Cheats, Inner> {
    /// Create a new `CheatedStack` with an optional cheats inspector and a
    /// mandatory inner EDB inspector.
    pub fn new(cheats: Option<&'a mut Cheats>, inner: &'a mut Inner) -> Self {
        Self { cheats, inner }
    }
}

impl<CTX, Cheats, Inner> Inspector<CTX> for CheatedStack<'_, Cheats, Inner>
where
    Cheats: Inspector<CTX>,
    Inner: Inspector<CTX>,
{
    fn step(&mut self, interp: &mut Interpreter, ctx: &mut CTX) {
        self.inner.step(interp, ctx);
        if let Some(c) = self.cheats.as_deref_mut() {
            c.step(interp, ctx);
        }
    }

    fn step_end(&mut self, interp: &mut Interpreter, ctx: &mut CTX) {
        self.inner.step_end(interp, ctx);
        if let Some(c) = self.cheats.as_deref_mut() {
            c.step_end(interp, ctx);
        }
    }

    fn log(&mut self, ctx: &mut CTX, log: Log) {
        if let Some(c) = self.cheats.as_deref_mut() {
            c.log(ctx, log.clone());
        }
        self.inner.log(ctx, log);
    }

    fn log_full(&mut self, interp: &mut Interpreter, ctx: &mut CTX, log: Log) {
        if let Some(c) = self.cheats.as_deref_mut() {
            c.log_full(interp, ctx, log.clone());
        }
        self.inner.log_full(interp, ctx, log);
    }

    fn frame_start(&mut self, ctx: &mut CTX, frame_input: &mut FrameInput) -> Option<FrameResult> {
        let cheats_outcome =
            self.cheats.as_deref_mut().and_then(|c| c.frame_start(ctx, frame_input));
        let inner_outcome = self.inner.frame_start(ctx, frame_input);
        cheats_outcome.or(inner_outcome)
    }

    fn frame_end(
        &mut self,
        ctx: &mut CTX,
        frame_input: &FrameInput,
        frame_result: &mut FrameResult,
    ) {
        if let Some(c) = self.cheats.as_deref_mut() {
            c.frame_end(ctx, frame_input, frame_result);
        }
        self.inner.frame_end(ctx, frame_input, frame_result);
    }

    fn call(&mut self, ctx: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        let cheats_outcome = self.cheats.as_deref_mut().and_then(|c| c.call(ctx, inputs));
        let inner_outcome = self.inner.call(ctx, inputs);
        cheats_outcome.or(inner_outcome)
    }

    fn call_end(&mut self, ctx: &mut CTX, inputs: &CallInputs, outcome: &mut CallOutcome) {
        if let Some(c) = self.cheats.as_deref_mut() {
            c.call_end(ctx, inputs, outcome);
        }
        self.inner.call_end(ctx, inputs, outcome);
    }

    fn create(&mut self, ctx: &mut CTX, inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        let cheats_outcome = self.cheats.as_deref_mut().and_then(|c| c.create(ctx, inputs));
        let inner_outcome = self.inner.create(ctx, inputs);
        cheats_outcome.or(inner_outcome)
    }

    fn create_end(&mut self, ctx: &mut CTX, inputs: &CreateInputs, outcome: &mut CreateOutcome) {
        if let Some(c) = self.cheats.as_deref_mut() {
            c.create_end(ctx, inputs, outcome);
        }
        self.inner.create_end(ctx, inputs, outcome);
    }

    fn selfdestruct(&mut self, contract: Address, target: Address, value: U256) {
        self.inner.selfdestruct(contract, target, value);
        if let Some(c) = self.cheats.as_deref_mut() {
            c.selfdestruct(contract, target, value);
        }
    }
}

/// Placeholder cheatcode inspector used in non-test-mode calls so the type slot
/// is inhabited without pulling in foundry-cheatcodes.
pub struct NoCheats;

impl<CTX> Inspector<CTX> for NoCheats {}
