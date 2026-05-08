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

use foundry_compilers::artifacts::ast::SourceLocation;
use serde::{Deserialize, Serialize};

/// Represents a range of source code in a source file.
///
/// The intension of this struct is to replace the `SourceLocation` in the `foundry_compilers`
/// crate, which provides more consistent source location information:
/// - The start, length, and file always exists, instead of being `Option`.
/// - The semicolon of the statement is included in the range.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SourceRange {
    /// The file index of the range.
    pub file: u32,
    /// The start index of the range.
    pub start: usize,
    /// The length of the range.
    pub length: usize,
}

impl From<SourceLocation> for SourceRange {
    fn from(location: SourceLocation) -> Self {
        match (location.index, location.start, location.length) {
            (Some(file), Some(start), Some(length)) => Self { file: file as u32, start, length },
            _ => Self::non_existent(),
        }
    }
}

impl From<SourceRange> for SourceLocation {
    fn from(range: SourceRange) -> Self {
        Self {
            index: Some(range.file as usize),
            start: Some(range.start),
            length: Some(range.length),
        }
    }
}

impl SourceRange {
    /// Creates a new source range.
    pub fn new(file: u32, start: usize, length: usize) -> Self {
        Self { file, start, length }
    }

    /// Expands the source range to the next semicolon in the same source file. The semicolon will
    /// be included in the range. If there is no semicolon, the source range is not expanded.
    pub fn expand_to_next_semicolon(mut self, source: &str) -> Self {
        let start = self.start;
        let end = self.next_loc();

        // Check if we already have a semicolon at the end
        if end > 0 && source.as_bytes().get(end - 1) == Some(&b';') {
            // Already have a semicolon, no need to expand
            return self;
        }

        let substr = &source[end..];
        if let Some(semicolon) = substr.find(";").map(|i| i + end) {
            self.length = semicolon - start + 1;
        }
        self
    }

    /// Creates a new source range that is non-existent.
    pub fn non_existent() -> Self {
        Self { file: u32::MAX, start: 0, length: 0 }
    }

    /// Checks if the source range is existent.
    pub fn is_existent(&self) -> bool {
        self.file != u32::MAX
    }

    /// Creates a new source range that is the next location after this one.
    pub fn next_loc(&self) -> usize {
        self.start + self.length
    }

    /// Alias for [`Self::next_loc`].
    pub fn end(&self) -> usize {
        self.next_loc()
    }

    /// Slices the source code from the start of the source range to the next location.
    pub fn slice_source<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.next_loc()]
    }

    /// Merges two source ranges into a single range that spans both.
    ///
    /// The merged range will start at the minimum start position and end at
    /// the maximum end position of the two ranges.
    ///
    /// # Panics
    ///
    /// - If the two source ranges are not in the same file, it will panic.
    /// - If the two source ranges are non-adjacent (have a gap between them), it will panic.
    ///
    /// # Returns
    ///
    /// - If both source ranges are non-existent, it will return a non-existent source range.
    /// - If one of the source ranges is non-existent, it will return the other source range.
    /// - Otherwise, it will return a new source range that spans both ranges.
    pub fn merge(self, other: Self) -> Self {
        assert_eq!(self.file, other.file, "The two source ranges must be in the same file");
        if !self.is_existent() {
            return other;
        }
        if !other.is_existent() {
            return self;
        }

        let (first, second) = if self.start <= other.start { (self, other) } else { (other, self) };

        // Check if ranges are adjacent or overlapping
        assert!(
            first.next_loc() >= second.start,
            "The two source ranges are non-adjacent: first ends at {}, second starts at {}",
            first.next_loc(),
            second.start
        );

        let start = first.start;
        let end = first.next_loc().max(second.next_loc());
        let length = end - start;

        Self { file: self.file, start, length }
    }
}
