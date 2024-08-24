pub mod debug_unit;
pub mod integrity;
pub mod source_label;

use std::rc::Rc;

use debug_unit::{DebugUnitAnlaysis, DebugUnits};
use eyre::{OptionExt, Result};
use foundry_compilers::artifacts::{sourcemap::SourceElement, Bytecode};
use integrity::IntegrityAnalsysis;

use crate::artifact::deploy::DeployArtifact;
use source_label::{SourceLabelAnalysis, SourceLabels};

const CONSTRUCTOR_IDX: usize = 0;
const DEPLOYED_IDX: usize = 1;

/// The refined source map analysis result.
pub struct RefinedSourceMap {
    type_id: usize,

    /// Debugging units.
    pub debug_units: Rc<DebugUnits>,

    /// Constructor/Deployed source labels.
    pub labels: SourceLabels,

    /// Constructor/Deployed source map.
    pub source_map: Vec<SourceElement>,

    /// Whether the source map is corrupted.
    pub is_corrupted: bool,
}

impl RefinedSourceMap {
    pub fn is_constructor(&self) -> bool {
        self.type_id == CONSTRUCTOR_IDX
    }

    pub fn is_deployed(&self) -> bool {
        self.type_id == DEPLOYED_IDX
    }
}

/// The analysis result store.
#[derive(Default, Clone)]
pub struct AnalysisStore<'a> {
    /// The debugging units.
    debug_units: Option<DebugUnits>,

    /// Constructor/Deployed Bytecode.
    bytecode: Option<[&'a Bytecode; 2]>,

    /// Constructor/Deployed source map.
    source_map: Option<[Vec<SourceElement>; 2]>,

    /// Constructor/Deployed source labels.
    source_labels: Option<[SourceLabels; 2]>,

    /// Whether the source map is corrupted.
    is_corrupted: Option<[bool; 2]>,
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
    store_getter!(source_map, [Vec<SourceElement>; 2]);

    pub fn init(artifact: &'a DeployArtifact) -> Result<Self> {
        let deployed_bytecode =
            artifact.deployed_bytecode().ok_or_eyre("no deployed bytecode found")?;
        let construction_bytecode =
            artifact.constructor_bytecode().ok_or_eyre("no construction bytecode found")?;

        let source_map = [
            construction_bytecode.source_map().ok_or_eyre("no source map found")??,
            deployed_bytecode.source_map().ok_or_eyre("no source map found")??,
        ];

        Ok(Self {
            bytecode: Some([construction_bytecode, deployed_bytecode]),
            source_map: Some(source_map),
            ..Default::default()
        })
    }

    pub fn produce(self) -> Result<[RefinedSourceMap; 2]> {
        let units = Rc::new(self.debug_units.ok_or_eyre("no debug units found")?);

        let [c_labels, d_labels] = self.source_labels.ok_or_eyre("no source labels found")?;
        let [c_map, d_map] = self.source_map.ok_or_eyre("no source map found")?;
        let [c_is_corrupted, d_is_corrupted] =
            self.is_corrupted.ok_or_eyre("no corruption information found")?;

        Ok([
            RefinedSourceMap {
                type_id: CONSTRUCTOR_IDX,
                debug_units: units.clone(),
                labels: c_labels,
                source_map: c_map,
                is_corrupted: c_is_corrupted,
            },
            RefinedSourceMap {
                type_id: DEPLOYED_IDX,
                debug_units: units,
                labels: d_labels,
                source_map: d_map,
                is_corrupted: d_is_corrupted,
            },
        ])
    }
}

/// A delicate analysis of the source map of a deployment artifact.
pub struct SourceMapAnalysis {}

impl SourceMapAnalysis {
    /// Analyze the source map of a compilation artifact.
    pub fn analyze(artifact: &DeployArtifact) -> Result<[RefinedSourceMap; 2]> {
        let mut store = AnalysisStore::init(artifact)?;

        // Step 0. analyze the integrity of the source map.
        IntegrityAnalsysis::analyze(artifact, &mut store)?;

        // Step 1. collect primitive debugging units.
        DebugUnitAnlaysis::analyze(artifact, &mut store)?;

        // Step 2. analyze the source labels.
        SourceLabelAnalysis::analyze(artifact, &mut store)?;

        store.produce()
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use alloy_chains::Chain;
    use alloy_primitives::Address;
    use edb_utils::cache::{Cache, EDBCache};
    use serial_test::serial;

    use crate::artifact::deploy::DeployArtifact;

    use super::*;

    fn run_test(chain: Chain, addr: Address) -> Result<[RefinedSourceMap; 2]> {
        // load cached artifacts
        let cache_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdata/cache/backend")
            .join(chain.to_string());
        let cache = EDBCache::new(Some(cache_root), None)?;
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

    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn test_no_via_ir_0_8_x() {
        run_test(
            Chain::mainnet(),
            Address::from_str("0x9aBB27581c2E46A114F8C367355851e0580e9703").unwrap(),
        )
        .unwrap();
    }
}
