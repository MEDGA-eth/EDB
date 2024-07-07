use eyre::Result;

use super::{debug_unit::DebugUnits, source_label::{SourceLabel, SourceLabels}};
use crate::artifact::deploy::DeployArtifact;

pub struct AlignmentAnalysis {}

impl AlignmentAnalysis {
    pub fn analyze(
        _artifact: &DeployArtifact,
        _units: &DebugUnits,
        labels: &SourceLabels,
    ) -> Result<()> {
        debug!("analyzing source map alignment");

        Self::analyze_labels(&labels.construction);
        Self::analyze_labels(&labels.deployed);

        Ok(())
    }

    fn analyze_labels(labels: &[SourceLabel]) {
        let mut start = 0;

        for (end, label) in labels.iter().enumerate() {
            if label.is_intermessage_action() || label.is_interprocedural_jmp() {
                debug!("analyzing region: [{}..{}]", start, end);
                Self::analyze_region(&labels[start..=end]);
                start = end + 1;
            }
        }
    
        if start < labels.len() {
            debug!("analyzing region: [{}..{}]", start, labels.len() - 1);
            Self::analyze_region(&labels[start..]);
        }
    }

    fn analyze_region(labels: &[SourceLabel]) {
    }
}
