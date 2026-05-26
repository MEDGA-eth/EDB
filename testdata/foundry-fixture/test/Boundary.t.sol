// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

contract Boundary is Test {
    function testSelectForkIsRejected() public {
        // EDB should make this call revert with "EDB: cheatcode vm.selectFork not supported in v1"
        vm.selectFork(0);
    }
}
