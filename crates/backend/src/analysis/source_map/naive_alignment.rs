use eyre::Result;

use super::{debug_unit::DebugUnits, source_label::SourceLabels};
use crate::artifact::deploy::DeployArtifact;

pub struct AlignmentAnalysis {}

impl AlignmentAnalysis {
    pub fn analyze(
        artifact: &DeployArtifact,
        units: &DebugUnits,
        labels: &SourceLabels,
    ) -> Result<()> {
        debug!("analyzing source map alignment");

        Ok(())
    }
}
