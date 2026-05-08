// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::{collections::BTreeMap, fmt::Display, sync::Arc};

use crate::{
    USID, UVID,
    analysis::{SourceAnalysis, StepKind, VariableRef},
    instrumentation::codegen,
};

use eyre::Result;
use semver::Version;

const LEFT_BRACKET_PRIORITY: u8 = 255; // used for the left bracket of the block
const FUNCTION_ENTRY_PRIORITY: u8 = 191; // used for the before step hook of function and modifier entry
const VISIBILITY_PRIORITY: u8 = 128; // used for the visibility of state variables and functions
const VARIABLE_UPDATE_PRIORITY: u8 = 127; // used for the variable update hook
const BEFORE_STEP_PRIORITY: u8 = 63; // used for the before step hook of statements other than function and modifier entry.
const RIGHT_BRACKET_PRIORITY: u8 = 0; // used for the right bracket of the block

/// A reference to a version.
pub type VersionRef = Arc<Version>;

/// The collections of modifications on a source file.
pub struct SourceModifications {
    source_id: u32,
    /// The modifications on the source file. The key is the location of modification in the
    /// original source code.
    modifications: BTreeMap<usize, InstrumentAction>,
}

impl SourceModifications {
    /// Creates a new source modifications.
    pub fn new(source_id: u32) -> Self {
        Self { source_id, modifications: BTreeMap::new() }
    }

    /// Adds a modification to the source modifications.
    ///
    /// # Panics
    ///
    /// Panics if the modification overlaps with the previous or next modification.
    pub fn add_modification(&mut self, modification: InstrumentAction) {
        assert_eq!(modification.source_id, self.source_id, "modification source id mismatch");

        let loc = modification.loc;
        // Check if the modification overlaps with the previous modification
        if let Some((immediate_prev_loc, _immediate_prev)) =
            self.modifications.range_mut(..loc).next_back()
        {
            assert!(
                *immediate_prev_loc <= loc,
                "modification location overlaps with previous modification"
            );
        }
        // Check if the modification overlaps with the next modification
        if let Some((immediate_next_loc, immediate_next)) =
            self.modifications.range_mut(loc..).next()
        {
            assert!(
                loc <= *immediate_next_loc,
                "modification location overlaps with next modification"
            );
            // if both of them instrument at the same location, merge them. The later coming
            // modification will be appended after the earlier one.
            if *immediate_next_loc == loc {
                immediate_next.content = if immediate_next.priority >= modification.priority {
                    InstrumentContent::Plain(format!(
                        "{} {}",
                        immediate_next.content, modification.content,
                    ))
                } else {
                    InstrumentContent::Plain(format!(
                        "{} {}",
                        modification.content, immediate_next.content,
                    ))
                };
                return;
            }
        }
        // Insert the modification
        self.modifications.insert(loc, modification);
    }

    /// Extends the modifications with the given modifications.
    pub fn extend_modifications(&mut self, modifications: Vec<InstrumentAction>) {
        for modification in modifications {
            self.add_modification(modification);
        }
    }

    /// Modifies the source code with the modifications.
    pub fn modify_source(&self, source: &str) -> String {
        let mut modified_source = source.to_string();
        // Apply the modifications in reverse order to avoid index shifting
        for (_, instrument_action) in self.modifications.iter().rev() {
            modified_source
                .insert_str(instrument_action.loc, instrument_action.content.to_string().as_str());
        }
        modified_source
    }
}

/// An action to instrument a code in the source file.
#[derive(Debug, Clone)]
pub struct InstrumentAction {
    /// The source ID of the source file to instrument
    pub source_id: u32,
    /// The location of the code to instrument. This is the offset of the code at which the
    /// instrumented code should be inserted.
    pub loc: usize,
    /// The code to instrument
    pub content: InstrumentContent,
    /// The priority of the instrument action. If two `InstrumentAction`s have the same `loc`, the
    /// one with higher priority will be applied first.
    pub priority: u8,
}

/// The content to instrument.
#[derive(Debug, Clone)]
pub enum InstrumentContent {
    /// The code to instrument. The plain code can be directly inserted into the source code as a
    /// string.
    Plain(String),
    /// View method for state variables
    ViewMethod {
        /// The state variable being accessed.
        variable: VariableRef,
    },
    /// A `before_step` hook. The debugger will pause here during step-by-step execution.
    BeforeStepHook {
        /// Compiler Version
        version: VersionRef,
        /// The USID of the step.
        usid: USID,
        /// The number of function calls made in the step.
        function_calls: usize,
    },
    /// A `variable_update` hook. The debugger will record the value of the variable when it is
    /// updated.
    VariableUpdateHook {
        /// Compiler Version
        version: VersionRef,
        /// The UVID of the variable.
        uvid: UVID,
        /// The variable that is updated.
        variable: VariableRef,
    },
}

