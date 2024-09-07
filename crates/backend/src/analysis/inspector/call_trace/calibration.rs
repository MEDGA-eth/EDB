use std::collections::{BTreeMap, BTreeSet};

use eyre::{bail, eyre, OptionExt, Result};
use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

use crate::{
    analysis::source_map::{debug_unit::DebugUnit, RefinedSourceMap},
    RuntimeAddress,
};

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

            let mut new_trace = Vec::with_capacity(func.trace.len());
            for mut block in func.trace.drain(..) {
                let funcs = block.label_source(source_map)?;
                if funcs.len() <= 1 {
                    // If there is at most one function, we can directly push the block into the new
                    // trace.
                    new_trace.push(block)
                } else {
                    // Otherwise, we need to split the block into multiple blocks.
                    // new_trace.extend(block.split_by_calibrated_funcs(source_map, funcs)?);
                }
            }
            func.trace = new_trace;

            Ok(())
        })
    }
}

impl BlockNode {
    fn clear_calibrations(&mut self) {
        self.calib.clear();
        self.calib_func = None;
        self.calib_modifiers.clear();
        self.calib_inlined.clear();
    }

    fn split_by_calibrated_funcs(
        mut self,
        source_map: &RefinedSourceMap,
        funcs: Vec<DebugUnit>,
    ) -> Result<Vec<Self>> {
        match funcs.len() {
            1 => {
                bail!("only one function is found, no need to split the block");
            }
            2 => {
                // TODO (ZZ): For now, we only plan to support the case where the two functions are
                // concatnated together. If there are more than two functions, we will support it in
                // the future.
                //
                // Solidity could even emit the JUMP opcode within the callsite (similar to function
                // inlining). In this case, we need to split the block into two. For
                // example, address: 0x3fc91a3afd70395cd496c647d5a6cc9d4b2b7fad,
                // block: ic[1317..1337].
                let [func1, func2] =
                    funcs.try_into().map_err(|_| eyre!("more than two functions"))?;
                debug!(addr=?self.addr, block=format!("{self}"), func1=format!("{func1}"), func2=format!("{func2}"), "splitting the block");

                for ic in self.start_ic..self.start_ic + self.inst_n {
                    let Some(c_label) = source_map.labels.get(ic) else {
                        break;
                    };

                    if c_label.function() != Some(&func2) {
                        continue;
                    }
                    debug!(addr=?self.addr, block=format!("{self}"), ic, "found the statement associated with the second function");

                    // Update instruction count.
                    let mut block2 = self.clone();
                    block2.start_ic = ic;
                    block2.inst_n = self.start_ic + self.inst_n - ic;
                    self.inst_n -= block2.inst_n;

                    // Update call-to information.
                    // At this point, we simply put a PLACEHOLDER for the first block's call-to
                    // information. The accurate call-to information will be updated in the next
                    // step when we re-construct the call trace.
                    self.call_to = Some(usize::MAX);

                    // Update the calibration point information.
                    self.clear_calibrations();
                    self.label_source(source_map)?;
                    debug_assert!(
                        self.calib_func == Some(func1),
                        "the first function is not calibrated correctly"
                    );

                    block2.clear_calibrations();
                    block2.label_source(source_map)?;
                    debug_assert!(
                        block2.calib_func == Some(func2),
                        "the second function is not calibrated correctly"
                    );

                    return Ok(vec![self, block2]);
                }

                bail!("statement associated with the second function is not found");
            }
            _ => {
                if cfg!(debug_assertions) {
                    debug!(addr=?self.addr, block=format!("{self}"), "calibration points are not from the same function");
                    for p in self.calib.values() {
                        if let CalibrationPoint::Singleton(label) = p {
                            debug!(label = format!("{label}"), "calibration point");
                        }
                    }
                }
                bail!("the calibration points are not from the same function");
            }
        }
    }

    fn label_source(&mut self, source_map: &RefinedSourceMap) -> Result<Vec<DebugUnit>> {
        // We first label the first opcode of a statement or an inline assembly block.
        let mut cur_label = None;
        trace!(addr=?self.addr, block=format!("{self}"), "calibrating");
        for ic in self.start_ic..self.start_ic + self.inst_n {
            // TODO (ZZ): maybe we need to record the statement tag information here.
            let Some(c_label) = source_map.labels.get(ic) else {
                // Source map may not cover all instructions. For example, the constructor of
                // 0x1f98431c8ad98523631ae4a59f267346ea31f984.
                break;
            };
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

        // An internal struct to store the information of a calculation function.
        #[derive(Default)]
        struct CalculationFunctionInfo {
            pub normal: Vec<DebugUnit>,
            pub pure: Vec<DebugUnit>,
            pub modifiers: Vec<DebugUnit>,
        }

        let info = funcs.into_iter().fold(CalculationFunctionInfo::default(), |mut info, func| {
            let meta = func.get_function_meta().expect("this has to be a function unit");
            match (meta.is_modifier, meta.is_pure) {
                (false, false) => info.normal.push(func.clone()),
                (false, true) => info.pure.push(func.clone()),
                (true, false) => info.modifiers.push(func.clone()),
                _ => debug_assert!(false, "modifier cannot be pure"),
            }
            info
        });

        self.calib_modifiers = info.modifiers;
        trace!(addr=?self.addr, block=format!("{self}"), modifier_n=self.calib_modifiers.len(), "calibration points");

        self.calib_inlined = info.pure;
        trace!(addr=?self.addr, block=format!("{self}"), inline_n=self.calib_inlined.len(), "calibration points");

        match info.normal.len() {
            0 => {
                trace!(addr=?self.addr, block=format!("{self}"), "no normal calibration points");
                Ok(Vec::new())
            }
            1 => {
                self.calib_func = info.normal.into_iter().next();
                self.calib_func
                    .as_ref()
                    .map(|f| vec![f.clone()])
                    .ok_or_eyre("no normal calibration points")
            }
            _ => {
                // Solidity could even emit the JUMP opcode within the callsite (similar to function
                // inlining). In this case, we need to split the block into multiple blocks. For
                // example, address: 0x3fc91a3afd70395cd496c647d5a6cc9d4b2b7fad,
                // block: ic[1317..1337].
                Ok(info.normal)
            }
        }
    }
}
