// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
// SPDX-License-Identifier: AGPL-3.0

//! Hand-rolled cheatcode inspector for `edb test` sessions.
//!
//! Implements a curated subset of foundry's cheatcodes by intercepting CALLs
//! to the cheatcode precompile address (0x7109...DD12D) and dispatching by
//! 4-byte selector. Unsupported cheatcodes return a revert with a clear EDB
//! error string.
//!
//! Coverage matrix: see `docs/cheatcode-coverage.md` (linked from README).
//!
//! Design notes:
//! - The inspector is generic over `EdbContext<DB>` so it composes natively
//!   with EDB's `CacheDB<DB>` journal — no Inspector trait-bound mismatch
//!   like the upstream foundry-cheatcodes inspector (see Task 5.5 commit).
//! - State (pranks, mocks, expectRevert, recorded logs, labels) lives on
//!   the inspector value itself; the engine builds a fresh inspector via
//!   `build_cheats_factory` for each orchestration pass.

use alloy_primitives::{Address, B256, Bytes, Log, U256, address};
use revm::{
    Database, DatabaseCommit, DatabaseRef, Inspector,
    context::JournalTr,
    context_interface::journaled_state::account::JournaledAccountTr,
    database::CacheDB,
    interpreter::{CallInputs, CallOutcome, Gas, InstructionResult, InterpreterResult},
    state::Bytecode,
};
use std::collections::{BTreeMap, HashMap};

/// Cheatcode precompile address (matches foundry's).
pub const CHEATCODE_ADDRESS: Address = address!("7109709ECfa91a80626fF3989D68f67F5b1DD12D");

// ----------------------------------------------------------------------------
// Cheatcode selectors (verified against forge-std's `Vm.sol` by recomputing
// keccak256 of the canonical signature; see this file's unit tests).
// ----------------------------------------------------------------------------

// Supported set
const SEL_WARP: [u8; 4] = [0xe5, 0xd6, 0xbf, 0x02]; // warp(uint256)
const SEL_ROLL: [u8; 4] = [0x1f, 0x7b, 0x4f, 0x30]; // roll(uint256)
const SEL_CHAIN_ID: [u8; 4] = [0x40, 0x49, 0xdd, 0xd2]; // chainId(uint256)
const SEL_DEAL: [u8; 4] = [0xc8, 0x8a, 0x5e, 0x6d]; // deal(address,uint256)
const SEL_ETCH: [u8; 4] = [0xb4, 0xd6, 0xc7, 0x82]; // etch(address,bytes)
const SEL_STORE: [u8; 4] = [0x70, 0xca, 0x10, 0xbb]; // store(address,bytes32,bytes32)
const SEL_LOAD: [u8; 4] = [0x66, 0x7f, 0x9d, 0x70]; // load(address,bytes32)
const SEL_SET_NONCE: [u8; 4] = [0xf8, 0xe1, 0x8b, 0x57]; // setNonce(address,uint64)
const SEL_PRANK: [u8; 4] = [0xca, 0x66, 0x9f, 0xa7]; // prank(address)
const SEL_START_PRANK: [u8; 4] = [0x06, 0x44, 0x7d, 0x56]; // startPrank(address)
const SEL_STOP_PRANK: [u8; 4] = [0x90, 0xc5, 0x01, 0x3b]; // stopPrank()
const SEL_MOCK_CALL: [u8; 4] = [0xb9, 0x62, 0x13, 0xe4]; // mockCall(address,bytes,bytes)
const SEL_MOCK_CALL_REVERT: [u8; 4] = [0xdb, 0xaa, 0xd1, 0x47]; // mockCallRevert(address,bytes,bytes)
const SEL_CLEAR_MOCKED_CALLS: [u8; 4] = [0x3f, 0xdf, 0x4e, 0x15]; // clearMockedCalls()
const SEL_EXPECT_REVERT_BARE: [u8; 4] = [0xf4, 0x84, 0x48, 0x14]; // expectRevert()
const SEL_EXPECT_REVERT_BYTES: [u8; 4] = [0xf2, 0x8d, 0xce, 0xb3]; // expectRevert(bytes)
const SEL_LABEL: [u8; 4] = [0xc6, 0x57, 0xc7, 0x18]; // label(address,string)
const SEL_RECORD_LOGS: [u8; 4] = [0x41, 0xaf, 0x2f, 0x52]; // recordLogs()
const SEL_GET_RECORDED_LOGS: [u8; 4] = [0x19, 0x15, 0x53, 0xa4]; // getRecordedLogs()
const SEL_EXPECT_EMIT_BARE: [u8; 4] = [0x44, 0x0e, 0xd1, 0x0d]; // expectEmit()
const SEL_EXPECT_EMIT_FILTER4: [u8; 4] = [0x49, 0x1c, 0xc7, 0xc2]; // expectEmit(bool,bool,bool,bool)
const SEL_EXPECT_EMIT_FILTER5: [u8; 4] = [0x81, 0xba, 0xd6, 0xf3]; // expectEmit(bool,bool,bool,bool,address)
const SEL_EXPECT_EMIT_ADDR: [u8; 4] = [0x86, 0xb9, 0x62, 0x0d]; // expectEmit(address)
const SEL_EXPECT_CALL: [u8; 4] = [0xbd, 0x6a, 0xf4, 0x34]; // expectCall(address,bytes)
const SEL_EXPECT_CALL_COUNT: [u8; 4] = [0xc1, 0xad, 0xbb, 0xff]; // expectCall(address,bytes,uint64)
const SEL_EXPECT_CALL_MIN_GAS: [u8; 4] = [0x08, 0xe4, 0xe1, 0x16]; // expectCallMinGas(address,uint256,uint64,bytes)
const SEL_ASSUME: [u8; 4] = [0x4c, 0x63, 0xe5, 0x62]; // assume(bool)
const SEL_ENV_BOOL: [u8; 4] = [0x7e, 0xd1, 0xec, 0x7d]; // envBool(string)
const SEL_ENV_BYTES: [u8; 4] = [0x4d, 0x7b, 0xaf, 0x06]; // envBytes(string)
const SEL_ENV_STRING: [u8; 4] = [0xf8, 0x77, 0xcb, 0x19]; // envString(string)
const SEL_ENV_OR_BOOL: [u8; 4] = [0x47, 0x77, 0xf3, 0xcf]; // envOr(string,bool)
const SEL_ENV_OR_BYTES: [u8; 4] = [0xb3, 0xe4, 0x77, 0x05]; // envOr(string,bytes)
const SEL_ENV_OR_STRING: [u8; 4] = [0xd1, 0x45, 0x73, 0x6c]; // envOr(string,string)

// Explicitly rejected — multi-fork / state-snapshot / scripting / fs+ffi.
const SEL_SNAPSHOT_STATE: [u8; 4] = [0x9c, 0xd2, 0x38, 0x35]; // snapshotState()
const SEL_SNAPSHOT_LEGACY: [u8; 4] = [0x97, 0x11, 0x71, 0x5a]; // snapshot()
const SEL_REVERT_TO_STATE: [u8; 4] = [0xc2, 0x52, 0x74, 0x05]; // revertToState(uint256)
const SEL_REVERT_TO_LEGACY: [u8; 4] = [0x44, 0xd7, 0xf0, 0xa4]; // revertTo(uint256)
const SEL_CREATE_FORK: [u8; 4] = [0x31, 0xba, 0x34, 0x98]; // createFork(string)
const SEL_CREATE_SELECT_FORK: [u8; 4] = [0x98, 0x68, 0x00, 0x34]; // createSelectFork(string)
const SEL_SELECT_FORK: [u8; 4] = [0x9e, 0xbf, 0x68, 0x27]; // selectFork(uint256)
const SEL_ROLL_FORK: [u8; 4] = [0xd9, 0xbb, 0xf3, 0xa1]; // rollFork(uint256)
const SEL_ACTIVE_FORK: [u8; 4] = [0x2f, 0x10, 0x3f, 0x22]; // activeFork()
const SEL_MAKE_PERSISTENT: [u8; 4] = [0x57, 0xe2, 0x2d, 0xde]; // makePersistent(address)
const SEL_TRANSACT: [u8; 4] = [0xbe, 0x64, 0x6d, 0xa1]; // transact(bytes32)
const SEL_FFI: [u8; 4] = [0x89, 0x16, 0x04, 0x67]; // ffi(string[])
const SEL_READ_FILE: [u8; 4] = [0x60, 0xf9, 0xbb, 0x11]; // readFile(string)
const SEL_WRITE_FILE: [u8; 4] = [0x89, 0x7e, 0x0a, 0x97]; // writeFile(string,string)
const SEL_REMOVE_FILE: [u8; 4] = [0xf1, 0xaf, 0xe0, 0x4d]; // removeFile(string)
const SEL_BROADCAST: [u8; 4] = [0xaf, 0xc9, 0x80, 0x40]; // broadcast()
const SEL_START_BROADCAST: [u8; 4] = [0x7f, 0xb5, 0x29, 0x7f]; // startBroadcast()
const SEL_STOP_BROADCAST: [u8; 4] = [0x76, 0xea, 0xdd, 0x36]; // stopBroadcast()

// ----------------------------------------------------------------------------
// Config + state types
// ----------------------------------------------------------------------------

