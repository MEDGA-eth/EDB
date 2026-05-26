// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Codehash-alias normalization for `vm.etch`-aliased contracts.
//!
//! The engine's `codehash_to_canonical` index maps a runtime-bytecode hash to
//! the canonical artifact address that produced it, so source/analysis lookups
//! resolve even when a contract's code lives at a `vm.etch`'d alias address.
//!
//! The catch: the *same* contract is observed with two kinds of byte-level
//! noise that defeat a raw `keccak256` comparison.
//!
//!   1. **Trailing `0x00` padding.** REVM appends a variable number of STOP
//!      bytes when it analyzes legacy bytecode for execution; the pad length is
//!      an internal detail that shifts with EVM/DB state (e.g. whether external
//!      libraries are pre-etched into the fork).
//!   2. **CBOR metadata.** solc appends a metadata trailer (an IPFS/bzzr hash
//!      of the contract's metadata JSON, plus the compiler version). That JSON
//!      embeds the compiler *settings* — including the `libraries` map — so the
//!      identical contract compiled with vs. without a `settings.libraries`
//!      entry (EDB's entrypoint compile sets it; the main compile does not)
//!      gets a *different* metadata hash even though the executable code is
//!      byte-for-byte identical.
//!
//! Normalizing away both — strip trailing zeros, then strip the CBOR metadata
//! trailer — recovers the executable code body, which is stable across these
//! representations. Matching on the normalized form is an *exact* match on the
//! real code (deterministic), not a fuzzy similarity score. Two genuinely
//! different contracts only collide if their code bodies are byte-identical and
//! differ solely in metadata, in which case they behave identically anyway;
//! `entry().or_insert()` makes the (rare) tie deterministic.

use alloy_primitives::{Address, B256, keccak256};
use std::collections::HashMap;

/// Trim trailing `0x00` bytes from a runtime-bytecode blob.
pub fn strip_trailing_zeros(code: &[u8]) -> &[u8] {
    let end = code.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    &code[..end]
}

/// Trim solc's CBOR metadata trailer, if present.
///
/// Layout: `<code> <cbor map> <2-byte big-endian cbor length>`. The trailing
/// two bytes give the metadata length `L`; the CBOR block sits in the `L` bytes
/// before them and starts with a CBOR map header (`0xa1`/`0xa2`/`0xa3` for
/// solc). When the suffix doesn't describe a plausible CBOR map (e.g.
/// `bytecode_hash = "none"`, hand-rolled bytecode, or EOF), the input is
/// returned unchanged. Expects trailing padding to already be removed.
pub fn strip_metadata(code: &[u8]) -> &[u8] {
    if code.len() < 2 {
        return code;
    }
    let meta_len = ((code[code.len() - 2] as usize) << 8) | code[code.len() - 1] as usize;
    let Some(meta_start) = code.len().checked_sub(2 + meta_len) else {
        return code;
    };
    // solc emits a CBOR map with 2 or 3 entries; accept 1 too for safety.
    if matches!(code.get(meta_start), Some(0xa1..=0xa3)) {
        return &code[..meta_start];
    }
    code
}

/// Normalize a runtime-bytecode blob to its executable code body: strip REVM
/// trailing-zero padding, then the CBOR metadata trailer.
pub fn normalize_runtime_code(code: &[u8]) -> &[u8] {
    strip_metadata(strip_trailing_zeros(code))
}

/// Register `addr` under both the exact hash of `code` and the hash of its
/// normalized code body in a `codehash_to_canonical` index. `entry().or_insert()`
/// keeps first-insert-wins so callers control collision precedence by iteration
/// order. The normalized insert collapses into the exact one for code with no
/// metadata/padding to strip.
pub fn index_codehash(index: &mut HashMap<B256, Address>, code: &[u8], addr: Address) {
    index.entry(keccak256(code)).or_insert(addr);
    index.entry(keccak256(normalize_runtime_code(code))).or_insert(addr);
}

