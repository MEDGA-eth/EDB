use std::collections::BTreeMap;

use rayon::iter::{IntoParallelRefMutIterator, ParallelIterator};

use crate::{analysis::source_map::RefinedSourceMap, RuntimeAddress};

use super::{AnalyzedCallTrace, BlockNode};

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
    ) {
        // We first project the source labels into the call trace.
        self.project_source_labels(source_map);

        // At last, we apply the lazy updates to the call trace.
        self.apply_lazy_updates();
    }

    /// This function label the first opcode of a statement or an inline assembly block with the
    /// corresponding source label.
    fn project_source_labels(&mut self, source_map: &BTreeMap<RuntimeAddress, RefinedSourceMap>) {
        // This can be done in parallel.
        self.nodes.par_iter_mut().filter(|func| !func.is_discarded()).for_each(|func| {
            let source_map = match source_map.get(&func.addr) {
                Some(source_map) => source_map,
                None => return,
            };

            func.trace.par_iter_mut().for_each(|block| {
                block.label_source(source_map);
            });
        });
    }
}

impl BlockNode {
    fn label_source(&mut self, source_map: &RefinedSourceMap) {
        // TODO
    }
}
