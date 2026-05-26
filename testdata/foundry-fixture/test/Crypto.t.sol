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

    /// `vm.publicKeyP256(1)` returns the P-256 generator point `(Gx, Gy)`
    /// (the public key for private key `1`). These coordinates are taken
    /// from FIPS 186-4 §D.1.2.3.
    function testPublicKeyP256SkOneMatchesGenerator() public pure {
        uint256 expectedGx =
            0x6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296;
        uint256 expectedGy =
            0x4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5;
        (uint256 x, uint256 y) = vm.publicKeyP256(1);
        require(x == expectedGx, "vm.publicKeyP256(1) X coordinate wrong");
        require(y == expectedGy, "vm.publicKeyP256(1) Y coordinate wrong");
    }

    /// `vm.signP256(sk, digest)` produces a 64-byte (r, s) signature with
    /// s normalized to the low half of the curve order. We don't pin exact
    /// (r, s) bytes — the deterministic-k may differ across crate revs —
    /// but we check:
    ///   1. r != 0 (a zero r means the cheatcode failed silently).
    ///   2. s is low-half normalized: `s <= n/2`. P-256 n/2 has its high
    ///      uint128 well below `0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF`, so the
    ///      first 16 bytes of `s` must compare <= `n/2`'s first 16 bytes.
    function testSignP256ReturnsCanonicalLowSSignature() public pure {
        uint256 sk = 1;
        bytes32 digest = keccak256("hello edb p256");
        (bytes32 r, bytes32 s) = vm.signP256(sk, digest);
        require(r != bytes32(0), "vm.signP256 returned r=0");
        // Low-s: s as a uint256 must be <= n/2 where n is the P-256 curve order.
        // n/2 = 0x7fffffff80000000_7fffffffffffffff_de737d56_d38bcf42_79dce5617e3192a8
        uint256 nHalf =
            0x7fffffff800000007fffffffffffffffde737d56d38bcf4279dce5617e3192a8;
        require(uint256(s) <= nHalf, "vm.signP256 returned high-s signature");
    }
}
