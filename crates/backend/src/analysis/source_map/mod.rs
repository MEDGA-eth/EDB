pub mod debug_unit;
pub mod source_label;

use debug_unit::{DebugUnitAnlaysis, DebugUnits};
use eyre::{OptionExt, Result};
use foundry_compilers::artifacts::Bytecode;

use crate::{
    artifact::deploy::DeployArtifact,
    utils::opcode::{IcPcMap, PcIcMap},
};
use source_label::{SourceLabelAnalysis, SourceLabels};

const CONSTRUCTOR_IDX: usize = 0;
const DEPLOYED_IDX: usize = 1;

/// The analysis result store.
#[derive(Default)]
pub struct AnalysisStore<'a> {
    /// The debugging units.
    debug_units: Option<DebugUnits>,

    /// Constructor/Deployed Bytecode.
    bytecode: Option<[&'a Bytecode; 2]>,

    /// Constructor/Deployed ic_pc_map.
    ic_pc_map: Option<[IcPcMap; 2]>,

    /// Constructor/Deployed pc_ic_map.
    pc_ic_map: Option<[PcIcMap; 2]>,

    /// Constructor/Deployed source labels.
    source_labels: Option<[SourceLabels; 2]>,
}

macro_rules! store_getter {
    ($name:ident, $type:ty) => {
        pub fn $name(&self) -> Result<&$type> {
            self.$name.as_ref().ok_or_else(|| eyre::eyre!("no {} found", stringify!($type)))
        }
    };
}

impl<'a> AnalysisStore<'a> {
    store_getter!(debug_units, DebugUnits);
    store_getter!(source_labels, [SourceLabels; 2]);
    store_getter!(bytecode, [&Bytecode; 2]);
    store_getter!(ic_pc_map, [IcPcMap; 2]);
    store_getter!(pc_ic_map, [PcIcMap; 2]);

    pub fn init(artifact: &'a DeployArtifact) -> Result<Self> {
        let deployed_bytecode = artifact
            .evm
            .deployed_bytecode
            .as_ref()
            .and_then(|b| b.bytecode.as_ref())
            .ok_or_eyre("no deployed bytecode found")?;
        let construction_bytecode =
            artifact.evm.bytecode.as_ref().ok_or_eyre("no construction bytecode found")?;

        let deployed_ic_pc_map =
            IcPcMap::new(deployed_bytecode.bytes().ok_or_eyre("no code found")?.as_ref());
        let construction_ic_pc_map =
            IcPcMap::new(construction_bytecode.bytes().ok_or_eyre("no code found")?.as_ref());

        let deployed_pc_ic_map =
            PcIcMap::new(deployed_bytecode.bytes().ok_or_eyre("no code found")?.as_ref());
        let construction_pc_ic_map =
            PcIcMap::new(construction_bytecode.bytes().ok_or_eyre("no code found")?.as_ref());

        Ok(Self {
            bytecode: Some([construction_bytecode, deployed_bytecode]),
            ic_pc_map: Some([construction_ic_pc_map, deployed_ic_pc_map]),
            pc_ic_map: Some([construction_pc_ic_map, deployed_pc_ic_map]),
            ..Default::default()
        })
    }
}

/// A delicate analysis of the source map of a deployment artifact.
pub struct SourceMapAnalysis {}

impl SourceMapAnalysis {
    /// Analyze the source map of a compilation artifact.
    pub fn analyze(artifact: &DeployArtifact) -> Result<()> {
        let mut store = AnalysisStore::init(artifact)?;

        // Step 1. collect primitive debugging units.
        DebugUnitAnlaysis::analyze(artifact, &mut store)?;

        // Step 2. analyze the source labels.
        SourceLabelAnalysis::analyze(artifact, &mut store)?;

        Ok(())
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

    fn run_test(chain: Chain, addr: Address) -> Result<()> {
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

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_via_ir() {
        run_test(
            Chain::mainnet(),
            Address::from_str("0x6cc61ff5b01dc1904f280a11c8f5cd3c0a72dbb6").unwrap(),
        )
        .unwrap();
    }
}
