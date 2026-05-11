// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

contract Cheats is Test {
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

    function revertingFn() internal pure {
        revert("boom");
    }
}
