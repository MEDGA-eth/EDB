pub mod debug_unit;
pub mod source_label;

use crate::artifact::deploy::DeployArtifact;
use debug_unit::{DebugUnitAnlaysis, DebugUnits};
use eyre::Result;
use source_label::SourceLabelAnalysis;

/// A more reliable source map analysis.
pub struct SourceMapAnalysis {}

impl SourceMapAnalysis {
    /// Analyze the source map of a compilation artifact.
    pub fn analyze(artifact: &DeployArtifact) -> Result<DebugUnits> {
        // Step 1. collect primitive debugging units.
        let units = DebugUnitAnlaysis::analyze(artifact)?;

        // Step 2. analyze the source labels.
        SourceLabelAnalysis::analyze(artifact, &units)?;

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

        SourceMapAnalysis::analyze(&artifact)
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
