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

    function testRevertRestoresStorage() public {
        uint256 id = vm.snapshotState();
        c.set(42);
        require(c.value() == 42, "set worked");
        bool ok = vm.revertToState(id);
        require(ok, "revertToState returned true");
        require(c.value() == 7, "storage rolled back to setUp value");
    }

    function testRevertToUnknownReturnsFalse() public {
        bool ok = vm.revertToState(999);
        require(!ok, "unknown id returns false");
    }
}
