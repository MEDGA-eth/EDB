use std::collections::BTreeMap;

use eyre::Result;

use crate::{
    analysis::source_map::{integrity::IntergrityLevel, RefinedSourceMap},
    RuntimeAddress,
};

use super::AnalyzedCallTrace;

#[cfg(feature = "paralize_analysis")]
use rayon::prelude::*;

// fn get_associated_function<'a>(
//     source_map: &'a RefinedSourceMap,
//     label: &'a SourceLabel,
// ) -> Option<(&'a DebugUnit, &'a FunctionMeta)> {
//     let func = label
//         .statement_tag()
//         .and_then(|stmt| source_map.debug_units.function(stmt))
//         .or(label.function_tag())?;
//     let meta = func.get_function_meta()?;
//     if meta.is_modifier {
//         // We do not take modifiers into account.
//         None
//     } else {
//         Some((func, meta))
//     }
// }

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

            // #[cfg(feature = "paralize_analysis")]
            // let iter = func.trace.par_iter_mut();
            // #[cfg(not(feature = "paralize_analysis"))]
            // let mut iter = func.trace.iter_mut();

            // iter.try_for_each(|block| block.label_source(source_map))?;
            // func.construct_source_call_trace(source_map)?;

            Ok(())
        })
    }
}

// impl FuncNode {
//     // XXX (ZZ): since it is very hard to get type information from the AST, we make a strong
//     // assumption when constructing the source-level call trace. Specifically, we assume that,
//     // within a message call (i.e., do not take any inter-contract call into account), the only
//     // valid case of having two functions with the same name is that one directly calls the other
//     // (e.g., `super._beforeTransferFrom`). This is a very strong assumption (and seems to be
// held     // in most cases), but it is the best we can do for now.
//     fn construct_source_call_trace(&mut self, source_map: &RefinedSourceMap) -> Result<()> {
//         let (nodes, edges) = self.collect_source_nodes_and_edges(source_map)?;
//
//         Ok(())
//     }
//
//     fn collect_source_nodes_and_edges<'a>(
//         &mut self,
//         source_map: &'a RefinedSourceMap,
//     ) -> Result<(BTreeSet<&'a DebugUnit>, BTreeSet<(&'a DebugUnit, &'a DebugUnit)>)> {
//         let func_sig = |name: &str, n: usize| format!("{}::{}", name, n);
//
//         // Step 0. Collect all labels to avoid duplicated functions.
//         let mut labels = BTreeSet::new();
//         for block in &self.trace {
//             for ic in block.start_ic..block.start_ic + block.inst_n {
//                 if let Some(label) = source_map.labels.get(ic) {
//                     // Source map may not cover all instructions. For example, the constructor of
//                     // 0x1f98431c8ad98523631ae4a59f267346ea31f984.
//                     labels.insert(label);
//                 };
//             }
//         }
//
//         // Step 1. Collect all intra-contract functions, and assign them with a default function
//         // signature.
//         let mut func_nodes = labels
//             .iter()
//             .filter_map(|label| get_associated_function(source_map, label))
//             .map(|(func, meta)| {
//                 let sig = func_sig(&meta.name, meta.parameters.parameters.len());
//                 (func, sig)
//             })
//             .collect::<BTreeMap<_, _>>();
//         trace!(addr=?self.addr, func_n=func_nodes.len(), funcs=?func_nodes.keys().map(|f|
// format!("{}", *f)).collect::<Vec<_>>(), "source-level functions found: {self}");
//
//         // Step 2. Fix the function signature conflicts.
//         for label in &labels {
//             let Some(stmt) = label.statement_tag() else {
//                 continue;
//             };
//
//             let Some((func, caller_meta)) = get_associated_function(source_map, label) else {
//                 continue
//             };
//             debug_assert!(matches!(func, DebugUnit::Function(_, _)));
//             debug_assert!(func_nodes.contains_key(&func));
//
//             // Get the caller function signature.
//             let caller_sig = func_sig(&caller_meta.name,
// caller_meta.parameters.parameters.len());
//
//             // Check whether there is a function signature conflict.
//             let statement_meta =
//                 stmt.get_statement_meta().expect("this has to be a statement unit");
//             if statement_meta
//                 .inner_func_call
//                 .iter()
//                 .any(|meta| !meta.is_constructor && func_sig(&meta.name, meta.arg_n) ==
// caller_sig)             {
//                 // We will always enforce the update without any check.
//                 let caller_sig = format!("{}::caller", caller_sig);
//                 func_nodes.insert(func, caller_sig);
//             }
//         }
//         // Check that every function has a unique signature.
//         trace!(addr=?self.addr, func_n=func_nodes.len(),
// funcs=?func_nodes.values().collect::<Vec<_>>(), "source-level functions found: {self}");
//         let reverse_func_nodes = func_nodes.iter().map(|(f, s)| (s, f)).collect::<HashMap<_,
// _>>();         ensure!(reverse_func_nodes.len() == func_nodes.len(), "function signature
// conflict");
//
//         // Step 3. Collect functions and callsites as nodes and edges, respectively. We will also
//         // try to handle function signature conflicts.
//         let mut call_edges = BTreeSet::new();
//         for label in &labels {
//             let Some(stmt) = label.statement_tag() else {
//                 continue;
//             };
//
//             let Some((func, _)) = get_associated_function(source_map, label) else { continue };
//             debug_assert!(matches!(func, DebugUnit::Function(_, _)));
//             debug_assert!(func_nodes.contains_key(func));
//
//             // Check whether there is a function signature conflict.
//             let caller_sig = func_nodes.get(func).expect("this has to be valid");
//             let statement_meta =
//                 stmt.get_statement_meta().expect("this has to be a statement unit");
//
//             // Collect edges
//             for callee_meta in &statement_meta.inner_func_call {
//                 if callee_meta.is_constructor {
//                     // External message call (e.g., constructor) will not be included.
//                     continue;
//                 }
//
//                 let callee_sig = func_sig(&callee_meta.name, callee_meta.arg_n);
//                 trace!(addr=?self.addr, caller_sig=caller_sig, callee_sig=callee_sig,
// "source-level callsite");                 let callee_func = if
// caller_sig.starts_with(&callee_sig) {                     // This is the conflict case.
//                     trace!("function signature conflict");
//                     reverse_func_nodes.get(&callee_sig)
//                 } else {
//                     // XXX (ZZ): here we assume that only the *caller* (in confliction) can be
//                     // called by other functions. We hence first try with `::caler` postfix.
//                     trace!("normal case");
//                     reverse_func_nodes
//                         .get(&format!("{}::caller", callee_sig))
//                         .and(reverse_func_nodes.get(&callee_sig))
//                 };
//                 let Some(callee_func) = callee_func else {
//                     // It is an inter-contract call.
//                     trace!("callee function not found");
//                     continue;
//                 };
//
//                 call_edges.insert((func, **callee_func));
//             }
//         }
//         trace!(addr=?self.addr, func_n=func_nodes.len(), edge_n=call_edges.len(), "source-level
// callsites found: {self}");
//
//         Ok((func_nodes.into_keys().collect(), call_edges))
//     }
// }
//
// impl BlockNode {
//     fn label_source(&mut self, source_map: &RefinedSourceMap) -> Result<()> {
//         // We first label the first opcode of a statement or an inline assembly block.
//         let mut cur_label = None;
//         trace!(addr=?self.addr, block=format!("{self}"), "calibrating");
//         for ic in self.start_ic..self.start_ic + self.inst_n {
//             let Some(c_label) = source_map.labels.get(ic) else {
//                 // Source map may not cover all instructions. For example, the constructor of
//                 // 0x1f98431c8ad98523631ae4a59f267346ea31f984.
//                 break;
//             };
//             match cur_label {
//                 Some(label) if label != c_label => {
//                     if c_label.is_source() {
//                         self.calib.insert(ic, CalibrationPoint::Singleton(c_label.clone()));
//                         cur_label = Some(c_label);
//                     }
//                 }
//                 None if c_label.is_source() => {
//                     self.calib.insert(ic, CalibrationPoint::Singleton(c_label.clone()));
//                     cur_label = Some(c_label);
//                 }
//                 _ => {}
//             }
//         }
//
//         // We then try to find the function of this block.
//         let funcs: BTreeSet<_> = self
//             .calib
//             .values()
//             .filter_map(|p| p.as_singleton().and_then(|l| l.function()))
//             .collect();
//
//         // An internal struct to store the information of a calculation function.
//         #[derive(Default)]
//         struct CalculationFunctionInfo {
//             pub normal: Vec<DebugUnit>,
//             pub modifiers: Vec<DebugUnit>,
//         }
//
//         let info = funcs.into_iter().fold(CalculationFunctionInfo::default(), |mut info, func| {
//             let meta = func.get_function_meta().expect("this has to be a function unit");
//             if meta.is_modifier {
//                 info.modifiers.push(func.clone());
//             } else {
//                 info.normal.push(func.clone());
//             }
//             info
//         });
//
//         self.contained_modifiers = info.modifiers;
//         trace!(addr=?self.addr, block=format!("{self}"),
// modifier_n=self.contained_modifiers.len(), "calibration points");
//
//         self.contained_funcs = info.normal;
//         trace!(addr=?self.addr, block=format!("{self}"), func_n=self.contained_funcs.len(),
// "calibration points");
//
//         Ok(())
//
//         /*
//          * The following code may be useless.
//          */
//         // if self.contained_funcs.len() == 0 {
//         //     // If there is no normal function, we cannot calibrate the block for now. We will
//         //     // handle this case in the next step.
//         //     trace!(addr=?self.addr, block=format!("{self}"), "no normal calibration points");
//         //     self.calib_func = None;
//         //     return Ok(());
//         // } else if self.contained_funcs.len() == 1 {
//         //     // If there is only one normal function, we can calibrate the block directly.
//         //     self.calib_func = self.contained_funcs.pop();
//         //     return Ok(());
//         // }
//
//         // // This is a complex case, indicating there are function inlining or other
// optimizations         // // happened. For example, address:
// 0x3fc91a3afd70395cd496c647d5a6cc9d4b2b7fad,         // // block: ic[1317..1337].
//         // //
//         // // We first collect the names of all the functions that have been called in this
// block.         // let callee_names = self
//         //     .calib
//         //     .values()
//         //     .filter_map(|p| {
//         //         p.as_singleton().and_then(|l| match l {
//         //             SourceLabel::PrimitiveStmt { stmt: DebugUnit::Primitive(_, meta), .. } =>
// {         //                 Some(meta)
//         //             }
//         //             _ => None,
//         //         })
//         //     })
//         //     .map(|meta| {
//         //         meta.inner_func_call.iter().filter_map(|c| {
//         //             if c.is_constructor {
//         //                 // External message call (e.g., constructor) connot be inlined.
//         //                 None
//         //             } else {
//         //                 Some(c.name.as_str())
//         //             }
//         //         })
//         //     })
//         //     .flatten()
//         //     .collect::<BTreeSet<_>>();
//
//         // let mut non_callee_funcs = self
//         //     .contained_funcs
//         //     .iter()
//         //     .filter(|f| {
//         //         !callee_names.contains(f.get_name().expect("this has to be a function unit"))
//         //     })
//         //     .collect::<Vec<_>>();
//
//         // if non_callee_funcs.len() == 1 {
//         //     // If there is only one function that is not called in this block, we can
// calibrate         // the     // block directly.
//         //     self.calib_func = non_callee_funcs.pop().cloned();
//         //     return Ok(());
//         // }
//
//         // // We do know how to calibrate the block in this case. We will handle this case in the
//         // next // step.
//         // warn!(addr=?self.addr, block=format!("{self}"), callee_names=?callee_names, "multiple
//         // functions are called in this block"); self.calib_func = None;
//
//         // Ok(())
//     }
// }
