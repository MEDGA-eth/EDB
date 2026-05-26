// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

// `Inner` is CREATEd by `NestedDeploy` inside its `setUp()`. The point of this
// fixture is to drive a *nested* CREATE (depth 2): the synthetic entrypoint
// CREATEs `NestedDeploy` (depth 1), whose constructor (via setUp) CREATEs
// `Inner` (depth 2). The integration test confirms hook snapshots fire for
// both contracts — regression guard for the Task 6.3 fix path.
contract Inner {
    uint256 public x;

    constructor(uint256 v) {
        x = v;
    }

    function bump() public {
        x++;
    }
}

contract NestedDeploy is Test {
    Inner internal child;

    function setUp() public {
        child = new Inner(7);
    }

    function testInnerWasDeployed() public {
        require(address(child) != address(0), "Inner not deployed");
        require(child.x() == 7, "Inner state wrong");
        child.bump();
        require(child.x() == 8, "bump didn't work");
    }
}