/// Resolve a runtime-bytecode blob to its canonical artifact address via the
/// codehash-alias index. Tries the exact hash first (fast path), then the
/// normalized code-body hash to absorb REVM padding and metadata differences.
pub fn resolve_canonical(index: &HashMap<B256, Address>, runtime_code: &[u8]) -> Option<Address> {
    if let Some(addr) = index.get(&keccak256(runtime_code)) {
        return Some(*addr);
    }
    let normalized = normalize_runtime_code(runtime_code);
    if normalized.len() != runtime_code.len()
        && let Some(addr) = index.get(&keccak256(normalized))
    {
        return Some(*addr);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_only_trailing_zeros() {
        assert_eq!(strip_trailing_zeros(&[1, 2, 3]), &[1, 2, 3]);
        assert_eq!(strip_trailing_zeros(&[1, 2, 3, 0, 0]), &[1, 2, 3]);
        assert_eq!(strip_trailing_zeros(&[0, 1, 0, 2, 0]), &[0, 1, 0, 2]);
        assert_eq!(strip_trailing_zeros(&[0, 0, 0]), &[] as &[u8]);
        assert_eq!(strip_trailing_zeros(&[]), &[] as &[u8]);
    }

    #[test]
    fn strips_solc_cbor_metadata() {
        // <code: 0x6001> <cbor map 0xa1 'a' 0x01 = 3 bytes> <len = 0x0003>
        let with_meta = [0x60, 0x01, 0xa1, 0x61, 0x01, 0x00, 0x03];
        assert_eq!(strip_metadata(&with_meta), &[0x60, 0x01]);
        // No valid CBOR trailer → unchanged.
        let no_meta = [0x60, 0x01, 0x00, 0x00, 0x05];
        assert_eq!(strip_metadata(&no_meta), &no_meta);
        // Too short → unchanged.
        assert_eq!(strip_metadata(&[0x33]), &[0x33]);
    }

    #[test]
    fn resolves_through_padding_and_metadata() {
        let canonical = Address::with_last_byte(0xAB);
        // Realistic shape: code body + 3-byte CBOR map + length suffix.
        let body = [0x60, 0x00, 0x33];
        let meta_a = [0xa1, 0x61, 0x01, 0x00, 0x03]; // metadata variant A
        let meta_b = [0xa1, 0x61, 0x02, 0x00, 0x03]; // metadata variant B (different hash)
        let code_a: Vec<u8> = body.iter().chain(&meta_a).copied().collect();
        let code_b: Vec<u8> = body.iter().chain(&meta_b).copied().collect();

        let mut index = HashMap::new();
        index_codehash(&mut index, &code_a, canonical);

        // Exact match on the indexed form.
        assert_eq!(resolve_canonical(&index, &code_a), Some(canonical));
        // A metadata-divergent twin (same code body) resolves via normalization.
        assert_eq!(resolve_canonical(&index, &code_b), Some(canonical));
        // REVM trailing-zero padding on top of either still resolves.
        let mut padded_b = code_b;
        padded_b.extend_from_slice(&[0, 0, 0, 0, 0]);
        assert_eq!(resolve_canonical(&index, &padded_b), Some(canonical));
        // A different code body does not resolve.
        let other: Vec<u8> = [0x60, 0x01, 0x33].iter().chain(&meta_a).copied().collect();
        assert_eq!(resolve_canonical(&index, &other), None);
    }

    #[test]
    fn resolves_metadataless_code_via_padding() {
        // bytecode_hash="none": no CBOR trailer, only REVM padding noise.
        let canonical = Address::with_last_byte(0xCD);
        let code: &[u8] = &[0x60, 0x01, 0x60, 0x02];
        let mut index = HashMap::new();
        index_codehash(&mut index, code, canonical);
        assert_eq!(resolve_canonical(&index, code), Some(canonical));
        assert_eq!(
            resolve_canonical(&index, &[0x60, 0x01, 0x60, 0x02, 0x00, 0x00]),
            Some(canonical)
        );
    }
}
