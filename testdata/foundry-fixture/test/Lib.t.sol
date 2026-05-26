// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

/// External library — a `public` function forces solc to emit a separately
/// deployed library + `__$hash$__` link placeholders in every caller's
/// bytecode (creation + deployed).
library MathLib {
    function addOne(uint256 x) public pure returns (uint256) {
        return x + 1;
    }
}

/// A contract-under-test that links against MathLib (the "contracts-under-test
/// use libraries" case — its instrumented bytecode must link or it reverts).
contract UsesLib {
    uint256 public total;

    function bump(uint256 x) public {
        total = MathLib.addOne(x);
    }
}

contract LibTest is Test {
    function testExternalLibraryLinks() public {
        // (1) The test contract ITSELF calls the external library — this makes
        // LibTest's own bytecode unlinked, the case that used to abort
        // discovery with "artifact missing deployed bytecode" (Aave-style).
        require(MathLib.addOne(41) == 42, "direct library call failed");

        // (2) A deployed contract-under-test also uses the library.
        UsesLib u = new UsesLib();
        u.bump(41);
        require(u.total() == 42, "library call via deployed contract failed");
    }
}
