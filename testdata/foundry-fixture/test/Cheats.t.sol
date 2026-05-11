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

    function revertingFn() internal pure {
        revert("boom");
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
