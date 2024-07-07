pub mod debug_unit;
pub mod naive_alignment;
pub mod source_label;

use debug_unit::{DebugUnitAnlaysis, DebugUnits};
use eyre::Result;
use serde::{Deserialize, Serialize};

use crate::artifact::deploy::DeployArtifact;
use naive_alignment::AlignmentAnalysis as NaiveAlignmentAnalysis;
use source_label::SourceLabelAnalysis;

/// The alignment algorithm of the source map.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SourceMapAlignment {
    /// Naive alignment.
    Naive,
}

/// A more reliable source map analysis.
pub struct SourceMapAnalysis {}

impl SourceMapAnalysis {
    /// Analyze the source map of a compilation artifact.
    pub fn analyze(artifact: &DeployArtifact, align: SourceMapAlignment) -> Result<DebugUnits> {
        // Step 1. collect primitive debugging units.
        let units = DebugUnitAnlaysis::analyze(artifact)?;

        // Step 2. analyze the source labels.
        let labels = SourceLabelAnalysis::analyze(artifact, &units)?;

        /// Step 3. align the source map.
        match align {
            SourceMapAlignment::Naive => {
                NaiveAlignmentAnalysis::analyze(artifact, &units, &labels)?;
            }
        }

        Ok(units)
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use alloy_chains::Chain;
    use alloy_primitives::Address;
    use edb_utils::cache::Cache;
    use eyre::OptionExt;
    use serial_test::serial;

    use crate::artifact::deploy::DeployArtifact;

    use super::*;

    fn run_test(chain: Chain, addr: Address) -> Result<DebugUnits> {
        // load cached artifacts
        let cache_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/cache/backend")
            .join(chain.to_string());
        let cache = Cache::new(cache_root, None)?;
        let artifact: DeployArtifact =
            cache.load_cache(addr.to_string()).ok_or_eyre("missing cached artifact")?;

        SourceMapAnalysis::analyze(&artifact, SourceMapAlignment::Naive)
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_usd() {
        run_test(
            Chain::mainnet(),
            Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap(),
        )
        .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_pepe() {
        run_test(
            Chain::mainnet(),
            Address::from_str("0x6982508145454Ce325dDbE47a25d4ec3d2311933").unwrap(),
        )
        .unwrap();
    }
}