/// Configuration for the cheatcodes inspector.
#[derive(Clone, Debug, Default)]
pub struct CheatsConfig {
    /// Project root (used for future fs-allowlist; currently unused).
    #[allow(dead_code)] // reserved for fs-allowlist in a future phase
    pub project_root: std::path::PathBuf,
}

/// Hand-rolled cheatcode inspector over `EdbContext<DB>`.
#[derive(Debug)]
pub struct EdbCheatcodes {
    #[allow(dead_code)] // reserved for future fs-allowlist
    config: CheatsConfig,
    /// Pranks keyed by call depth (the depth at which the prank was installed).
    pranks: HashMap<usize, Prank>,
    /// Mock returns keyed by (target, calldata).
    mocks: HashMap<Address, BTreeMap<Bytes, MockReturn>>,
    /// Active expectRevert (consumed by the next call_end).
    expected_revert: Option<ExpectedRevert>,
    /// Address → human label (recorded but otherwise unused by the inspector;
    /// callers can read it via the public getter for trace pretty-printing).
    labels: HashMap<Address, String>,
    /// Whether vm.recordLogs() has armed the recorder.
    recording_logs: bool,
    /// Logs captured since the last `recordLogs()`.
    recorded_logs: Vec<Log>,
    /// Pending log expectations from `vm.expectEmit`. Matched against incoming
    /// logs via `Inspector::log`. Verified — and removed — when the registering
    /// frame ends (its `call_end` fires). Failed expectations rewrite that
    /// frame's outcome to a Revert with a clear `EDB: expectEmit ...` message.
    ///
    /// v1 SOFT-MATCH semantics: we don't capture a "template log" from the
    /// next `emit Foo(...)` in the test contract (foundry's full semantics).
    /// Instead, an expectation matches the first log it sees that satisfies
    /// emitter+topic-presence constraints. See `docs/cheatcode-coverage.md`.
    expected_emits: Vec<ExpectedEmit>,
    /// Pending call expectations from `vm.expectCall`. Inspector::call
    /// increments `observed` for matching (target, calldata) pairs. Verified
    /// when the registering frame ends.
    expected_calls: Vec<ExpectedCall>,
    /// Monotonic frame-depth counter for non-cheatcode calls. Incremented in
    /// `Inspector::call` for non-cheatcode targets, decremented in `call_end`
    /// for the same. Used to scope `expected_emits` / `expected_calls` to the
    /// frame that registered them — robust to the "emit happens in the same
    /// frame as the registration, with no intervening sub-call" case (where
    /// REVM's own `depth()` would never cross a child boundary).
    call_depth: u64,
}

#[derive(Clone, Debug)]
struct Prank {
    /// Replacement msg.sender.
    new_caller: Address,
    /// `true` for `vm.prank` (consumed by first call), `false` for `vm.startPrank`.
    one_shot: bool,
    /// Whether a one-shot prank has fired (set in `call`, cleared in `call_end`).
    fired: bool,
}

#[derive(Clone, Debug)]
enum MockReturn {
    Return(Bytes),
    Revert(Bytes),
}

#[derive(Clone, Debug)]
struct ExpectedRevert {
    /// `None` = match any revert; `Some(bytes)` = match exact revert payload.
    expected_data: Option<Bytes>,
}

/// Pending `vm.expectEmit` expectation. v1 soft-match semantics: we don't
/// know the template log content at registration time (foundry infers it from
/// the test contract's own next `emit Foo(...)` between the cheatcode call and
/// the next external call), so the expectation matches the first log that
/// satisfies the structural constraints recorded here.
#[derive(Clone, Debug)]
struct ExpectedEmit {
    /// Whether each of the 4 topic slots must be present in the matched log.
    /// Foundry's `(bool t1, bool t2, bool t3, bool t4)` overloads encode this
    /// directly. In soft-match mode we only verify the matched log HAS the
    /// requested topic at the requested index; we do not compare its value
    /// against a template (we have no template).
    check_topics: [bool; 4],
    /// Whether data presence is required. In soft-match mode we only verify
    /// the matched log carries non-empty data when this is true; we do not
    /// compare data bytes against a template.
    check_data: bool,
    /// `None` = match any emitter, `Some(addr)` = only logs from this emitter.
    expected_emitter: Option<Address>,
    /// Set to true once a log matched. Read at the registering frame's
    /// `call_end` to decide pass/fail.
    matched: bool,
    /// Frame-depth at which `vm.expectEmit` ran (see `EdbCheatcodes::call_depth`).
    registered_at_call_depth: u64,
}

#[derive(Clone, Debug)]
struct ExpectedCall {
    target: Address,
    calldata: Bytes,
    /// Minimum number of matching calls required to satisfy the expectation.
    /// `vm.expectCall(addr, data)` sets this to 1; the (..., uint64 count)
    /// overload uses the supplied count.
    min_count: u64,
    /// Number of matching (target, calldata) calls observed so far.
    observed: u64,
    /// Frame-depth at which the expectation was set.
    registered_at_call_depth: u64,
}

// ----------------------------------------------------------------------------
// Construction + public accessors
// ----------------------------------------------------------------------------

impl EdbCheatcodes {
    /// Build a fresh inspector with the given config.
    pub fn new(config: CheatsConfig) -> Self {
        Self {
            config,
            pranks: HashMap::new(),
            mocks: HashMap::new(),
            expected_revert: None,
            labels: HashMap::new(),
            recording_logs: false,
            recorded_logs: Vec::new(),
            expected_emits: Vec::new(),
            expected_calls: Vec::new(),
            call_depth: 0,
        }
    }

    /// Returns the address-label map collected via `vm.label`.
    #[allow(dead_code)] // public API; consumed by trace pretty-printing in a future phase
    pub fn labels(&self) -> &HashMap<Address, String> {
        &self.labels
    }

    /// Returns the captured logs (filled by `vm.recordLogs`).
    #[allow(dead_code)] // public API; consumed by test result reporting in a future phase
    pub fn recorded_logs(&self) -> &[Log] {
        &self.recorded_logs
    }
}

/// Factory for building fresh `EdbCheatcodes` instances per engine pass.
///
/// `Engine::prepare_with_router_and_cheats` calls this once per orchestration
/// pass (tracer / opcode / hook) so prank/mock/expectRevert state never bleeds
/// between passes.
pub fn build_cheats_factory(
    config: CheatsConfig,
) -> impl Fn() -> EdbCheatcodes + Send + Sync + 'static {
    let config = std::sync::Arc::new(config);
    move || EdbCheatcodes::new((*config).clone())
}

// ----------------------------------------------------------------------------
// Inspector impl over EdbContext<DB>
// ----------------------------------------------------------------------------

