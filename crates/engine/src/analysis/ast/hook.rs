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

use serde::{Deserialize, Serialize};

/// Locations of different types of hooks in a [`crate::analysis::Step`]. The locations are the
/// source string index in the same source file as the corresponding step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepHookLocations {
    /// The location to instrument hooks before the step
    pub before_step: usize,
    /// The locations to instrument hooks after the step. The hooks will be instrumented after all
    /// the locations in this vector.
    pub after_step: Vec<usize>,
}
