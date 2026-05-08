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

//! Ethereum mainnet hardfork specification ID mapping
//!
//! This module provides utilities to determine the correct SpecId (hardfork)
//! based on block numbers for Ethereum mainnet.

use revm::primitives::{
    eip4844::{BLOB_BASE_FEE_UPDATE_FRACTION_CANCUN, BLOB_BASE_FEE_UPDATE_FRACTION_PRAGUE},
    hardfork::SpecId,
};
use std::{collections::BTreeMap, sync::LazyLock};

/// Global BTreeMap for Ethereum mainnet hardfork specifications
/// The key is the starting block number for each hardfork
static MAINNET_HARDFORKS: LazyLock<BTreeMap<u64, SpecId>> = LazyLock::new(|| {
    [
        (0, SpecId::FRONTIER),
        (1_150_000, SpecId::HOMESTEAD),
        (2_463_000, SpecId::TANGERINE),
        (2_675_000, SpecId::SPURIOUS_DRAGON),
        (4_370_000, SpecId::BYZANTIUM),
        // Constantinople was planned but immediately replaced by Petersburg
        // Both activate at block 7_280_000, but Petersburg takes precedence
        (7_280_000, SpecId::PETERSBURG),
        (9_069_000, SpecId::ISTANBUL),
        (12_244_000, SpecId::BERLIN),
        (12_965_000, SpecId::LONDON),
        (13_773_000, SpecId::ARROW_GLACIER),
        (15_050_000, SpecId::GRAY_GLACIER),
        (15_537_394, SpecId::MERGE),
        (17_034_870, SpecId::SHANGHAI),
        (19_426_589, SpecId::CANCUN),
        // Pectra (Prague on EL) mainnet activation
        (22_431_084, SpecId::PRAGUE),
    ]
    .into_iter()
    .collect()
});

/// Get the SpecId for a given block number on Ethereum mainnet
///
/// This function uses a global BTreeMap to efficiently find the correct hardfork
/// specification for any given block number. It handles the special case
/// of Constantinople/Petersburg where Petersburg immediately replaced
/// Constantinople at the same block height.
pub fn get_mainnet_spec_id(block_number: u64) -> SpecId {
    // Find the last hardfork that started at or before the given block
    MAINNET_HARDFORKS
        .range(..=block_number)
        .last()
        .map(|(_, spec_id)| *spec_id)
        .unwrap_or(SpecId::FRONTIER)
}

/// Get hardfork information for a specific SpecId
pub fn get_hardfork_info(spec_id: SpecId) -> (&'static str, u64) {
    match spec_id {
        SpecId::FRONTIER => ("Frontier", 0),
        SpecId::HOMESTEAD => ("Homestead", 1_150_000),
        SpecId::TANGERINE => ("Tangerine Whistle", 2_463_000),
        SpecId::SPURIOUS_DRAGON => ("Spurious Dragon", 2_675_000),
        SpecId::BYZANTIUM => ("Byzantium", 4_370_000),
        SpecId::CONSTANTINOPLE => ("Constantinople", 7_280_000), // Note: Replaced by Petersburg
        SpecId::PETERSBURG => ("Petersburg", 7_280_000),
        SpecId::ISTANBUL => ("Istanbul", 9_069_000),
        SpecId::BERLIN => ("Berlin", 12_244_000),
        SpecId::LONDON => ("London", 12_965_000),
        SpecId::ARROW_GLACIER => ("Arrow Glacier", 13_773_000),
        SpecId::GRAY_GLACIER => ("Gray Glacier", 15_050_000),
        SpecId::MERGE => ("The Merge", 15_537_394),
        SpecId::SHANGHAI => ("Shanghai", 17_034_870),
        SpecId::CANCUN => ("Cancun", 19_426_589),
        SpecId::PRAGUE => ("Prague (Pectra EL)", 22_431_084),
        _ => ("Unknown", 0),
    }
}

/// Returns the blob base fee update fraction based on the spec id.
pub fn get_blob_base_fee_update_fraction_by_spec_id(spec: SpecId) -> u64 {
    if spec >= SpecId::PRAGUE {
        BLOB_BASE_FEE_UPDATE_FRACTION_PRAGUE
    } else {
        BLOB_BASE_FEE_UPDATE_FRACTION_CANCUN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mainnet_spec_id() {
        // Test genesis
        assert_eq!(get_mainnet_spec_id(0), SpecId::FRONTIER);
        assert_eq!(get_mainnet_spec_id(1), SpecId::FRONTIER);

        // Test Homestead
        assert_eq!(get_mainnet_spec_id(1_149_999), SpecId::FRONTIER);
        assert_eq!(get_mainnet_spec_id(1_150_000), SpecId::HOMESTEAD);
        assert_eq!(get_mainnet_spec_id(1_150_001), SpecId::HOMESTEAD);

        // Test Constantinople/Petersburg transition
        assert_eq!(get_mainnet_spec_id(7_279_999), SpecId::BYZANTIUM);
        assert_eq!(get_mainnet_spec_id(7_280_000), SpecId::PETERSBURG); // Petersburg, not Constantinople
        assert_eq!(get_mainnet_spec_id(7_280_001), SpecId::PETERSBURG);

        // Test recent hardforks
        assert_eq!(get_mainnet_spec_id(15_537_394), SpecId::MERGE);
        assert_eq!(get_mainnet_spec_id(17_034_870), SpecId::SHANGHAI);
        assert_eq!(get_mainnet_spec_id(19_426_589), SpecId::CANCUN);
        assert_eq!(get_mainnet_spec_id(22_431_084), SpecId::PRAGUE);

        // Test random blocks
        assert_eq!(get_mainnet_spec_id(20_000_000), SpecId::CANCUN);
        assert_eq!(get_mainnet_spec_id(u64::MAX), SpecId::PRAGUE);
    }
}