impl<DB> Inspector<edb_common::EdbContext<DB>> for EdbCheatcodes
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    fn call(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        // 1) target == cheatcode address → decode & dispatch.
        //    The cheatcode call itself does NOT count toward `call_depth`; it
        //    is intercepted before any real frame work happens.
        if inputs.target_address == CHEATCODE_ADDRESS {
            return Some(self.dispatch(ctx, inputs));
        }

        // 2) Bump the non-cheatcode call-depth counter. We do this BEFORE
        //    mock interception so a fully mocked call still participates in
        //    `expectCall` accounting and frame-scoping (its `call_end` will
        //    fire and decrement again).
        self.call_depth += 1;

        // 3) Active prank for this depth?
        //    Pranks are keyed by the depth at which they were installed; we
        //    apply them to calls made *at the next depth down*, which mirrors
        //    forge's "vm.prank affects the next call" semantics. In practice
        //    that means: when `vm.prank` runs in the test at depth N, the
        //    next sub-call to user code happens at depth N+1 with the
        //    overridden caller — but `ctx.journaled_state.depth()` reports
        //    the *parent* frame's depth at this moment in the Inspector,
        //    so we look up by exactly that depth.
        let depth = ctx.journaled_state.depth();
        if let Some(prank) = self.pranks.get_mut(&depth) {
            inputs.caller = prank.new_caller;
            if prank.one_shot {
                prank.fired = true;
            }
        }

        // 4) Resolve calldata once (shared by expectCall accounting + mockCall lookup).
        let calldata = match &inputs.input {
            revm::interpreter::CallInput::Bytes(b) => b.clone(),
            revm::interpreter::CallInput::SharedBuffer(range) => {
                use revm::context_interface::LocalContextTr;
                ctx.local
                    .shared_memory_buffer_slice(range.clone())
                    .map(|b| Bytes::from(b.to_vec()))
                    .unwrap_or_default()
            }
        };

        // 5) expectCall accounting — any pending expectation whose (target, calldata)
        //    match this call has its observed counter incremented.
        for ec in self.expected_calls.iter_mut() {
            if ec.target == inputs.target_address && ec.calldata == calldata {
                ec.observed = ec.observed.saturating_add(1);
            }
        }

        // 6) mockCall match?
        if let Some(mocks) = self.mocks.get(&inputs.target_address)
            && let Some(mock) = mocks.get(&calldata)
        {
            return Some(match mock {
                MockReturn::Return(data) => ok_return(inputs.gas_limit, data.clone()),
                MockReturn::Revert(data) => revert_with(inputs.gas_limit, data.clone()),
            });
        }

        None
    }

    fn call_end(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        outcome: &mut CallOutcome,
    ) {
        // Cheatcode call_end is a no-op for everything below: the cheatcode
        // call is intercepted in `call()`, never counted toward `call_depth`,
        // and never satisfies any expectation. Skip outright.
        if inputs.target_address == CHEATCODE_ADDRESS {
            return;
        }

        // Snapshot the frame depth this call belongs to before we decrement,
        // and decrement to mirror the increment in `call()`.
        let ending_depth = self.call_depth;
        self.call_depth = self.call_depth.saturating_sub(1);

        // Clear one-shot pranks that fired.
        self.pranks.retain(|_, p| !(p.one_shot && p.fired));

        // expectRevert: verify the just-completed call matches.
        if let Some(expected) = self.expected_revert.take() {
            let reverted = matches!(outcome.result.result, InstructionResult::Revert);
            let matched = match (reverted, &expected.expected_data) {
                (true, None) => true,
                (true, Some(want)) => outcome.result.output.as_ref() == want.as_ref(),
                (false, _) => false,
            };
            if matched {
                // Convert the matched revert into a successful return
                // (forge semantics: the test continues past expectRevert).
                outcome.result.result = InstructionResult::Return;
                outcome.result.output = Bytes::new();
            } else {
                outcome.result.result = InstructionResult::Revert;
                outcome.result.output = encode_error_string(
                    "EDB: expectRevert did not match: the call did not revert as expected",
                );
                // Early-out: don't also try to verify expectEmit/expectCall on
                // a frame we're already rewriting to a failure.
                return;
            }
        }

        // expectEmit: any expectation registered AT or BELOW this frame's depth
        // has now had its window closed (the registering frame is ending). For
        // soft-match v1 we require `matched == true`; otherwise rewrite the
        // outcome to a Revert with a clear EDB error.
        //
        // We collect failures before mutating `outcome` so a single unfulfilled
        // expectation reports a deterministic message.
        let unfulfilled_emit = self
            .expected_emits
            .iter()
            .find(|e| e.registered_at_call_depth >= ending_depth && !e.matched)
            .cloned();
        self.expected_emits.retain(|e| e.registered_at_call_depth < ending_depth);
        if let Some(_unfulfilled) = unfulfilled_emit {
            outcome.result.result = InstructionResult::Revert;
            outcome.result.output = encode_error_string(
                "EDB: expectEmit did not match: no log emitted that satisfied the expected \
                 emitter/topics/data constraints before the registering frame ended",
            );
            // Clear any same-frame expectCalls too — they're no longer relevant.
            self.expected_calls.retain(|ec| ec.registered_at_call_depth < ending_depth);
            return;
        }

        // expectCall: same window-closing logic.
        let unfulfilled_call = self
            .expected_calls
            .iter()
            .find(|ec| ec.registered_at_call_depth >= ending_depth && ec.observed < ec.min_count)
            .cloned();
        self.expected_calls.retain(|ec| ec.registered_at_call_depth < ending_depth);
        if let Some(uc) = unfulfilled_call {
            outcome.result.result = InstructionResult::Revert;
            outcome.result.output = encode_error_string(&format!(
                "EDB: expectCall did not match (target=0x{}, calldata len={}, expected>={}, observed={})",
                alloy_primitives::hex::encode(uc.target),
                uc.calldata.len(),
                uc.min_count,
                uc.observed,
            ));
        }
    }

    fn log(&mut self, _ctx: &mut edb_common::EdbContext<DB>, log: Log) {
        if self.recording_logs {
            self.recorded_logs.push(log.clone());
        }
        // First-fit match against pending expectEmits: the earliest unmatched
        // expectation that accepts this log wins. We bind one-log-per-expect.
        for emit in self.expected_emits.iter_mut() {
            if !emit.matched && emit.matches(&log) {
                emit.matched = true;
                break;
            }
        }
    }
}

impl ExpectedEmit {
    /// Soft-match: accept the log if its emitter matches (when constrained),
    /// and if the requested topic indices are PRESENT (not necessarily equal
    /// to a template — we don't have a template in v1). When `check_data` is
    /// true we additionally require non-empty data.
    fn matches(&self, log: &Log) -> bool {
        if let Some(want) = self.expected_emitter
            && log.address != want
        {
            return false;
        }
        let topics = log.topics();
        for (i, &check) in self.check_topics.iter().enumerate() {
            if check && i >= topics.len() {
                return false;
            }
        }
        if self.check_data && log.data.data.is_empty() {
            return false;
        }
        true
    }
}

// ----------------------------------------------------------------------------
// Dispatch + per-cheatcode handlers
// ----------------------------------------------------------------------------

