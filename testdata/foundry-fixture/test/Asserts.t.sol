// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

/// Exercises the assertion-family cheatcodes end-to-end.
///
/// Most forge-std wrappers (`assertTrue`, `assertEq`, etc.) short-circuit on
/// the passing branch and forward to the `vm.assert*` cheatcode only on
/// failure. To regression-cover C2-1 and C2-4 from the Round-2 audit, several
/// tests below bypass the wrapper and call `vm.assert*` DIRECTLY on the
/// passing branch — exactly the path that previously reverted with "EDB:
/// vm.assert* called with insufficient calldata".
contract Asserts is Test {
    /// Sanity: the forge-std wrapper short-circuits to a no-op on the passing
    /// branch, so this test exercises the inlined `if (left != right)` guard
    /// only (no cheatcode invocation).
    function testAssertEqUintPasses() public pure {
        assertEq(uint256(1), uint256(1));
    }

    /// C2-1 regression: direct call to `vm.assertTrue(true)`. The single-bool
    /// ABI calldata is exactly 32 bytes; before the fix the inspector rejected
    /// the call with the bouncer error, so this test failed under EDB even
    /// though `forge test` accepts it. With C2-1 fixed the cheatcode returns
    /// successfully and the test passes.
    function testAssertTrueDirectCall() public pure {
        vm.assertTrue(true);
    }

    /// Direct call to `vm.assertFalse(false)` — symmetric to the above.
    function testAssertFalseDirectCall() public pure {
        vm.assertFalse(false);
    }

    /// Cross-sign signed comparison. `assertGt(5, -1)` exercises the
    /// two's-complement sign-flip branch in `cheat_assert`'s
    /// SEL_ASSERT_GT_I256 arm — left is positive, right is negative, so
    /// the result must be `true`.
    function testAssertGtSignedCrossSign() public pure {
        assertGt(int256(5), int256(-1));
    }

    /// Passing assertion with a custom message — uses forge-std's
    /// `assertEq(uint, uint, string)` wrapper, which short-circuits on the
    /// passing branch, so this primarily exercises the wrapper inlining
    /// rather than the cheatcode. Kept for completeness.
    function testAssertEqWithCustomMessagePass() public pure {
        assertEq(uint256(2), uint256(2), "should match");
    }

    /// C2-4 regression — DIRECT call to `vm.assertTrue(true, "ok")`.
    /// The 2-head-word + string-tail layout is the one whose error-message
    /// decoder was previously broken. We're on the passing branch here so
    /// no error message is constructed, but the call must NOT trip the
    /// "insufficient calldata" guard (which would fire for >= 32-byte
    /// args when the bouncer demanded 64). Combined with the unit-test
    /// `assert_true_with_message_decodes_offset_correctly` covering the
    /// failing branch, this locks the decoder fix in end-to-end.
    function testAssertTrueWithMessagePass() public pure {
        vm.assertTrue(true, "ok");
    }
}
