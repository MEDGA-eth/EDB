use std::collections::{BTreeMap, BTreeSet};

use eyre::{bail, Result};
use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

use crate::{analysis::source_map::RefinedSourceMap, RuntimeAddress};

use super::{AnalyzedCallTrace, BlockNode, CalibrationPoint};

impl AnalyzedCallTrace {
    pub fn is_calibrated(&self) -> bool {
        self.calibrated
    }

    /// This function is used to calibrate the call trace by the source map. Specifically, it
    /// will pinpoint those opcode locations that are the ``likely'' first opcode of a statement or
    /// an inline assembly block. During the process, we may also merge some statements or inline
    /// assembly blocks that are fused together during compilation.
    pub fn calibrate_with_source(
        &mut self,
        source_map: &BTreeMap<RuntimeAddress, RefinedSourceMap>,
    ) -> Result<()> {
        // We first project the source labels into the call trace.
        self.project_source_labels(source_map)?;

        // At last, we apply the lazy updates to the call trace.
        self.apply_lazy_updates();

        Ok(())
    }

    /// This function label the first opcode of a statement or an inline assembly block with the
    /// corresponding source label.
    fn project_source_labels(
        &mut self,
        source_map: &BTreeMap<RuntimeAddress, RefinedSourceMap>,
    ) -> Result<()> {
        // This can be done in parallel.
        self.nodes.iter_mut().filter(|func| !func.is_discarded()).try_for_each(|func| {
            let source_map = match source_map.get(&func.addr) {
                Some(source_map) => source_map,
                None => return Ok(()),
            };

            if source_map.is_corrupted {
                // We do not need to calibrate the function if the source map is corrupted.
                return Ok(());
            }

            func.trace.iter_mut().try_for_each(|block| block.label_source(source_map))
        })
    }
}

impl BlockNode {
    fn label_source(&mut self, source_map: &RefinedSourceMap) -> Result<()> {
        // We first label the first opcode of a statement or an inline assembly block.
        let mut cur_label = None;
        for ic in self.start_ic..self.start_ic + self.inst_n {
            let c_label = &source_map.labels[ic];
            match cur_label {
                Some(label) if label != c_label => {
                    if c_label.is_source() {
                        self.calib.insert(ic, CalibrationPoint::Singleton(c_label.clone()));
                        cur_label = Some(c_label);
                    }
                }
                None if c_label.is_source() => {
                    self.calib.insert(ic, CalibrationPoint::Singleton(c_label.clone()));
                    cur_label = Some(c_label);
                }
                _ => {}
            }
        }

        // We then try to find the function of this block.
        let funcs: BTreeSet<_> = self
            .calib
            .values()
            .filter_map(|p| p.as_singleton().and_then(|l| l.function()))
            .collect();

        self.calib_modifiers = funcs
            .iter()
            .filter_map(|f| {
                if f.is_modifier().unwrap_or_else(|| panic!("this has to be a function unit")) {
                    Some((*f).clone())
                } else {
                    None
                }
            })
            .collect();
        debug!(addr=?self.addr, block=format!("{self}"), modifier_n=self.calib_modifiers.len(), "calibration points");

        let total_n = funcs.len();
        let func_n = total_n - self.calib_modifiers.len();

        if func_n == 1 {
            let func = funcs
                .iter()
                .find(|f| {
                    !f.is_modifier().unwrap_or_else(|| panic!("this has to be a function unit"))
                })
                .unwrap();
            self.calib_func = Some((*func).clone());
        } else if func_n > 1 {
            if cfg!(debug_assertions) {
                debug!(addr=?self.addr, block=format!("{self}"), "calibration points are not from the same function");
                for p in self.calib.values() {
                    if let CalibrationPoint::Singleton(label) = p {
                        debug!(label = format!("{label}"), "calibration point");
                    }
                }
            }
            bail!("the calibration points are not from the same function");
        } else {
            warn!(addr=?self.addr, block=format!("{self}"), "no calibration function found");
        }

        Ok(())
    }
}