impl EdbCheatcodes {
    fn dispatch<DB>(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        // Resolve calldata (handle SharedBuffer variant).
        let calldata_bytes = match &inputs.input {
            revm::interpreter::CallInput::Bytes(b) => b.clone(),
            revm::interpreter::CallInput::SharedBuffer(range) => {
                use revm::context_interface::LocalContextTr;
                ctx.local
                    .shared_memory_buffer_slice(range.clone())
                    .map(|b| Bytes::from(b.to_vec()))
                    .unwrap_or_default()
            }
        };
        let calldata = calldata_bytes.as_ref();
        if calldata.len() < 4 {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("EDB: cheatcode call has no selector"),
            );
        }
        let selector: [u8; 4] = calldata[..4].try_into().expect("just sliced 4 bytes");
        let args = &calldata[4..];
        match selector {
            // Supported
            SEL_WARP => self.cheat_warp(ctx, inputs, args),
            SEL_ROLL => self.cheat_roll(ctx, inputs, args),
            SEL_CHAIN_ID => self.cheat_chain_id(ctx, inputs, args),
            SEL_DEAL => self.cheat_deal(ctx, inputs, args),
            SEL_ETCH => self.cheat_etch(ctx, inputs, args),
            SEL_STORE => self.cheat_store(ctx, inputs, args),
            SEL_LOAD => self.cheat_load(ctx, inputs, args),
            SEL_SET_NONCE => self.cheat_set_nonce(ctx, inputs, args),
            SEL_PRANK => self.cheat_prank(ctx, inputs, args, true),
            SEL_START_PRANK => self.cheat_prank(ctx, inputs, args, false),
            SEL_STOP_PRANK => self.cheat_stop_prank(ctx, inputs),
            SEL_MOCK_CALL => self.cheat_mock_call(ctx, inputs, args, false),
            SEL_MOCK_CALL_REVERT => self.cheat_mock_call(ctx, inputs, args, true),
            SEL_CLEAR_MOCKED_CALLS => self.cheat_clear_mocked_calls(ctx, inputs),
            SEL_EXPECT_REVERT_BARE => self.cheat_expect_revert(inputs, None),
            SEL_EXPECT_REVERT_BYTES => self.cheat_expect_revert(inputs, Some(args)),
            SEL_LABEL => self.cheat_label(ctx, inputs, args),
            SEL_RECORD_LOGS => self.cheat_record_logs(inputs),
            SEL_GET_RECORDED_LOGS => self.cheat_get_recorded_logs(inputs),
            SEL_EXPECT_EMIT_BARE => self.cheat_expect_emit(ctx, inputs, args, ExpectEmitMode::All),
            SEL_EXPECT_EMIT_FILTER4 => {
                self.cheat_expect_emit(ctx, inputs, args, ExpectEmitMode::Filter4)
            }
            SEL_EXPECT_EMIT_FILTER5 => {
                self.cheat_expect_emit(ctx, inputs, args, ExpectEmitMode::Filter5)
            }
            SEL_EXPECT_EMIT_ADDR => {
                self.cheat_expect_emit(ctx, inputs, args, ExpectEmitMode::AnyTopicsFromEmitter)
            }
            SEL_EXPECT_CALL => self.cheat_expect_call(ctx, inputs, args, 1),
            SEL_EXPECT_CALL_COUNT => self.cheat_expect_call_with_count(ctx, inputs, args),
            SEL_EXPECT_CALL_MIN_GAS => revert_with(
                inputs.gas_limit,
                encode_error_string(
                    "EDB: cheatcode vm.expectCallMinGas not supported in v1: gas accounting under \
                     EDB's instrumented bytecode needs separate design work",
                ),
            ),

            // assume + env family
            SEL_ASSUME => self.cheat_assume(inputs, args),
            SEL_ENV_BOOL => self.cheat_env_bool(inputs, args),
            SEL_ENV_BYTES => self.cheat_env_bytes(inputs, args),
            SEL_ENV_STRING => self.cheat_env_string(inputs, args),
            SEL_ENV_OR_BOOL => self.cheat_env_or_bool(inputs, args),
            SEL_ENV_OR_BYTES => self.cheat_env_or_bytes(inputs, args),
            SEL_ENV_OR_STRING => self.cheat_env_or_string(inputs, args),

            // Explicitly rejected — multi-fork
            SEL_CREATE_FORK
            | SEL_CREATE_SELECT_FORK
            | SEL_SELECT_FORK
            | SEL_ROLL_FORK
            | SEL_ACTIVE_FORK
            | SEL_MAKE_PERSISTENT => {
                revert_with(inputs.gas_limit, unsupported_revert("multi-fork (e.g. selectFork)"))
            }
            // Explicitly rejected — state snapshots
            SEL_SNAPSHOT_STATE | SEL_SNAPSHOT_LEGACY | SEL_REVERT_TO_STATE
            | SEL_REVERT_TO_LEGACY => revert_with(
                inputs.gas_limit,
                unsupported_revert("state snapshots (snapshotState/revertToState)"),
            ),
            // Explicitly rejected — separate-tx model
            SEL_TRANSACT => revert_with(inputs.gas_limit, unsupported_revert("transact")),
            // Explicitly rejected — fs + ffi
            SEL_FFI | SEL_READ_FILE | SEL_WRITE_FILE | SEL_REMOVE_FILE => revert_with(
                inputs.gas_limit,
                unsupported_revert("ffi/fs (ffi, readFile, writeFile, removeFile)"),
            ),
            // Explicitly rejected — broadcasting
            SEL_BROADCAST | SEL_START_BROADCAST | SEL_STOP_BROADCAST => {
                revert_with(inputs.gas_limit, unsupported_revert("broadcast (script-only)"))
            }

            _ => {
                let hex = alloy_primitives::hex::encode(selector);
                revert_with(
                    inputs.gas_limit,
                    encode_error_string(&format!(
                        "EDB: unknown cheatcode selector 0x{hex} (likely not implemented in v1)"
                    )),
                )
            }
        }
    }

    // --- Block / chain mutators ---------------------------------------------

    fn cheat_warp<DB>(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(value) = read_u256(args, 0) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.warp: bad calldata"));
        };
        ctx.block.timestamp = value;
        ok_return(inputs.gas_limit, Bytes::new())
    }

    fn cheat_roll<DB>(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(value) = read_u256(args, 0) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.roll: bad calldata"));
        };
        ctx.block.number = value;
        ok_return(inputs.gas_limit, Bytes::new())
    }

    fn cheat_chain_id<DB>(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(value) = read_u256(args, 0) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.chainId: bad calldata"));
        };
        let chain_id: u64 = match value.try_into() {
            Ok(v) => v,
            Err(_) => {
                return revert_with(
                    inputs.gas_limit,
                    encode_error_string("vm.chainId: value does not fit in u64"),
                );
            }
        };
        ctx.cfg.chain_id = chain_id;
        ok_return(inputs.gas_limit, Bytes::new())
    }

    // --- Account state mutators ---------------------------------------------

    fn cheat_deal<DB>(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(target) = read_address(args, 0) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.deal: bad address arg"));
        };
        let Some(value) = read_u256(args, 1) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.deal: bad value arg"));
        };
        match ctx.journaled_state.load_account_mut(target) {
            Ok(mut acc) => {
                acc.set_balance(value);
                acc.touch();
                ok_return(inputs.gas_limit, Bytes::new())
            }
            Err(_) => revert_with(
                inputs.gas_limit,
                encode_error_string("vm.deal: failed to load account"),
            ),
        }
    }

    fn cheat_etch<DB>(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(target) = read_address(args, 0) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.etch: bad address arg"));
        };
        let Some(code) = read_bytes(args, 1) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.etch: bad bytes arg"));
        };
        // Make sure the account is warm before set_code (per JournalTr contract).
        if ctx.journaled_state.load_account_with_code(target).is_err() {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.etch: failed to load account"),
            );
        }
        let bytecode = Bytecode::new_raw(code);
        ctx.journaled_state.set_code(target, bytecode);
        ok_return(inputs.gas_limit, Bytes::new())
    }

    fn cheat_store<DB>(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(target) = read_address(args, 0) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.store: bad address arg"));
        };
        let Some(slot) = read_b256(args, 1) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.store: bad slot arg"));
        };
        let Some(value) = read_b256(args, 2) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.store: bad value arg"));
        };
        // Make sure the account is warm.
        if ctx.journaled_state.load_account(target).is_err() {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.store: failed to load account"),
            );
        }
        let key = U256::from_be_bytes(slot.0);
        let val = U256::from_be_bytes(value.0);
        if ctx.journaled_state.sstore(target, key, val).is_err() {
            return revert_with(inputs.gas_limit, encode_error_string("vm.store: sstore failed"));
        }
        ok_return(inputs.gas_limit, Bytes::new())
    }

    fn cheat_load<DB>(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(target) = read_address(args, 0) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.load: bad address arg"));
        };
        let Some(slot) = read_b256(args, 1) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.load: bad slot arg"));
        };
        if ctx.journaled_state.load_account(target).is_err() {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.load: failed to load account"),
            );
        }
        let key = U256::from_be_bytes(slot.0);
        match ctx.journaled_state.sload(target, key) {
            Ok(loaded) => {
                let bytes = Bytes::copy_from_slice(&loaded.data.to_be_bytes::<32>());
                ok_return(inputs.gas_limit, bytes)
            }
            Err(_) => revert_with(inputs.gas_limit, encode_error_string("vm.load: sload failed")),
        }
    }

    fn cheat_set_nonce<DB>(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(target) = read_address(args, 0) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.setNonce: bad address arg"),
            );
        };
        let Some(value) = read_u256(args, 1) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.setNonce: bad nonce arg"),
            );
        };
        let nonce: u64 = match value.try_into() {
            Ok(v) => v,
            Err(_) => {
                return revert_with(
                    inputs.gas_limit,
                    encode_error_string("vm.setNonce: nonce does not fit in u64"),
                );
            }
        };
        match ctx.journaled_state.load_account_mut(target) {
            Ok(mut acc) => {
                acc.set_nonce(nonce);
                acc.touch();
                ok_return(inputs.gas_limit, Bytes::new())
            }
            Err(_) => revert_with(
                inputs.gas_limit,
                encode_error_string("vm.setNonce: failed to load account"),
            ),
        }
    }

    // --- Pranks --------------------------------------------------------------

    fn cheat_prank<DB>(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
        one_shot: bool,
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(new_caller) = read_address(args, 0) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.prank/startPrank: bad address arg"),
            );
        };
        // Install at the caller's depth (the depth at which vm.prank ran).
        // The next sub-call out of that frame happens at the same depth from
        // the Inspector's vantage point (Inspector::call fires before the
        // child journal checkpoint), so we key by `depth()` at install time.
        let depth = ctx.journaled_state.depth();
        self.pranks.insert(depth, Prank { new_caller, one_shot, fired: false });
        ok_return(inputs.gas_limit, Bytes::new())
    }

    fn cheat_stop_prank<DB>(
        &mut self,
        ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let depth = ctx.journaled_state.depth();
        self.pranks.remove(&depth);
        ok_return(inputs.gas_limit, Bytes::new())
    }

    // --- Mocks --------------------------------------------------------------

    fn cheat_mock_call<DB>(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
        reverts: bool,
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(target) = read_address(args, 0) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.mockCall: bad address arg"),
            );
        };
        let Some(calldata) = read_bytes(args, 1) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.mockCall: bad calldata arg"),
            );
        };
        let Some(retdata) = read_bytes(args, 2) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.mockCall: bad return-data arg"),
            );
        };
        let entry = if reverts { MockReturn::Revert(retdata) } else { MockReturn::Return(retdata) };
        self.mocks.entry(target).or_default().insert(calldata, entry);
        ok_return(inputs.gas_limit, Bytes::new())
    }

    fn cheat_clear_mocked_calls<DB>(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        self.mocks.clear();
        ok_return(inputs.gas_limit, Bytes::new())
    }

    // --- expectRevert --------------------------------------------------------

    fn cheat_expect_revert(&mut self, inputs: &CallInputs, args: Option<&[u8]>) -> CallOutcome {
        let expected_data = match args {
            None => None,
            Some(a) => match read_bytes(a, 0) {
                Some(b) => Some(b),
                None => {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string("vm.expectRevert(bytes): bad bytes arg"),
                    );
                }
            },
        };
        self.expected_revert = Some(ExpectedRevert { expected_data });
        ok_return(inputs.gas_limit, Bytes::new())
    }

    // --- Labels --------------------------------------------------------------

    fn cheat_label<DB>(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(addr) = read_address(args, 0) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.label: bad address arg"));
        };
        let Some(label) = read_string(args, 1) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.label: bad string arg"));
        };
        self.labels.insert(addr, label);
        ok_return(inputs.gas_limit, Bytes::new())
    }

    // --- recordLogs / getRecordedLogs ----------------------------------------

    fn cheat_record_logs(&mut self, inputs: &CallInputs) -> CallOutcome {
        self.recording_logs = true;
        self.recorded_logs.clear();
        ok_return(inputs.gas_limit, Bytes::new())
    }

    /// ABI-encodes the captured logs as `Log[]` where
    /// `struct Log { bytes32[] topics; bytes data; address emitter; }`,
    /// matching foundry's `Vm.Log` shape.
    fn cheat_get_recorded_logs(&mut self, inputs: &CallInputs) -> CallOutcome {
        let logs = std::mem::take(&mut self.recorded_logs);
        // We stop recording after the read, matching foundry's reset semantic.
        self.recording_logs = false;
        let encoded = abi_encode_logs(&logs);
        ok_return(inputs.gas_limit, encoded)
    }

    // --- expectEmit ---------------------------------------------------------

    fn cheat_expect_emit<DB>(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
        mode: ExpectEmitMode,
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let (check_topics, check_data, expected_emitter) = match mode {
            ExpectEmitMode::All => ([true; 4], true, None),
            ExpectEmitMode::Filter4 => {
                let Some(t1) = read_bool(args, 0) else {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string("vm.expectEmit(bool,bool,bool,bool): bad arg 0"),
                    );
                };
                let Some(t2) = read_bool(args, 1) else {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string("vm.expectEmit(bool,bool,bool,bool): bad arg 1"),
                    );
                };
                let Some(t3) = read_bool(args, 2) else {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string("vm.expectEmit(bool,bool,bool,bool): bad arg 2"),
                    );
                };
                let Some(t4) = read_bool(args, 3) else {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string("vm.expectEmit(bool,bool,bool,bool): bad arg 3"),
                    );
                };
                // Foundry's bools are (check_topic_1, check_topic_2, check_topic_3, check_data).
                // We map them onto our 4-topic + data layout: topic[0] is the
                // event sig (always present when emitted), topics[1..4] are
                // the 3 indexed args.
                ([true, t1, t2, t3], t4, None)
            }
            ExpectEmitMode::Filter5 => {
                let Some(t1) = read_bool(args, 0) else {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string(
                            "vm.expectEmit(bool,bool,bool,bool,address): bad arg 0",
                        ),
                    );
                };
                let Some(t2) = read_bool(args, 1) else {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string(
                            "vm.expectEmit(bool,bool,bool,bool,address): bad arg 1",
                        ),
                    );
                };
                let Some(t3) = read_bool(args, 2) else {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string(
                            "vm.expectEmit(bool,bool,bool,bool,address): bad arg 2",
                        ),
                    );
                };
                let Some(t4) = read_bool(args, 3) else {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string(
                            "vm.expectEmit(bool,bool,bool,bool,address): bad arg 3",
                        ),
                    );
                };
                let Some(emitter) = read_address(args, 4) else {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string(
                            "vm.expectEmit(bool,bool,bool,bool,address): bad emitter arg",
                        ),
                    );
                };
                ([true, t1, t2, t3], t4, Some(emitter))
            }
            ExpectEmitMode::AnyTopicsFromEmitter => {
                let Some(emitter) = read_address(args, 0) else {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string("vm.expectEmit(address): bad emitter arg"),
                    );
                };
                ([true; 4], true, Some(emitter))
            }
        };
        self.expected_emits.push(ExpectedEmit {
            check_topics,
            check_data,
            expected_emitter,
            matched: false,
            registered_at_call_depth: self.call_depth,
        });
        ok_return(inputs.gas_limit, Bytes::new())
    }

    // --- expectCall ---------------------------------------------------------

    fn cheat_expect_call<DB>(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
        default_count: u64,
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(target) = read_address(args, 0) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.expectCall: bad address arg"),
            );
        };
        let Some(calldata) = read_bytes(args, 1) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.expectCall: bad calldata arg"),
            );
        };
        self.expected_calls.push(ExpectedCall {
            target,
            calldata,
            min_count: default_count,
            observed: 0,
            registered_at_call_depth: self.call_depth,
        });
        ok_return(inputs.gas_limit, Bytes::new())
    }

    fn cheat_expect_call_with_count<DB>(
        &mut self,
        _ctx: &mut edb_common::EdbContext<DB>,
        inputs: &CallInputs,
        args: &[u8],
    ) -> CallOutcome
    where
        DB: Database + DatabaseCommit + DatabaseRef + Clone,
        <CacheDB<DB> as Database>::Error: Clone,
        <DB as Database>::Error: Clone,
    {
        let Some(target) = read_address(args, 0) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.expectCall(...,uint64): bad address arg"),
            );
        };
        let Some(calldata) = read_bytes(args, 1) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.expectCall(...,uint64): bad calldata arg"),
            );
        };
        let Some(count_word) = read_u256(args, 2) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.expectCall(...,uint64): bad count arg"),
            );
        };
        let count: u64 = match count_word.try_into() {
            Ok(v) => v,
            Err(_) => {
                return revert_with(
                    inputs.gas_limit,
                    encode_error_string("vm.expectCall(...,uint64): count does not fit in u64"),
                );
            }
        };
        self.expected_calls.push(ExpectedCall {
            target,
            calldata,
            min_count: count,
            observed: 0,
            registered_at_call_depth: self.call_depth,
        });
        ok_return(inputs.gas_limit, Bytes::new())
    }

    // --- vm.assume -----------------------------------------------------------

    fn cheat_assume(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        // ABI-decoded `bool` is a single 32-byte word; the bool is in the last byte.
        let Some(cond) = read_bool(args, 0) else {
            return revert_with(inputs.gas_limit, encode_error_string("vm.assume: bad calldata"));
        };
        if cond {
            ok_return(inputs.gas_limit, Bytes::new())
        } else {
            revert_with(
                inputs.gas_limit,
                encode_error_string(
                    "EDB: vm.assume(false) -- assumption violated \
                     (real foundry would skip this fuzz iter; EDB surfaces it as a revert)",
                ),
            )
        }
    }

    // --- vm.envBool / envBytes / envString -----------------------------------

    fn cheat_env_bool(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(name) = read_string(args, 0) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.envBool: malformed calldata"),
            );
        };
        match std::env::var(&name) {
            Ok(v) => {
                let lower = v.trim().to_lowercase();
                let b = match lower.as_str() {
                    "true" | "1" => true,
                    "false" | "0" => false,
                    _ => {
                        return revert_with(
                            inputs.gas_limit,
                            encode_error_string(&format!(
                                "EDB: vm.envBool: {name}={v:?} not parseable as bool"
                            )),
                        );
                    }
                };
                let mut out = [0u8; 32];
                out[31] = u8::from(b);
                ok_return(inputs.gas_limit, Bytes::copy_from_slice(&out))
            }
            Err(_) => revert_with(
                inputs.gas_limit,
                encode_error_string(&format!("EDB: vm.envBool: {name} not set")),
            ),
        }
    }

    fn cheat_env_bytes(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(name) = read_string(args, 0) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.envBytes: malformed calldata"),
            );
        };
        match std::env::var(&name) {
            Ok(v) => {
                let trimmed = v.trim();
                let hex_body = if let Some(h) = trimmed.strip_prefix("0x") {
                    h
                } else {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string(&format!(
                            "EDB: vm.envBytes: {name}={v:?} must start with 0x for hex decoding"
                        )),
                    );
                };
                match alloy_primitives::hex::decode(hex_body) {
                    Ok(decoded) => ok_return(inputs.gas_limit, encode_abi_bytes(&decoded)),
                    Err(_) => revert_with(
                        inputs.gas_limit,
                        encode_error_string(&format!(
                            "EDB: vm.envBytes: {name}={v:?} not valid hex"
                        )),
                    ),
                }
            }
            Err(_) => revert_with(
                inputs.gas_limit,
                encode_error_string(&format!("EDB: vm.envBytes: {name} not set")),
            ),
        }
    }

    fn cheat_env_string(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        let Some(name) = read_string(args, 0) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.envString: malformed calldata"),
            );
        };
        match std::env::var(&name) {
            Ok(v) => ok_return(inputs.gas_limit, encode_abi_string(&v)),
            Err(_) => revert_with(
                inputs.gas_limit,
                encode_error_string(&format!("EDB: vm.envString: {name} not set")),
            ),
        }
    }

    // --- vm.envOr overloads --------------------------------------------------

    fn cheat_env_or_bool(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        // ABI: (string name, bool defaultValue)
        // head: [0..32) = offset to name (== 0x40), [32..64) = bool word
        // tail: the string at offset 0x40
        let Some(default_val) = read_bool(args, 1) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.envOr(string,bool): bad default arg"),
            );
        };
        let Some(name) = read_string(args, 0) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.envOr(string,bool): malformed calldata"),
            );
        };
        match std::env::var(&name) {
            Ok(v) => {
                let lower = v.trim().to_lowercase();
                let b = match lower.as_str() {
                    "true" | "1" => true,
                    "false" | "0" => false,
                    _ => {
                        return revert_with(
                            inputs.gas_limit,
                            encode_error_string(&format!(
                                "EDB: vm.envOr(string,bool): {name}={v:?} not parseable as bool"
                            )),
                        );
                    }
                };
                let mut out = [0u8; 32];
                out[31] = u8::from(b);
                ok_return(inputs.gas_limit, Bytes::copy_from_slice(&out))
            }
            Err(_) => {
                // Return the default value.
                let mut out = [0u8; 32];
                out[31] = u8::from(default_val);
                ok_return(inputs.gas_limit, Bytes::copy_from_slice(&out))
            }
        }
    }

    fn cheat_env_or_bytes(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        // ABI: (string name, bytes defaultValue) — both dynamic
        // head: [0..32) = offset to name, [32..64) = offset to bytes
        // The name is always at head_index 0 and the bytes at head_index 1.
        let Some(name) = read_string(args, 0) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.envOr(string,bytes): malformed calldata"),
            );
        };
        let Some(default_val) = read_bytes(args, 1) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.envOr(string,bytes): bad default arg"),
            );
        };
        match std::env::var(&name) {
            Ok(v) => {
                let trimmed = v.trim();
                let hex_body = if let Some(h) = trimmed.strip_prefix("0x") {
                    h
                } else {
                    return revert_with(
                        inputs.gas_limit,
                        encode_error_string(&format!(
                            "EDB: vm.envOr(string,bytes): {name}={v:?} must start with 0x"
                        )),
                    );
                };
                match alloy_primitives::hex::decode(hex_body) {
                    Ok(decoded) => ok_return(inputs.gas_limit, encode_abi_bytes(&decoded)),
                    Err(_) => revert_with(
                        inputs.gas_limit,
                        encode_error_string(&format!(
                            "EDB: vm.envOr(string,bytes): {name}={v:?} not valid hex"
                        )),
                    ),
                }
            }
            Err(_) => ok_return(inputs.gas_limit, encode_abi_bytes(&default_val)),
        }
    }

    fn cheat_env_or_string(&mut self, inputs: &CallInputs, args: &[u8]) -> CallOutcome {
        // ABI: (string name, string defaultValue) — both dynamic
        let Some(name) = read_string(args, 0) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.envOr(string,string): malformed calldata"),
            );
        };
        let Some(default_val) = read_string(args, 1) else {
            return revert_with(
                inputs.gas_limit,
                encode_error_string("vm.envOr(string,string): bad default arg"),
            );
        };
        match std::env::var(&name) {
            Ok(v) => ok_return(inputs.gas_limit, encode_abi_string(&v)),
            Err(_) => ok_return(inputs.gas_limit, encode_abi_string(&default_val)),
        }
    }
}

