// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

contract Basic is Test {
    function testTrivial() public pure {
        require(1 + 1 == 2, "math broken");
    }
}
