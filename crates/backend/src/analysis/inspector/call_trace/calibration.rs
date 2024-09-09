use std::collections::{BTreeMap, BTreeSet, HashMap};

use eyre::{bail, ensure, OptionExt, Result};

use crate::{
    analysis::source_map::{
        debug_unit::DebugUnit, integrity::IntergrityLevel, source_label::SourceLabel,
        RefinedSourceMap,
    },
    RuntimeAddress,
};

use super::{AnalyzedCallTrace, BlockNode, CalibrationPoint, FuncNode};

#[cfg(feature = "paralize_analysis")]
use rayon::prelude::*;

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
        #[cfg(feature = "paralize_analysis")]
        let iter = self.nodes.par_iter_mut();
        #[cfg(not(feature = "paralize_analysis"))]
        let iter = self.nodes.iter_mut();

        iter.filter(|func| !func.is_discarded()).try_for_each(|func| {
            let source_map = match source_map.get(&func.addr) {
                Some(source_map) => source_map,
                None => return Ok(()),
            };

            if source_map.intergrity_level == IntergrityLevel::Corrupted {
                // We do not need to calibrate the function if the source map is corrupted.
                return Ok(());
            }

            #[cfg(feature = "paralize_analysis")]
            let iter = func.trace.par_iter_mut();
            #[cfg(not(feature = "paralize_analysis"))]
            let mut iter = func.trace.iter_mut();

            iter.try_for_each(|block| block.label_source(source_map))?;

            func.construct_source_call_trace(source_map)?;
            func.reorganize_blocks()
        })
    }
}

impl FuncNode {
    fn construct_source_call_trace(&mut self, source_map: &RefinedSourceMap) -> Result<()> {
        let func_sig = |name: &str, n: usize| format!("{}::{}", name, n);

        // Step 0. Collect all labels to avoid duplicated functions.
        let mut labels = BTreeSet::new();
        for block in &self.trace {
            for ic in block.start_ic..block.start_ic + block.inst_n {
                if let Some(label) = source_map.labels.get(ic) {
                    // Source map may not cover all instructions. For example, the constructor of
                    // 0x1f98431c8ad98523631ae4a59f267346ea31f984.
                    labels.insert(label);
                };
            }
        }

        // Step 1. Collect related functions as nodes.
        let mut func_nodes: HashMap<String, DebugUnit> = HashMap::new();
        for label in &labels {
            // We need first collect all covere functions.
            let Some(func) = label.function_tag() else {
                continue;
            };

            let meta = func.get_function_meta().expect("this has to be a function unit");
            if meta.is_modifier {
                continue;
            }

            // TODO (ZZ): This is still an estimated signature. We need to refine it in the future
            // (e.g., considering the parameter types and the contract).
            let sig = func_sig(&meta.name, meta.parameters.parameters.len());
            debug!(addr=?self.addr, sig=?sig, "source-level function found: {self}");

            if let Some(old_func) = func_nodes.get(&sig) {
                if old_func != func {
                    // At this point, it means that we found two functions with the same name
                    // within *a single contract*. Note that, at this point, external functions
                    // (in other contracts) are not included, since we are analyzing a single
                    // function node. The only valid case is that one of
                    // the functions is a virtual function and could be overridden by another.
                    let old_meta =
                        old_func.get_function_meta().expect("this has to be a function unit");
                    let new_meta =
                        func.get_function_meta().expect("this has to be a function unit");
                    match (old_meta.is_virtual, new_meta.is_virtual) {
                        (false, false) => {
                            bail!("two different functions with the same name: {self} {sig} {old_meta}")
                        }
                        (true, true) => {
                            bail!("two different virtual functions with the same name: {self} {sig} {old_meta}")
                        }
                        (false, true) => {
                            // We do not need to inject the virtual function here.
                            continue;
                        }
                        (true, false) => {
                            // We need to replace the old function with the new one.
                            // This will be done in the next step.
                        }
                    }
                }
            }

            func_nodes.insert(sig, func.clone());
        }
        debug!(addr=?self.addr, func_n=func_nodes.len(), funcs=?func_nodes.keys(), "source-level functions found: {self}");

        // Step 2. Collect callsites as edges.
        let mut call_edges: Vec<(String, String)> = Vec::new();
        for label in &labels {
            let Some(stmt) = label.statement_tag() else {
                continue;
            };

            let func = source_map
                .debug_units
                .function(stmt)
                .ok_or_eyre("statement has to be in a function")?;
            debug_assert!(matches!(func, DebugUnit::Function(_, _)));

            let caller_meta = func.get_function_meta().expect("this has to be a function unit");
            let caller_sig = func_sig(&caller_meta.name, caller_meta.parameters.parameters.len());
            debug!(addr=?self.addr, caller_sig=?caller_sig, "source-level caller found: {self}");

            let statement_meta =
                stmt.get_statement_meta().expect("this has to be a statement unit");
            for callee_meta in &statement_meta.inner_func_call {
                if callee_meta.is_constructor {
                    // External message call (e.g., constructor) will not be included.
                    continue;
                }

                let callee_sig = func_sig(&callee_meta.name, callee_meta.arg_n);
                debug!(addr=?self.addr, caller_sig=?caller_sig, callee_sig=?callee_sig, "source-level callsite found: {self}");
                if func_nodes.contains_key(&callee_sig) {
                    call_edges.push((caller_sig.clone(), callee_sig));
                }
            }
        }
        debug!(addr=?self.addr, edge_n=call_edges.len(), "source-level callsites found: {self}");

        Ok(())
    }