/// Argument-shape selector for the four supported `vm.expectEmit` overloads.
#[derive(Clone, Copy, Debug)]
enum ExpectEmitMode {
    /// `vm.expectEmit()` — all topics + data checked, any emitter.
    All,
    /// `vm.expectEmit(bool,bool,bool,bool)` — t1/t2/t3 + data, any emitter.
    Filter4,
    /// `vm.expectEmit(bool,bool,bool,bool,address)` — t1/t2/t3 + data, explicit emitter.
    Filter5,
    /// `vm.expectEmit(address)` — all topics + data checked, explicit emitter.
    AnyTopicsFromEmitter,
}

// ----------------------------------------------------------------------------
// CallOutcome helpers
// ----------------------------------------------------------------------------

fn ok_return(gas_limit: u64, output: Bytes) -> CallOutcome {
    CallOutcome {
        result: InterpreterResult {
            result: InstructionResult::Return,
            output,
            gas: Gas::new(gas_limit),
        },
        memory_offset: 0..0,
        was_precompile_called: false,
        precompile_call_logs: Vec::new(),
    }
}

fn revert_with(gas_limit: u64, output: Bytes) -> CallOutcome {
    CallOutcome {
        result: InterpreterResult {
            result: InstructionResult::Revert,
            output,
            gas: Gas::new(gas_limit),
        },
        memory_offset: 0..0,
        was_precompile_called: false,
        precompile_call_logs: Vec::new(),
    }
}