impl Display for InstrumentContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let content = match self {
            Self::Plain(content) => content.clone(),
            Self::ViewMethod { variable } => {
                codegen::generate_view_method(variable).unwrap_or_default()
            }
            Self::BeforeStepHook { version, usid, .. } => {
                codegen::generate_step_hook(version, *usid).unwrap_or_default()
            }
            Self::VariableUpdateHook { version, uvid, variable } => {
                codegen::generate_variable_update_hook(version, *uvid, variable).unwrap_or_default()
            }
        };
        write!(f, "{content}")
    }
}

impl SourceModifications {
    /// Collects the modifications on the source code given the analysis result.
    pub fn collect_modifications(
        &mut self,
        compiler_version: VersionRef,
        source: &str,
        analysis: &SourceAnalysis,
    ) -> Result<()> {
        // Collect the modifications to generate view methods for state variables.
        self.collect_view_method_modifications(analysis);

        // Collect the modifications to patch single-statement if/for/while/try/catch/etc.
        self.collect_statement_to_block_modifications(source, analysis)?;

        // Collect the before step hook modifications for each step.
        self.collect_before_step_hook_modifications(compiler_version.clone(), source, analysis)?;

        // Collect the variable update hook modifications for each step.
        self.collect_variable_update_hook_modifications(compiler_version, source, analysis)?;

        Ok(())
    }

    /// Collects the modifications to generate view methods for state variables.
    fn collect_view_method_modifications(&mut self, analysis: &SourceAnalysis) {
        let source_id = self.source_id;
        for state_variable in &analysis.state_variables {
            let src = &state_variable.declaration().src;
            let loc = src.start.unwrap_or(0) + src.length.unwrap_or(0) + 1; // XXX (ZZ): we may need to check last char
            let instrument_action = InstrumentAction {
                source_id,
                loc,
                content: InstrumentContent::ViewMethod { variable: state_variable.clone() },
                priority: VISIBILITY_PRIORITY,
            };
            self.add_modification(instrument_action);
        }
    }

    /// Collects the modifications to convert a statement to a block. Some control flow structures,
    /// such as if/for/while/try/catch/etc., may have their body as a single statement. We need to
    /// convert them to a block.
    fn collect_statement_to_block_modifications(
        &mut self,
        _source: &str,
        analysis: &SourceAnalysis,
    ) -> Result<()> {
        let source_id = self.source_id;

        let left_bracket = |loc: usize| -> InstrumentAction {
            InstrumentAction {
                source_id,
                loc,
                content: InstrumentContent::Plain("{".to_string()),
                priority: LEFT_BRACKET_PRIORITY,
            }
        };
        let right_bracket = |loc: usize| -> InstrumentAction {
            InstrumentAction {
                source_id,
                loc,
                content: InstrumentContent::Plain("}".to_string()),
                priority: RIGHT_BRACKET_PRIORITY,
            }
        };

        for statement_body in &analysis.statement_bodies {
            let left_bracket = left_bracket(statement_body.range.start);
            let right_bracket = right_bracket(statement_body.range.end());
            let modifications = vec![left_bracket, right_bracket];
            self.extend_modifications(modifications);
        }

        Ok(())
    }

    fn collect_before_step_hook_modifications(
        &mut self,
        compiler_version: VersionRef,
        _source: &str,
        analysis: &SourceAnalysis,
    ) -> Result<()> {
        let source_id = self.source_id;
        for step in &analysis.steps {
            let usid = step.usid();
            let function_calls = step.function_calls();
            let loc = step.hook_locations().before_step;
            let priority = if matches!(step.kind(), StepKind::Entry(_)) {
                FUNCTION_ENTRY_PRIORITY
            } else {
                BEFORE_STEP_PRIORITY
            };
            let instrument_action = InstrumentAction {
                source_id,
                loc,
                content: InstrumentContent::BeforeStepHook {
                    version: compiler_version.clone(),
                    usid,
                    function_calls,
                },
                priority,
            };
            self.add_modification(instrument_action);
        }

        Ok(())
    }

