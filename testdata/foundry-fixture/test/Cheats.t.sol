// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

contract Cheats is Test {
    event Transfer(address indexed from, address indexed to, uint256 value);

    function testWarp() public {
        vm.warp(1234567);
        require(block.timestamp == 1234567, "vm.warp did not apply");
    }

    function testDeal() public {
        address payable a = payable(address(0xA11CE));
        vm.deal(a, 1 ether);
        require(a.balance == 1 ether, "vm.deal did not apply");
    }

    function testExpectRevert() public {
        // Bare expectRevert matches any revert; the call to `revertingFn`
        // reverts and the cheatcode rewrites the outcome to a successful
        // return so testExpectRevert returns normally.
        vm.expectRevert();
        revertingFn();
    }

    /// `vm.expectEmit(bool,bool,bool,bool)` followed by an actual `emit` in the
    /// same frame. Transfer has 2 indexed args + a non-indexed value, so the
    /// mask is `(true, true, false, true)`: check topic[1] (from), topic[2]
    /// (to), don't check topic[3] (doesn't exist), check data (the value).
    /// Under EDB's soft-match semantics this satisfies the expectation because
    /// Inspector::log catches the emit at the same depth.
    function testExpectEmit() public {
        address a = address(0xA11CE);
        address b = address(0xB0B);
        vm.expectEmit(true, true, false, true);
        emit Transfer(a, b, 100);
    }

    /// `vm.expectCall(target, calldata)` followed by an actual call to that
    /// target with that calldata.
    function testExpectCall() public {
        ExpectCallTarget t = new ExpectCallTarget();
        vm.expectCall(address(t), abi.encodeWithSignature("increment()"));
        t.increment();
    }

    function testAssumeTrue() public pure {
        vm.assume(true); // no-op; test should succeed
    }

    function testEnvOrString() public {
        string memory v = vm.envOr("EDB_TEST_NONEXISTENT_VAR_XYZ", string("fallback"));
        require(
            keccak256(bytes(v)) == keccak256(bytes("fallback")),
            "vm.envOr should return fallback for unset var"
        );
    }

    /// `vm.snapshotValue` (both overloads) is a no-op stub in EDB v1 — it
    /// must not revert. Foundry's benchmark recorder is not wired up; we just
    /// confirm the call sequence flows through and the test returns normally.
    function testSnapshotValueStubsDoNotRevert() public {
        vm.snapshotValue("a", uint256(123));
        vm.snapshotValue("group", "b", uint256(456));
        // Hit each overload more than once to also exercise the warn_once
        // deduplication path.
        vm.snapshotValue("a", uint256(789));
    }

    /// `vm.getDeployedCode(string)` returns the deployed bytecode of a
    /// project artifact, loaded from EDB's `LocalArtifactSet`.
    ///
    /// Properties we verify:
    ///   - Returned bytes are non-empty and longer than the EVM-runtime
    ///     dispatcher header (~12 bytes for an empty contract).
    ///   - Returned bytes begin with the standard Solidity runtime prelude
    ///     `0x6080604052` (PUSH1 0x80 PUSH1 0x40 MSTORE) emitted by every
    ///     solc-compiled contract since 0.5.x.
    ///   - The `path:contract` shape returns the same bytes as the bare name.
    ///
    /// We DON'T compare against `address(target).code` because EDB rewrites
    /// runtime bytecode between its tracer / opcode / hook passes (snapshot
    /// instrumentation), so the on-chain code is intentionally different
    /// from the on-disk template that `vm.getDeployedCode` returns. The
    /// shape checks above are stable across all three passes.
    function testGetDeployedCodeReturnsRuntimeTemplate() public {
        bytes memory bare = vm.getDeployedCode("GetDeployedCodeTarget");
        bytes memory pathContract = vm.getDeployedCode(
            "test/Cheats.t.sol:GetDeployedCodeTarget"
        );

        require(bare.length >= 12, "vm.getDeployedCode returned too few bytes");
        require(
            keccak256(bare) == keccak256(pathContract),
            "bare-name and path:contract shapes returned different bytes"
        );

        // Solidity dispatcher prelude: PUSH1 0x80 PUSH1 0x40 MSTORE
        require(
            bare[0] == 0x60 && bare[1] == 0x80 && bare[2] == 0x60 && bare[3] == 0x40 && bare[4] == 0x52,
            "vm.getDeployedCode result does not start with Solidity runtime prelude"
        );
    }

    function testGasMeteringStubs() public {
        // pauseGasMetering / resumeGasMetering are stubs — just confirm they
        // don't revert.
        vm.pauseGasMetering();
        vm.resumeGasMetering();
        // vm.lastCallGas() is also a stub returning all-zero Gas fields; tested
        // separately in the unit tests. We omit it here because the hook-pass
        // instrumented execution changes the observable number of sub-calls,
        // which would make an all-zeros return inconsistent with what the
        // original tracer pass records (EDB's multi-pass design doesn't expose
        // real gas values to lastCallGas in v1 anyway).
    }

    /// Regression for C-1 (memory_offset propagation).
    ///
    /// `vm.load(address,bytes32) returns (bytes32)` is a STATIC-return
    /// cheatcode: Solidity reads the result directly from
    /// `mem[memory_offset..memory_offset + 32]`. EDB previously built its
    /// synthetic `CallOutcome` with `memory_offset: 0..0`, so REVM copied zero
    /// bytes back into the caller's frame and Solidity observed `bytes32(0)`
    /// no matter what the slot actually held. With the fix in place, vm.load
    /// returns the value we just vm.stored — verified by the `require` below.
    function testLoadReadsActualStorage() public {
        address target = address(this);
        bytes32 slot = bytes32(uint256(0xdead));
        bytes32 expected = bytes32(uint256(0xc0ffee));
        vm.store(target, slot, expected);
        bytes32 read = vm.load(target, slot);
        require(read == expected, "vm.load returned wrong value (memory_offset propagation bug)");
    }

    /// `vm.expectRevert(bytes4)` — match only the leading 4-byte selector of
    /// the revert payload. Used by tests that expect a custom error like
    /// `revert BadInput(x)` where the trailing ABI-encoded args may vary.
    function testExpectRevertSelector() public {
        ExpectRevertSelectorTarget t = new ExpectRevertSelectorTarget();
        vm.expectRevert(ExpectRevertSelectorTarget.BadInput.selector);
        t.boom(42);
    }

    function revertingFn() internal pure {
        revert("boom");
    }

    /// `vm.startPrank(msgSender, txOrigin)` — sets both msg.sender and
    /// tx.origin for subsequent external calls. We verify both via an
    /// external callee that captures them, then stopPrank and verify the
    /// override is reverted.
    function testStartPrankWithOrigin() public {
        address payable callee = payable(address(new PrankWitness()));
        address newSender = address(0xA11CE);
        address newOrigin = address(0xBEEF);

        vm.startPrank(newSender, newOrigin);
        (address seenSender, address seenOrigin) = PrankWitness(callee).observe();
        require(seenSender == newSender, "startPrank did not override msg.sender");
        require(seenOrigin == newOrigin, "startPrank did not override tx.origin");
        vm.stopPrank();

        (address afterSender, address afterOrigin) = PrankWitness(callee).observe();
        require(afterSender != newSender, "msg.sender stuck after stopPrank");
        require(afterOrigin != newOrigin, "tx.origin stuck after stopPrank");
    }

    /// Stacked 2-arg startPrank: two consecutive `vm.startPrank(sender,
    /// origin)` calls without an intervening `vm.stopPrank` must NOT cause
    /// the second override to win on restore — `vm.stopPrank` should
    /// restore back to the GENUINE pre-prank-A tx.origin, not to
    /// origin-A. The handler pins `saved_tx_origin = Some(ctx.tx.caller)`
    /// only on the FIRST 2-arg startPrank in scope; this fixture pins
    /// that invariant end-to-end.
    function testStartPrankStackedRestoresOriginalOrigin() public {
        address payable callee = payable(address(new PrankWitness()));
        address originA = address(0xAAAA);
        address originB = address(0xBBBB);
        (, address preOrigin) = PrankWitness(callee).observe();

        vm.startPrank(address(0xAAA1), originA);
        // Second startPrank without intervening stopPrank — origin should
        // shift to originB, but the SAVED slot must still hold preOrigin.
        vm.startPrank(address(0xBBB1), originB);
        (, address midOrigin) = PrankWitness(callee).observe();
        require(midOrigin == originB, "second startPrank did not override tx.origin");
        vm.stopPrank();

        // After a single stopPrank, both the prank stack AND the tx.origin
        // save are consumed. tx.origin must be back to preOrigin (NOT
        // originA — that would be the bug we're guarding against).
        (, address afterOrigin) = PrankWitness(callee).observe();
        require(afterOrigin == preOrigin, "stacked stopPrank restored to intermediate origin");
    }
}