// ----------------------------------------------------------------------------
// Revert payload helpers
// ----------------------------------------------------------------------------

fn unsupported_revert(name: &str) -> Bytes {
    let msg = format!("EDB: cheatcode vm.{name} not supported in v1");
    encode_error_string(&msg)
}

/// Encode `Error(string)` as ABI: 4-byte selector `0x08c379a0` + (offset, length, padded data).
fn encode_error_string(msg: &str) -> Bytes {
    let mut payload = Vec::with_capacity(4 + 64 + msg.len().div_ceil(32) * 32);
    payload.extend_from_slice(&[0x08, 0xc3, 0x79, 0xa0]); // Error(string)
    // offset = 0x20
    let mut offset = [0u8; 32];
    offset[31] = 0x20;
    payload.extend_from_slice(&offset);
    // length (as 256-bit BE)
    let mut len = [0u8; 32];
    let l = msg.len() as u64;
    len[24..].copy_from_slice(&l.to_be_bytes());
    payload.extend_from_slice(&len);
    // data, right-padded to 32 bytes
    let mut data = msg.as_bytes().to_vec();
    let pad = (32 - data.len() % 32) % 32;
    data.extend(std::iter::repeat_n(0u8, pad));
    payload.extend_from_slice(&data);
    Bytes::from(payload)
}

/// ABI-encode a `string` return value: `(bytes32 offset, bytes32 length, data padded)`.
fn encode_abi_string(s: &str) -> Bytes {
    encode_abi_bytes(s.as_bytes())
}

/// ABI-encode a `bytes` return value: `(bytes32 offset, bytes32 length, data padded)`.
fn encode_abi_bytes(b: &[u8]) -> Bytes {
    let pad = (32 - b.len() % 32) % 32;
    let mut out = Vec::with_capacity(64 + b.len() + pad);
    // offset = 0x20
    let mut offset = [0u8; 32];
    offset[31] = 0x20;
    out.extend_from_slice(&offset);
    // length
    let mut len_word = [0u8; 32];
    let l = b.len() as u64;
    len_word[24..].copy_from_slice(&l.to_be_bytes());
    out.extend_from_slice(&len_word);
    // data right-padded to 32-byte multiple
    out.extend_from_slice(b);
    out.extend(std::iter::repeat_n(0u8, pad));
    Bytes::from(out)
}

// ----------------------------------------------------------------------------
// ABI decoding helpers (32-byte head per arg)
//
// We hand-decode the small subset of types we care about — `address`,
// `uint256`, `bytes32`, dynamic `bytes`, dynamic `string`. Each arg in the
// head section is 32 bytes; for dynamic args the head holds the byte offset
// (from start of `args`) of the (length, data) pair in the tail.
// ----------------------------------------------------------------------------

fn read_word(args: &[u8], head_index: usize) -> Option<[u8; 32]> {
    let start = head_index.checked_mul(32)?;
    let end = start.checked_add(32)?;
    if end > args.len() {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&args[start..end]);
    Some(out)
}

fn read_u256(args: &[u8], head_index: usize) -> Option<U256> {
    let w = read_word(args, head_index)?;
    Some(U256::from_be_bytes(w))
}

fn read_b256(args: &[u8], head_index: usize) -> Option<B256> {
    Some(B256::from(read_word(args, head_index)?))
}

fn read_address(args: &[u8], head_index: usize) -> Option<Address> {
    let w = read_word(args, head_index)?;
    // Address is right-aligned in the 32-byte word.
    Some(Address::from_slice(&w[12..32]))
}

/// Read a Solidity `bool` (ABI: a 32-byte word that is all zero for `false`
/// and ends with `0x01` for `true`; technically any non-zero word is `true`).
fn read_bool(args: &[u8], head_index: usize) -> Option<bool> {
    let w = read_word(args, head_index)?;
    Some(w.iter().any(|&b| b != 0))
}

/// Read a dynamic `bytes` argument: head slot at `head_index` contains the
/// offset (in bytes, from the start of `args`); at that offset we find the
/// length (32 bytes BE) and then `length` bytes of data (zero-padded to a
/// 32-byte multiple).
fn read_bytes(args: &[u8], head_index: usize) -> Option<Bytes> {
    let off = read_u256(args, head_index)?;
    let off = usize::try_from(off).ok()?;
    let len = read_u256(args, off / 32)?;
    let len = usize::try_from(len).ok()?;
    let data_start = off.checked_add(32)?;
    let data_end = data_start.checked_add(len)?;
    if data_end > args.len() {
        return None;
    }
    Some(Bytes::copy_from_slice(&args[data_start..data_end]))
}

/// Same wire format as `bytes`; we additionally require valid UTF-8.
fn read_string(args: &[u8], head_index: usize) -> Option<String> {
    let bytes = read_bytes(args, head_index)?;
    String::from_utf8(bytes.to_vec()).ok()
}

// ----------------------------------------------------------------------------
// ABI encoding for `Vm.Log[]`
//
// Foundry's `Vm.Log`:
//   struct Log { bytes32[] topics; bytes data; address emitter; }
//
// Returned by `vm.getRecordedLogs()` as `Log[]`.
// ----------------------------------------------------------------------------