    fn reorganize_blocks(&mut self) -> Result<()> {
        // // We first merge the blocks that are fused together during compilation.
        // let first_block = self.trace.first().ok_or_eyre("empty trace")?;
        // if first_block.calib_func.is_none() && first_block.contained_funcs.len() > 0 {
        //     bail!("no calibration function found for the first block: {} {}", first_block.addr,
        // first_block); }

        Ok(())
    }
}

impl BlockNode {
    fn label_source(&mut self, source_map: &RefinedSourceMap) -> Result<()> {
        // We first label the first opcode of a statement or an inline assembly block.
        let mut cur_label = None;
        trace!(addr=?self.addr, block=format!("{self}"), "calibrating");
        for ic in self.start_ic..self.start_ic + self.inst_n {
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
            pub modifiers: Vec<DebugUnit>,
        }

        let info = funcs.into_iter().fold(CalculationFunctionInfo::default(), |mut info, func| {
            let meta = func.get_function_meta().expect("this has to be a function unit");
            if meta.is_modifier {
                info.modifiers.push(func.clone());
            } else {
                info.normal.push(func.clone());
            }
            info
        });

        self.contained_modifiers = info.modifiers;
        trace!(addr=?self.addr, block=format!("{self}"), modifier_n=self.contained_modifiers.len(), "calibration points");

        self.contained_funcs = info.normal;
        trace!(addr=?self.addr, block=format!("{self}"), func_n=self.contained_funcs.len(), "calibration points");

        Ok(())

        /*
         * The following code may be useless.
         */
        // if self.contained_funcs.len() == 0 {
        //     // If there is no normal function, we cannot calibrate the block for now. We will
        //     // handle this case in the next step.
        //     trace!(addr=?self.addr, block=format!("{self}"), "no normal calibration points");
        //     self.calib_func = None;
        //     return Ok(());
        // } else if self.contained_funcs.len() == 1 {
        //     // If there is only one normal function, we can calibrate the block directly.
        //     self.calib_func = self.contained_funcs.pop();
        //     return Ok(());
        // }

        // // This is a complex case, indicating there are function inlining or other optimizations
        // // happened. For example, address: 0x3fc91a3afd70395cd496c647d5a6cc9d4b2b7fad,
        // // block: ic[1317..1337].
        // //
        // // We first collect the names of all the functions that have been called in this block.
        // let callee_names = self
        //     .calib
        //     .values()
        //     .filter_map(|p| {
        //         p.as_singleton().and_then(|l| match l {
        //             SourceLabel::PrimitiveStmt { stmt: DebugUnit::Primitive(_, meta), .. } => {
        //                 Some(meta)
        //             }
        //             _ => None,
        //         })
        //     })
        //     .map(|meta| {
        //         meta.inner_func_call.iter().filter_map(|c| {
        //             if c.is_constructor {
        //                 // External message call (e.g., constructor) connot be inlined.
        //                 None
        //             } else {
        //                 Some(c.name.as_str())
        //             }
        //         })
        //     })
        //     .flatten()
        //     .collect::<BTreeSet<_>>();

        // let mut non_callee_funcs = self
        //     .contained_funcs
        //     .iter()
        //     .filter(|f| {
        //         !callee_names.contains(f.get_name().expect("this has to be a function unit"))
        //     })
        //     .collect::<Vec<_>>();

        // if non_callee_funcs.len() == 1 {
        //     // If there is only one function that is not called in this block, we can calibrate
        // the     // block directly.
        //     self.calib_func = non_callee_funcs.pop().cloned();
        //     return Ok(());
        // }

        // // We do know how to calibrate the block in this case. We will handle this case in the
        // next // step.
        // warn!(addr=?self.addr, block=format!("{self}"), callee_names=?callee_names, "multiple
        // functions are called in this block"); self.calib_func = None;

        // Ok(())
    }
}
