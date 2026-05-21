// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

/// Minimal counter — used as the impl whose bytecode gets etched at a
/// second address. Trivial source so the test reads cleanly; any
/// statement here produces a hook USID we can verify lands at BOTH
/// addresses.
contract Counter {
    uint256 public n;
    function inc() public {
        n += 1;
    }
}

contract EtchTest is Test {
    /// `vm.etch(target, address(impl).code)` installs the instrumented
    /// runtime of `impl` at `target` (in EDB Pass 3). The test calls
    /// `Counter(target).inc()` so hooks fire at the etched address.
    /// Without the codehash-alias fix, EDB drops these hooks because
    /// `analysis_results` has no entry for `target`. With the fix,
    /// hook snapshots fire correctly and the RPC `edb_getCode(target)`
    /// returns the Counter source.
    function testEtchedAddressProducesHookSnapshots() public {
        Counter impl = new Counter();
        address aliased = address(0xCAFEC0DE);
        vm.etch(aliased, address(impl).code);

        Counter(aliased).inc();
        require(Counter(aliased).n() == 1, "etched counter did not increment");

        Counter(aliased).inc();
        require(Counter(aliased).n() == 2, "etched counter did not increment twice");
    }
}
