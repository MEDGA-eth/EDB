// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

contract SnapCounter {
    uint256 public value;
    function set(uint256 v) external { value = v; }
}

contract SnapshotTest is Test {
    SnapCounter c;

    function setUp() public {
        c = new SnapCounter();
        c.set(7);
    }

    function testSnapshotReturnsMonotonicId() public {
        uint256 id1 = vm.snapshotState();
        uint256 id2 = vm.snapshotState();
        require(id2 > id1, "ids should be monotonic");
    }
}