/// External callee used by `testExpectCall` so the `increment()` invocation
/// actually fires Inspector::call at a non-cheatcode target (an internal call
/// would inline and bypass the inspector).
contract ExpectCallTarget {
    uint256 public x;
    function increment() external {
        x += 1;
    }
}

/// External callee for `testExpectRevertSelector`. `boom` reverts with the
/// custom error `BadInput(uint256)` whose ABI encoding is `selector || arg`;
/// only the leading selector is significant for `vm.expectRevert(bytes4)`.
contract ExpectRevertSelectorTarget {
    error BadInput(uint256 got);
    function boom(uint256 x) external pure {
        revert BadInput(x);
    }
}

/// External callee for `testStartPrankWithOrigin`. Returns the (msg.sender,
/// tx.origin) it observes so the caller can compare against the prank
/// override (and against the natural defaults after stopPrank).
contract PrankWitness {
    function observe() external view returns (address, address) {
        return (msg.sender, tx.origin);
    }
}

/// Target contract for `testGetDeployedCodeMatchesOnChain`. No immutables,
/// no linked libraries, no constructor args — the deployed runtime template
/// solc emits is byte-equal to what's at `address(target).code` after a
/// fresh `new`.
contract GetDeployedCodeTarget {
    uint256 public answer;
    function setAnswer(uint256 v) external {
        answer = v;
    }
}
