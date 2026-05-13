// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

contract Cheats is Test {
    event Transfer(address indexed from, address indexed to, uint256 value);

    function testWarp() public {
        vm.warp(1234567);
        require(block.timestamp == 1234567, "vm.warp did not apply");
    }

    /// `vm.fee(uint256)` — mutate `block.basefee`. Subsequent `BASEFEE`
    /// opcodes read the new value. We use `7 gwei` so the saturating
    /// `u256 -> u64` conversion is exercised on a value safely inside
    /// `u64::MAX`.
    function testFee() public {
        vm.fee(7 gwei);
        require(block.basefee == 7 gwei, "vm.fee did not apply");
    }

    /// `vm.txGasPrice(uint256)` — mutate `tx.gasprice`. The next read of
    /// `tx.gasprice` (which Solidity reads via the GASPRICE opcode) should
    /// reflect the new value.
    function testTxGasPrice() public {
        vm.txGasPrice(123_456_789);
        require(tx.gasprice == 123_456_789, "vm.txGasPrice did not apply");
    }

    /// `vm.parseJsonBool(json, ".x")` — minimal JSONPath access to extract a
    /// bool leaf. `Cheats::testParseJsonBool` exercises both `true` and
    /// `false` leaves.
    function testParseJsonBool() public {
        bool t = vm.parseJsonBool('{"x": true}', ".x");
        require(t, "vm.parseJsonBool true leaf wrong");
        bool f = vm.parseJsonBool('{"x": false}', ".x");
        require(!f, "vm.parseJsonBool false leaf wrong");
    }

    /// `vm.parseJsonString` — extract a JSON string leaf.
    function testParseJsonString() public {
        string memory s = vm.parseJsonString('{"k": "hello"}', ".k");
        require(
            keccak256(bytes(s)) == keccak256(bytes("hello")),
            "vm.parseJsonString wrong"
        );
    }

    /// `vm.parseJsonUint` — accept both number and `"0x"`-prefixed hex string.
    function testParseJsonUint() public {
        uint256 a = vm.parseJsonUint('{"n": 12345}', ".n");
        require(a == 12345, "vm.parseJsonUint number wrong");
        uint256 b = vm.parseJsonUint('{"n": "0x100"}', ".n");
        require(b == 256, "vm.parseJsonUint hex string wrong");
    }

    /// `vm.parseJsonInt` — signed leaf.
    function testParseJsonInt() public {
        int256 v = vm.parseJsonInt('{"n": -42}', ".n");
        require(v == -42, "vm.parseJsonInt negative wrong");
    }

    /// `vm.parseJsonBytes32` — hex-encoded leaf decoded to 32 bytes.
    function testParseJsonBytes32() public {
        bytes32 b = vm.parseJsonBytes32(
            '{"k": "0x000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"}',
            ".k"
        );
        require(
            b == bytes32(hex"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"),
            "vm.parseJsonBytes32 wrong"
        );
    }

    /// `vm.parseJsonAddress` — EIP-55 / lowercase hex string leaf decoded to address.
    function testParseJsonAddress() public {
        address a = vm.parseJsonAddress(
            '{"who": "0x52908400098527886E0F7030069857D2E4169EE7"}',
            ".who"
        );
        require(
            a == address(0x52908400098527886E0F7030069857D2E4169EE7),
            "vm.parseJsonAddress wrong"
        );
    }

    /// `vm.toString` — six overloads converting primitive types to their
    /// Solidity-canonical string form. Each branch hashes the result against
    /// the expected literal so a regression surfaces as a clean require
    /// failure with a descriptive message.
    function testToStringFamily() public {
        // uint256 -> decimal
        require(
            keccak256(bytes(vm.toString(uint256(123)))) == keccak256(bytes("123")),
            "vm.toString(uint256) wrong"
        );

        // int256 -> signed decimal (negative + positive)
        require(
            keccak256(bytes(vm.toString(int256(-7)))) == keccak256(bytes("-7")),
            "vm.toString(int256) negative wrong"
        );
        require(
            keccak256(bytes(vm.toString(int256(42)))) == keccak256(bytes("42")),
            "vm.toString(int256) positive wrong"
        );

        // bool -> "true" / "false"
        require(
            keccak256(bytes(vm.toString(true))) == keccak256(bytes("true")),
            "vm.toString(bool) true wrong"
        );
        require(
            keccak256(bytes(vm.toString(false))) == keccak256(bytes("false")),
            "vm.toString(bool) false wrong"
        );

        // bytes -> "0x" + lowercase hex
        bytes memory raw = hex"deadbeef";
        require(
            keccak256(bytes(vm.toString(raw))) == keccak256(bytes("0xdeadbeef")),
            "vm.toString(bytes) wrong"
        );

        // bytes32 -> "0x" + 64 lowercase hex chars
        bytes32 w = bytes32(uint256(0xc0ffee));
        require(
            keccak256(bytes(vm.toString(w))) ==
                keccak256(bytes("0x0000000000000000000000000000000000000000000000000000000000c0ffee")),
            "vm.toString(bytes32) wrong"
        );

        // address -> EIP-55 checksum.
        // 0x52908400098527886e0f7030069857d2e4169ee7 is one of the canonical
        // EIP-55 test vectors that flips to all-uppercase under the checksum,
        // so any case-sensitivity bug would surface here.
        address a = address(0x52908400098527886E0F7030069857D2E4169EE7);
        require(
            keccak256(bytes(vm.toString(a))) ==
                keccak256(bytes("0x52908400098527886E0F7030069857D2E4169EE7")),
            "vm.toString(address) wrong checksum"
        );
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

    /// `vm.getBlockNumber()` reads `ctx.block.number`. After `vm.roll(N)` the
    /// cheatcode must return `N` even though the cached `block.number` in
    /// Solidity would (without this dodge) be the compile-time captured value.
    function testGetBlockNumber() public {
        vm.roll(987_654);
        require(vm.getBlockNumber() == 987_654, "vm.getBlockNumber did not match vm.roll");
    }

    /// `vm.getNonce(address)` mirrors `vm.setNonce`'s journal-load path.
    /// Setting then reading the same account must round-trip exactly.
    function testGetNonceRoundtripsSetNonce() public {
        address acct = address(0xCAFE);
        vm.setNonce(acct, 42);
        require(vm.getNonce(acct) == 42, "vm.getNonce did not match vm.setNonce");
    }

    /// `vm.setBlockhash(blockNumber, hash)` installs a `BLOCKHASH` override.
    /// After the call, the EVM's `blockhash(blockNumber)` opcode must return
    /// the installed hash for any `blockNumber` in `[block.number - 256, block.number)`.
    function testSetBlockhash() public {
        // Move to a high block so we have room for an override 5 blocks back.
        vm.roll(1000);
        uint256 target = 995;
        bytes32 sentinel = bytes32(uint256(0xDEADBEEF));
        vm.setBlockhash(target, sentinel);
        require(blockhash(target) == sentinel, "vm.setBlockhash did not apply");
    }

    /// `vm.readLine(path)` returns the next line of the file each call (no
    /// trailing newline) and an empty string at EOF. The fixture under
    /// `test/fixtures/readline_lines.txt` is a 3-line file: alpha / beta / gamma.
    function testReadLineSequentialAndEof() public {
        string memory path = "test/fixtures/readline_lines.txt";
        string memory l1 = vm.readLine(path);
        string memory l2 = vm.readLine(path);
        string memory l3 = vm.readLine(path);
        string memory l4 = vm.readLine(path);
        require(keccak256(bytes(l1)) == keccak256(bytes("alpha")), "vm.readLine line 1 wrong");
        require(keccak256(bytes(l2)) == keccak256(bytes("beta")), "vm.readLine line 2 wrong");
        require(keccak256(bytes(l3)) == keccak256(bytes("gamma")), "vm.readLine line 3 wrong");
        require(bytes(l4).length == 0, "vm.readLine EOF should be empty");
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
