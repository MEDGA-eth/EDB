// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

/// Coverage for the secp256k1 cheatcodes `vm.addr` and `vm.sign`.
///
/// These tests are independent of foundry's full crypto suite — they pin EDB's
/// implementations against the canonical sk=1 test vector and exercise the
/// `sign` -> `ecrecover` roundtrip that real-world tests (solady's
/// ERC1271Test, OpenZeppelin's SignatureCheckerTest, etc.) rely on.
contract Crypto is Test {
    /// Canonical foundry test vector: secret key 1 maps to a well-known address.
    /// 0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf
    address constant SK1_ADDRESS = 0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf;

    function testAddrFromKey() public pure {
        address derived = vm.addr(1);
        require(derived == SK1_ADDRESS, "vm.addr(1) did not return the canonical sk=1 address");
    }

    /// `vm.sign(sk, digest)` returns (v, r, s). Feeding that triple through
    /// the EVM's `ecrecover` precompile must recover the address `vm.addr(sk)`
    /// derived. This is the operational invariant downstream tests check.
    function testSignAndEcrecover() public pure {
        uint256 sk = 1;
        bytes32 digest = keccak256("hello edb");
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(sk, digest);
        address recovered = ecrecover(digest, v, r, s);
        require(
            recovered == vm.addr(sk),
            "ecrecover(vm.sign(sk, digest)) did not recover vm.addr(sk)"
        );
    }
}