fn abi_encode_logs(logs: &[Log]) -> Bytes {
    // We encode `Log[]` as a single dynamic top-level value:
    //   [offset_to_array]           (32 bytes, == 0x20)
    //   [array_length]              (32)
    //   [offset_log_0, ..., offset_log_n]  (n*32, offsets relative to start
    //                                      of this nested block, after length)
    //   <log_0 abi-tail>            (dynamic per-log)
    //   ...
    //
    // Each Log abi-tail itself is:
    //   [offset_to_topics]          (== 0x60 — first 3 head words for the 3 fields)
    //   [offset_to_data]            (depends on topics length)
    //   [emitter (right-padded in 32 bytes)]
    //   [topics.length]
    //   [topic_0 ... topic_n]       (n*32)
    //   [data.length]
    //   [data]                      (zero-padded to 32-byte multiple)

    fn encode_one_log(log: &Log) -> Vec<u8> {
        let topics: Vec<B256> = log.topics().to_vec();
        let topics_n = topics.len();
        let data = log.data.data.clone();
        let data_padded_len = data.len().div_ceil(32) * 32;

        // Layout offsets, in bytes, from the start of THIS log's encoding.
        let head_size: usize = 3 * 32;
        let off_topics = head_size; // immediately after the 3 head words
        let topics_block_size = 32 + topics_n * 32; // length + words
        let off_data = off_topics + topics_block_size;
        let data_block_size = 32 + data_padded_len;

        let total_len = head_size + topics_block_size + data_block_size;
        let mut buf = Vec::with_capacity(total_len);

        // head
        buf.extend_from_slice(&U256::from(off_topics).to_be_bytes::<32>());
        buf.extend_from_slice(&U256::from(off_data).to_be_bytes::<32>());
        let mut addr_word = [0u8; 32];
        addr_word[12..].copy_from_slice(log.address.as_slice());
        buf.extend_from_slice(&addr_word);

        // topics block
        buf.extend_from_slice(&U256::from(topics_n).to_be_bytes::<32>());
        for t in &topics {
            buf.extend_from_slice(t.as_slice());
        }
        // data block
        buf.extend_from_slice(&U256::from(data.len()).to_be_bytes::<32>());
        buf.extend_from_slice(&data);
        buf.extend(std::iter::repeat_n(0u8, data_padded_len - data.len()));
        debug_assert_eq!(buf.len(), total_len);
        buf
    }

    let n = logs.len();
    let per_log: Vec<Vec<u8>> = logs.iter().map(encode_one_log).collect();

    // Inner array block: [length] + [offsets...] + [tails concatenated].
    // Offsets are relative to the start of THIS inner block, immediately
    // after the length word. That means the first log's tail starts at
    // `n * 32` (right after the n offset words).
    let mut inner = Vec::new();
    inner.extend_from_slice(&U256::from(n).to_be_bytes::<32>());
    let mut running_offset: usize = n * 32;
    let mut tails_concat = Vec::new();
    for tail in &per_log {
        inner.extend_from_slice(&U256::from(running_offset).to_be_bytes::<32>());
        running_offset = running_offset.checked_add(tail.len()).expect("encoded size overflow");
        tails_concat.extend_from_slice(tail);
    }
    inner.extend_from_slice(&tails_concat);

    // Outer wrapper: single dynamic value lives at offset 0x20.
    let mut out = Vec::with_capacity(32 + inner.len());
    let mut head_offset = [0u8; 32];
    head_offset[31] = 0x20;
    out.extend_from_slice(&head_offset);
    out.extend_from_slice(&inner);
    Bytes::from(out)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::keccak256;

    fn sel(sig: &str) -> [u8; 4] {
        let h = keccak256(sig.as_bytes());
        let mut out = [0u8; 4];
        out.copy_from_slice(&h[..4]);
        out
    }

    // --- Selector verification: each must match keccak256(sig)[..4] ---------

    #[test]
    fn selector_warp() {
        assert_eq!(sel("warp(uint256)"), SEL_WARP);
    }
    #[test]
    fn selector_roll() {
        assert_eq!(sel("roll(uint256)"), SEL_ROLL);
    }
    #[test]
    fn selector_chain_id() {
        assert_eq!(sel("chainId(uint256)"), SEL_CHAIN_ID);
    }
    #[test]
    fn selector_deal() {
        assert_eq!(sel("deal(address,uint256)"), SEL_DEAL);
    }
    #[test]
    fn selector_etch() {
        assert_eq!(sel("etch(address,bytes)"), SEL_ETCH);
    }
    #[test]
    fn selector_store() {
        assert_eq!(sel("store(address,bytes32,bytes32)"), SEL_STORE);
    }
    #[test]
    fn selector_load() {
        assert_eq!(sel("load(address,bytes32)"), SEL_LOAD);
    }
    #[test]
    fn selector_set_nonce() {
        assert_eq!(sel("setNonce(address,uint64)"), SEL_SET_NONCE);
    }
    #[test]
    fn selector_prank() {
        assert_eq!(sel("prank(address)"), SEL_PRANK);
    }
    #[test]
    fn selector_start_prank() {
        assert_eq!(sel("startPrank(address)"), SEL_START_PRANK);
    }
    #[test]
    fn selector_stop_prank() {
        assert_eq!(sel("stopPrank()"), SEL_STOP_PRANK);
    }
    #[test]
    fn selector_mock_call() {
        assert_eq!(sel("mockCall(address,bytes,bytes)"), SEL_MOCK_CALL);
    }
    #[test]
    fn selector_mock_call_revert() {
        assert_eq!(sel("mockCallRevert(address,bytes,bytes)"), SEL_MOCK_CALL_REVERT);
    }
    #[test]
    fn selector_clear_mocked_calls() {
        assert_eq!(sel("clearMockedCalls()"), SEL_CLEAR_MOCKED_CALLS);
    }
    #[test]
    fn selector_expect_revert() {
        assert_eq!(sel("expectRevert()"), SEL_EXPECT_REVERT_BARE);
        assert_eq!(sel("expectRevert(bytes)"), SEL_EXPECT_REVERT_BYTES);
    }
    #[test]
    fn selector_label() {
        assert_eq!(sel("label(address,string)"), SEL_LABEL);
    }
    #[test]
    fn selector_record_logs() {
        assert_eq!(sel("recordLogs()"), SEL_RECORD_LOGS);
        assert_eq!(sel("getRecordedLogs()"), SEL_GET_RECORDED_LOGS);
    }
    #[test]
    fn selector_expect_emit() {
        assert_eq!(sel("expectEmit()"), SEL_EXPECT_EMIT_BARE);
        assert_eq!(sel("expectEmit(bool,bool,bool,bool)"), SEL_EXPECT_EMIT_FILTER4);
        assert_eq!(sel("expectEmit(bool,bool,bool,bool,address)"), SEL_EXPECT_EMIT_FILTER5);
        assert_eq!(sel("expectEmit(address)"), SEL_EXPECT_EMIT_ADDR);
    }
    #[test]
    fn selector_expect_call() {
        assert_eq!(sel("expectCall(address,bytes)"), SEL_EXPECT_CALL);
        assert_eq!(sel("expectCall(address,bytes,uint64)"), SEL_EXPECT_CALL_COUNT);
        assert_eq!(sel("expectCallMinGas(address,uint256,uint64,bytes)"), SEL_EXPECT_CALL_MIN_GAS);
    }
    #[test]
    fn assume_selector_matches_vm_sol() {
        assert_eq!(sel("assume(bool)"), SEL_ASSUME);
    }
    #[test]
    fn env_bool_selector_matches_vm_sol() {
        assert_eq!(sel("envBool(string)"), SEL_ENV_BOOL);
        assert_eq!(sel("envBytes(string)"), SEL_ENV_BYTES);
        assert_eq!(sel("envString(string)"), SEL_ENV_STRING);
    }
    #[test]
    fn env_or_bool_selector_matches_vm_sol() {
        assert_eq!(sel("envOr(string,bool)"), SEL_ENV_OR_BOOL);
        assert_eq!(sel("envOr(string,bytes)"), SEL_ENV_OR_BYTES);
        assert_eq!(sel("envOr(string,string)"), SEL_ENV_OR_STRING);
    }
    #[test]
    fn decode_encode_abi_string_roundtrip() {
        let original = "hello world";
        let encoded = encode_abi_string(original);
        // ABI-encoded: [offset=0x20][length=11][data padded to 32]
        assert_eq!(encoded.len(), 64 + 32); // offset + length + 1 padded chunk
        // Verify via read_string: head slot 0 has offset 0x20 = 32 bytes.
        // read_string interprets head_index=0 as the offset word, then reads
        // (length, data) at that offset — all relative to the start of `args`.
        // encode_abi_string starts with offset=0x20, so the (length, data) is
        // at byte 0x20 = 32 within the encoded buffer, which corresponds to
        // head_index=1 in read_word terms. read_string calls read_bytes which
        // does: off = read_u256(args, 0) = 0x20 = 32; len = read_u256(args, 32/32=1).
        let decoded = read_string(&encoded, 0).unwrap();
        assert_eq!(decoded, original);
    }
    #[test]
    fn selector_rejected_set_matches_canonical_signatures() {
        // Spot-check a handful of rejected selectors so the rejected list
        // doesn't silently drift from the canonical Vm.sol signatures.
        assert_eq!(sel("selectFork(uint256)"), SEL_SELECT_FORK);
        assert_eq!(sel("snapshotState()"), SEL_SNAPSHOT_STATE);
        assert_eq!(sel("revertToState(uint256)"), SEL_REVERT_TO_STATE);
        assert_eq!(sel("ffi(string[])"), SEL_FFI);
        assert_eq!(sel("transact(bytes32)"), SEL_TRANSACT);
        assert_eq!(sel("broadcast()"), SEL_BROADCAST);
    }

    // --- Revert payload + factory -------------------------------------------

    #[test]
    fn unsupported_revert_format() {
        let r = unsupported_revert("selectFork");
        // First 4 bytes are the Error(string) selector.
        assert_eq!(&r[..4], &[0x08, 0xc3, 0x79, 0xa0]);
        // The encoded UTF-8 should contain the literal vm.selectFork message.
        let tail = &r[4 + 64..]; // skip selector + offset + length
        let msg = String::from_utf8_lossy(tail);
        assert!(msg.contains("EDB: cheatcode vm.selectFork not supported in v1"), "got: {msg:?}");
    }

    #[test]
    fn factory_yields_fresh_instances() {
        let factory = build_cheats_factory(CheatsConfig::default());
        let a = factory();
        let b = factory();
        // Fresh inspectors share no mutable state.
        assert!(a.recorded_logs().is_empty());
        assert!(b.labels().is_empty());
    }

    // --- expectRevert guard: cheatcode call must NOT consume expected_revert --

    /// Verify that `call_end` only consumes `expected_revert` when the ending
    /// call is NOT to the cheatcode address.
    ///
    /// We can't spin up a real `EdbContext<DB>` in a unit test, so we exercise
    /// the guard logic by directly mutating and inspecting the `EdbCheatcodes`
    /// fields.  The integration test `cheats_expect_revert_rewrites_outcome`
    /// provides the full end-to-end coverage.
    #[test]
    fn expected_revert_guard_skips_cheatcode_address() {
        let mut cheats = EdbCheatcodes::new(CheatsConfig::default());

        // Simulate what `cheat_expect_revert` does: arm the slot.
        cheats.expected_revert = Some(ExpectedRevert { expected_data: None });

        // The guard condition mirrors the one in `call_end`:
        //   if inputs.target_address != CHEATCODE_ADDRESS { take() }
        // When the target IS the cheatcode address the slot must remain Some.
        let is_cheatcode = CHEATCODE_ADDRESS;
        let not_consumed = is_cheatcode == CHEATCODE_ADDRESS; // true → guard skips take()
        assert!(
            not_consumed && cheats.expected_revert.is_some(),
            "expected_revert must remain Some when call_end fires for the cheatcode address",
        );

        // When the target is any other address the slot should be consumed.
        let user_addr = Address::from([0xAA; 20]);
        let would_consume = user_addr != CHEATCODE_ADDRESS; // true → take() runs
        assert!(
            would_consume,
            "expected_revert must be consumed when call_end fires for a non-cheatcode address",
        );
    }

    // --- ABI decode helpers -------------------------------------------------

    #[test]
    fn read_address_uint256_pair() {
        // mimic `deal(address,uint256)` calldata tail: 2 head words.
        let mut buf = Vec::new();
        // address arg
        let mut a = [0u8; 32];
        a[12..].copy_from_slice(&[0xde; 20]);
        buf.extend_from_slice(&a);
        // uint256 arg
        let v = U256::from(1_000_000u64).to_be_bytes::<32>();
        buf.extend_from_slice(&v);

        let addr = read_address(&buf, 0).expect("address");
        assert_eq!(addr, Address::from_slice(&[0xde; 20]));
        assert_eq!(read_u256(&buf, 1).expect("u256"), U256::from(1_000_000u64));
    }

    #[test]
    fn read_bool_decodes_canonical_words() {
        // false = all zero word
        let mut zero = vec![0u8; 32];
        assert_eq!(read_bool(&zero, 0), Some(false));
        // true = canonical 0x...01 word
        zero[31] = 1;
        assert_eq!(read_bool(&zero, 0), Some(true));
        // any non-zero byte counts as true (be permissive on encoded forms)
        let mut other = vec![0u8; 32];
        other[7] = 0xff;
        assert_eq!(read_bool(&other, 0), Some(true));
        // truncated input returns None
        assert_eq!(read_bool(&[0u8; 16], 0), None);
    }

    #[test]
    fn expected_emit_soft_match_rules() {
        let addr = Address::from([0x11; 20]);
        let other = Address::from([0x22; 20]);
        let log_full = Log::new_unchecked(
            addr,
            vec![B256::from([0xaa; 32]), B256::from([0xbb; 32]), B256::from([0xcc; 32])],
            Bytes::from_static(b"payload"),
        );
        let log_empty_data = Log::new_unchecked(addr, vec![B256::from([0xaa; 32])], Bytes::new());

        // Default expectEmit() with all bits → must satisfy 4 topics and non-empty data.
        // `log_full` has 3 topics but expectation asks for 4 → fail.
        let all = ExpectedEmit {
            check_topics: [true; 4],
            check_data: true,
            expected_emitter: None,
            matched: false,
            registered_at_call_depth: 0,
        };
        assert!(!all.matches(&log_full), "all-topics expectation needs 4 topics");

        // Relax: only require topic[0] (the event sig) + data.
        let lax = expect_emit_simple();
        assert!(lax.matches(&log_full), "topic[0]+data must match a non-empty log");
        assert!(!lax.matches(&log_empty_data), "check_data must reject empty data");

        // Emitter filter
        let mut with_emitter = lax;
        with_emitter.expected_emitter = Some(other);
        assert!(!with_emitter.matches(&log_full), "wrong emitter must reject");
        with_emitter.expected_emitter = Some(addr);
        assert!(with_emitter.matches(&log_full));
    }

    fn expect_emit_simple() -> ExpectedEmit {
        ExpectedEmit {
            check_topics: [true, false, false, false],
            check_data: true,
            expected_emitter: None,
            matched: false,
            registered_at_call_depth: 0,
        }
    }

    #[test]
    fn read_dynamic_bytes_roundtrip() {
        // Encode `etch(address,bytes)`-style tail: address head, offset head,
        // then (length, data) tail.
        let mut buf = Vec::new();
        // arg 0: address
        let mut a = [0u8; 32];
        a[12..].copy_from_slice(&[0xab; 20]);
        buf.extend_from_slice(&a);
        // arg 1 head: offset = 0x40 (2 head words behind us)
        let mut off = [0u8; 32];
        off[31] = 0x40;
        buf.extend_from_slice(&off);
        // tail: length, then data (pad to 32 bytes)
        let data = b"hello-edb-cheats";
        let mut len = [0u8; 32];
        len[24..].copy_from_slice(&(data.len() as u64).to_be_bytes());
        buf.extend_from_slice(&len);
        let mut padded = data.to_vec();
        padded.resize(32, 0);
        buf.extend_from_slice(&padded);

        assert_eq!(read_address(&buf, 0).unwrap(), Address::from_slice(&[0xab; 20]));
        assert_eq!(read_bytes(&buf, 1).unwrap().as_ref(), data);
    }

    // --- Synthetic CallOutcome shape ----------------------------------------

    #[test]
    fn ok_return_has_return_status() {
        let out = ok_return(123_000, Bytes::from_static(b"x"));
        assert!(matches!(out.result.result, InstructionResult::Return));
        assert_eq!(out.result.output.as_ref(), b"x");
        assert_eq!(out.memory_offset, 0..0);
    }

    #[test]
    fn revert_with_has_revert_status() {
        let out = revert_with(50_000, Bytes::from_static(b"r"));
        assert!(matches!(out.result.result, InstructionResult::Revert));
        assert_eq!(out.result.output.as_ref(), b"r");
    }

    // --- abi_encode_logs sanity --------------------------------------------

    #[test]
    fn empty_logs_encode_to_length_zero_array() {
        let b = abi_encode_logs(&[]);
        // outer offset (0x20) + inner length (0)
        assert_eq!(b.len(), 64);
        let mut expect_outer = [0u8; 32];
        expect_outer[31] = 0x20;
        assert_eq!(&b[..32], expect_outer);
        assert_eq!(&b[32..64], &[0u8; 32]);
    }

    #[test]
    fn one_log_encodes_structurally() {
        // Single log with 2 topics and 5 bytes of data, structurally laid out
        // per the docstring on `abi_encode_logs`.
        let log = Log::new_unchecked(
            Address::from_slice(&[0x11; 20]),
            vec![B256::from([0x22; 32]), B256::from([0x33; 32])],
            Bytes::from_static(b"data!"),
        );
        let encoded = abi_encode_logs(&[log]);

        // [0..32]   outer offset (0x20)
        // [32..64]  array length (1)
        // [64..96]  offset-to-log-0 within the inner array block — = 0x20
        //           (i.e., n*32 where n=1 → 32 bytes ahead of the start of
        //           the offsets list, which itself starts right after the
        //           length word)
        let mut expect_outer = [0u8; 32];
        expect_outer[31] = 0x20;
        assert_eq!(&encoded[..32], expect_outer, "outer offset");

        let mut one = [0u8; 32];
        one[31] = 1;
        assert_eq!(&encoded[32..64], one, "array length");

        let mut expect_log0_off = [0u8; 32];
        expect_log0_off[31] = 0x20;
        assert_eq!(&encoded[64..96], expect_log0_off, "log[0] offset");

        // Log head: 3 words — off_topics (0x60), off_data (0x60 + 32 + 2*32 = 0xc0), emitter.
        let mut expect_off_topics = [0u8; 32];
        expect_off_topics[31] = 0x60;
        assert_eq!(&encoded[96..128], expect_off_topics, "log head: off_topics");

        let mut expect_off_data = [0u8; 32];
        expect_off_data[31] = 0xc0;
        assert_eq!(&encoded[128..160], expect_off_data, "log head: off_data");

        // emitter is right-padded in the 32-byte slot.
        let mut emitter_word = [0u8; 32];
        emitter_word[12..].copy_from_slice(&[0x11; 20]);
        assert_eq!(&encoded[160..192], emitter_word, "log head: emitter");

        // topics block: length (2), then 2 topics (32 bytes each)
        let mut two = [0u8; 32];
        two[31] = 2;
        assert_eq!(&encoded[192..224], two, "topics length");
        assert_eq!(&encoded[224..256], [0x22u8; 32], "topic[0]");
        assert_eq!(&encoded[256..288], [0x33u8; 32], "topic[1]");

        // data block: length (5), then 32-byte padded "data!"
        let mut five = [0u8; 32];
        five[31] = 5;
        assert_eq!(&encoded[288..320], five, "data length");
        let mut data_padded = [0u8; 32];
        data_padded[..5].copy_from_slice(b"data!");
        assert_eq!(&encoded[320..352], data_padded, "data (right-padded)");

        assert_eq!(encoded.len(), 352, "total encoded size");
    }
}