    fn collect_variable_update_hook_modifications(
        &mut self,
        compiler_version: VersionRef,
        _source: &str,
        analysis: &SourceAnalysis,
    ) -> Result<()> {
        let source_id = self.source_id;
        for step in &analysis.steps {
            let updated_variables = step.updated_variables();
            let locs = step.hook_locations().after_step;
            for loc in locs {
                for updated_variable in updated_variables {
                    let uvid = updated_variable.id();
                    let instrument_action = InstrumentAction {
                        source_id,
                        loc,
                        content: InstrumentContent::VariableUpdateHook {
                            version: compiler_version.clone(),
                            uvid,
                            variable: updated_variable.clone(),
                        },
                        priority: VARIABLE_UPDATE_PRIORITY,
                    };

                    self.add_modification(instrument_action);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::analysis::{self, tests::compile_and_analyze};

    use super::*;

    #[test]
    fn test_do_while_loop_step_modification() {
        let source = r#"
        contract C {
            function a() public returns (uint256) {
                do {
                    uint x = 1;
                    return 0;
                } while (false);
            }
        }
        "#;

        let (_sources, analysis) = analysis::tests::compile_and_analyze(source);

        let mut modifications = SourceModifications::new(analysis::tests::TEST_CONTRACT_SOURCE_ID);
        let version = Arc::new(Version::parse("0.8.0").unwrap());
        modifications.collect_before_step_hook_modifications(version, source, &analysis).unwrap();
        assert_eq!(modifications.modifications.len(), 4);
        let modified_source = modifications.modify_source(source);

        // The modified source should be able to be compiled and analyzed.
        let (_sources, _analysis2) = analysis::tests::compile_and_analyze(&modified_source);
    }

    #[test]
    fn test_collect_function_entry_step_hook_modifications() {
        let source = r#"
        abstract contract C {
            function v() public virtual returns (uint256);

            function a() public returns (uint256) {
                uint x = 1;
            }
        }
        "#;

        let (_sources, analysis) = analysis::tests::compile_and_analyze(source);

        let mut modifications = SourceModifications::new(analysis::tests::TEST_CONTRACT_SOURCE_ID);
        let version = Arc::new(Version::parse("0.8.0").unwrap());
        modifications.collect_before_step_hook_modifications(version, source, &analysis).unwrap();
        // assert_eq!(modifications.modifications.len(), 1);
        let modified_source = modifications.modify_source(source);

        // The modified source should be able to be compiled and analyzed.
        let (_sources, _analysis2) = analysis::tests::compile_and_analyze(&modified_source);
    }

    #[test]
    fn test_collect_before_step_hook_modifications() {
        let source = r#"
        abstract contract C {
            function a() public returns (uint256) {
                if (false) {return 0;}
                else    {return 1;}
                for (uint256 i = 0; i < 10; i++) {
                    return 0;
                }
                while (true) {
                    return 0;
                }
                do {
                    return 0;
                } while (false);
                try this.a() {
                    return 0;
                }
                catch {}
                return 0;
            }
        }
        "#;

        let (_sources, analysis) = analysis::tests::compile_and_analyze(source);

        let mut modifications = SourceModifications::new(analysis::tests::TEST_CONTRACT_SOURCE_ID);
        let version = Arc::new(Version::parse("0.8.0").unwrap());
        modifications.collect_before_step_hook_modifications(version, source, &analysis).unwrap();
        assert_eq!(modifications.modifications.len(), 12);
        let modified_source = modifications.modify_source(source);

        // The modified source should be able to be compiled and analyzed.
        let (_sources, _analysis2) = analysis::tests::compile_and_analyze(&modified_source);
    }

    #[test]
    fn test_variable_update_hook_modification_for_for_loop() {
        let source = r#"
        contract C {
            function a() public returns (uint256) {
                for (uint i = 0; i < 10; i++) {
                    return i;
                }
            }
        }
        "#;
        let (_sources, analysis) = compile_and_analyze(source);

        let mut modifications = SourceModifications::new(analysis::tests::TEST_CONTRACT_SOURCE_ID);
        modifications
            .collect_variable_update_hook_modifications(
                Arc::new(Version::parse("0.8.0").unwrap()),
                source,
                &analysis,
            )
            .unwrap();
        assert_eq!(modifications.modifications.len(), 1);
        let modified_source = modifications.modify_source(source);

        // The modified source should be able to be compiled and analyzed.
        let (_sources, _analysis2) = analysis::tests::compile_and_analyze(&modified_source);
    }
}
