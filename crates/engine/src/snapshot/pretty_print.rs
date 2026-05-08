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

//! Pretty printing and statistics for unified snapshot collections.
//!
//! This module provides comprehensive display and analysis capabilities for the unified
//! snapshot collections. It offers detailed statistics, formatted output, and visual
//! summaries to help developers understand snapshot distribution and debugging coverage.
//!
//! # Core Features
//!
//! ## Statistics Generation
//! - **Snapshot Distribution**: Breakdown of hook vs opcode snapshots
//! - **Frame Coverage**: Analysis of execution frame coverage
//! - **Type Analysis**: Detailed categorization of snapshot types
//!
//! ## Visual Display
//! - **Summary Output**: Comprehensive formatted summary with colors and icons
//! - **Frame-by-Frame Details**: Organized display by execution frame
//! - **Coverage Visualization**: Clear indication of debugging coverage
//! - **Statistics Dashboard**: Overview of snapshot collection metrics
//!
//! # Usage
//!
//! The pretty printing functionality is designed for:
//! - **Development Analysis**: Understanding snapshot collection effectiveness
//! - **Debug Coverage Assessment**: Identifying gaps in debugging instrumentation
//! - **Performance Analysis**: Analyzing snapshot overhead and distribution
//! - **User-Friendly Display**: Providing clear, formatted output for developers

use std::collections::HashMap;

use edb_common::types::ExecutionFrameId;
use revm::{Database, DatabaseCommit, DatabaseRef, database::CacheDB};
use tracing::error;

use crate::{Snapshot, SnapshotDetail, Snapshots};

/// Comprehensive statistics about snapshot distribution and coverage.
///
/// This structure provides detailed metrics about how snapshots are distributed
/// across execution frames, enabling analysis of debugging coverage and effectiveness.
#[derive(Debug, Clone)]
pub struct SnapshotStats {
    /// Total number of snapshots
    pub total_snapshots: usize,
    /// Number of hook-based snapshots
    pub hook_snapshots: usize,
    /// Number of opcode-level snapshots
    pub opcode_snapshots: usize,
    /// Total number of unique execution frames
    pub total_frames: usize,
    /// Number of frames that have hook snapshots
    pub frames_with_hooks: usize,
    /// Number of frames that have opcode snapshots
    pub frames_with_opcodes: usize,
}

/// Pretty printing and statistics implementation for unified snapshot collections.
impl<DB> Snapshots<DB>
where
    DB: Database + DatabaseCommit + DatabaseRef + Clone,
    <CacheDB<DB> as Database>::Error: Clone,
    <DB as Database>::Error: Clone,
{
    /// Generate comprehensive statistics about snapshot distribution.
    ///
    /// This method analyzes the snapshot collection to provide detailed metrics
    /// about the distribution of hook vs opcode snapshots and frame coverage.
    pub fn get_snapshot_stats(&self) -> SnapshotStats {
        let mut hook_count = 0;
        let mut opcode_count = 0;
        let mut frames_with_hooks = std::collections::HashSet::new();
        let mut frames_with_opcodes = std::collections::HashSet::new();

        for (frame_id, snapshot) in &self.inner {
            match snapshot.detail() {
                SnapshotDetail::Hook(_) => {
                    hook_count += 1;
                    frames_with_hooks.insert(*frame_id);
                }
                SnapshotDetail::Opcode(_) => {
                    opcode_count += 1;
                    frames_with_opcodes.insert(*frame_id);
                }
            }
        }

        SnapshotStats {
            total_snapshots: self.inner.len(),
            hook_snapshots: hook_count,
            opcode_snapshots: opcode_count,
            total_frames: self.frame_count(),
            frames_with_hooks: frames_with_hooks.len(),
            frames_with_opcodes: frames_with_opcodes.len(),
        }
    }

    /// Print comprehensive visual summary of all snapshots with frame aggregation.
    ///
    /// This method provides a beautifully formatted, integrated view of both hook and
    /// opcode snapshots, organized by execution frame for easier debugging analysis.
    /// The output includes statistics, frame details, and a legend for easy interpretation.
    pub fn print_summary(&self) {
        println!(
            "\n\x1b[36m╔══════════════════════════════════════════════════════════════════╗\x1b[0m"
        );
        println!(
            "\x1b[36m║                    UNIFIED SNAPSHOTS SUMMARY                     ║\x1b[0m"
        );
        println!(
            "\x1b[36m╚══════════════════════════════════════════════════════════════════╝\x1b[0m\n"
        );

        // Get comprehensive statistics
        let stats = self.get_snapshot_stats();

        // Overall statistics section
        println!("\x1b[33m📊 Overall Statistics:\x1b[0m");
        println!("  Total snapshots: \x1b[32m{}\x1b[0m", stats.total_snapshots);
        println!("  Total frames: \x1b[32m{}\x1b[0m", stats.total_frames);
        println!(
            "  └─ Hook snapshots: \x1b[32m{}\x1b[0m ({:.1}%)",
            stats.hook_snapshots,
            if stats.total_snapshots > 0 {
                stats.hook_snapshots as f64 / stats.total_snapshots as f64 * 100.0
            } else {
                0.0
            }
        );
        println!(
            "  └─ Opcode snapshots: \x1b[32m{}\x1b[0m ({:.1}%)",
            stats.opcode_snapshots,
            if stats.total_snapshots > 0 {
                stats.opcode_snapshots as f64 / stats.total_snapshots as f64 * 100.0
            } else {
                0.0
            }
        );

        println!("\n\x1b[33m🎯 Frame Coverage:\x1b[0m");
        println!(
            "  Frames with hooks: \x1b[32m{}\x1b[0m ({:.1}%)",
            stats.frames_with_hooks,
            if stats.total_frames > 0 {
                stats.frames_with_hooks as f64 / stats.total_frames as f64 * 100.0
            } else {
                0.0
            }
        );
        println!(
            "  Frames with opcodes: \x1b[32m{}\x1b[0m ({:.1}%)",
            stats.frames_with_opcodes,
            if stats.total_frames > 0 {
                stats.frames_with_opcodes as f64 / stats.total_frames as f64 * 100.0
            } else {
                0.0
            }
        );

        if self.is_empty() {
            println!("\n\x1b[90m  No snapshots were recorded.\x1b[0m");
            return;
        }

        println!("\n\x1b[33m📋 Frame Details:\x1b[0m");
        println!(
            "\x1b[90m─────────────────────────────────────────────────────────────────\x1b[0m"
        );

        // Group snapshots by frame ID while preserving order
        let mut frame_groups: HashMap<ExecutionFrameId, Vec<&Snapshot<DB>>> = HashMap::new();
        let mut frame_order = Vec::new();

        for (frame_id, snapshot) in &self.inner {
            if !frame_groups.contains_key(frame_id) {
                frame_order.push(*frame_id);
            }
            frame_groups.entry(*frame_id).or_default().push(snapshot);
        }

        // Print frame-by-frame details
        for (display_idx, frame_id) in frame_order.iter().enumerate() {
            let snapshots = frame_groups.get(frame_id).unwrap();

            self.print_frame_summary(display_idx, *frame_id, snapshots);
        }

        println!(
            "\n\x1b[90m─────────────────────────────────────────────────────────────────\x1b[0m"
        );

        // Print legend
        println!("\n\x1b[33m📖 Legend:\x1b[0m");
        println!("  \x1b[92m🎯 Hook\x1b[0m    - Strategic instrumentation breakpoint");
        println!("  \x1b[94m⚙️ Opcode\x1b[0m  - Fine-grained instruction-level snapshot");
    }

    /// Print detailed information for a single execution frame.
    ///
    /// This method displays comprehensive information about all snapshots within
    /// a specific execution frame, including type analysis and address information.
    fn print_frame_summary(
        &self,
        display_idx: usize,
        frame_id: ExecutionFrameId,
        snapshots: &[&Snapshot<DB>],
    ) {
        let hook_count = snapshots.iter().filter(|s| s.is_hook()).count();
        let opcode_count = snapshots.iter().filter(|s| s.is_opcode()).count();
        let total_count = snapshots.len();

        // Determine frame type and color
        let (frame_type, color, icon) = if hook_count > 0 && opcode_count > 0 {
            error!("Frame {} has both hook and opcode snapshots, which is unexpected.", frame_id);
            ("Mixed", "\x1b[96m", "📍")
        } else if hook_count > 0 {
            ("Hook", "\x1b[92m", "🎯")
        } else {
            ("Opcode", "\x1b[94m", "⚙️")
        };

        println!(
            "\n  {}[{:3}] {} Frame {}\x1b[0m (trace.{}, re-entry {})",
            color,
            display_idx,
            icon,
            frame_id,
            frame_id.trace_entry_id(),
            frame_id.re_entry_count()
        );

        println!(
            "       └─ Type: \x1b[33m{frame_type}\x1b[0m | Snapshots: \x1b[32m{total_count}\x1b[0m"
        );

        if hook_count > 0 && opcode_count > 0 {
            println!("          ├─ Hook snapshots: \x1b[32m{hook_count}\x1b[0m");
            println!("          └─ Opcode snapshots: \x1b[32m{opcode_count}\x1b[0m");
        } else if hook_count > 0 {
            // Show hook details
            self.print_hook_details(snapshots, "          ");
        } else {
            // Show opcode summary
            self.print_opcode_summary(snapshots, "          ");
        }

        // Show address information
        let addresses: std::collections::HashSet<_> =
            snapshots.iter().map(|s| s.bytecode_address()).collect();
        if addresses.len() == 1 {
            println!("          └─ Address: \x1b[36m{:?}\x1b[0m", addresses.iter().next().unwrap());
        } else if !addresses.is_empty() {
            println!("          └─ Addresses: \x1b[36m{} unique\x1b[0m", addresses.len());
        }
    }

    /// Print detailed information for hook snapshots within a frame.
    ///
    /// This method displays USID information and other hook-specific details
    /// for all hook snapshots in the given frame.
    fn print_hook_details(&self, snapshots: &[&Snapshot<DB>], indent: &str) {
        let hook_snapshots: Vec<_> = snapshots
            .iter()
            .filter_map(|s| {
                if let SnapshotDetail::Hook(ref hook) = s.detail { Some(hook) } else { None }
            })
            .collect();

        if hook_snapshots.is_empty() {
            return;
        }

        let usids: Vec<_> = hook_snapshots.iter().map(|h| h.usid).collect();

        // Show USIDs with smart formatting (similar to hook_snapshot_inspector)
        if usids.len() == 1 {
            println!("{}└─ USID: \x1b[36m{}\x1b[0m", indent, usids[0]);
        } else if usids.len() <= 10 {
            let usid_list: Vec<String> = usids.iter().map(|u| u.to_string()).collect();
            println!("{}└─ USIDs: \x1b[36m[{}]\x1b[0m", indent, usid_list.join(", "));
        } else {
            let first_few: Vec<String> = usids.iter().take(3).map(|u| u.to_string()).collect();
            let last_few: Vec<String> =
                usids.iter().rev().take(3).rev().map(|u| u.to_string()).collect();

            println!(
                "{}└─ USIDs: \x1b[36m[{}, ... {}, {} total]\x1b[0m",
                indent,
                first_few.join(", "),
                last_few.join(", "),
                usids.len()
            );
        }
    }

    /// Print summary information for opcode snapshots within a frame.
    ///
    /// This method displays program counter ranges, stack depth information,
    /// and other opcode-specific details for all opcode snapshots in the frame.
    fn print_opcode_summary(&self, snapshots: &[&Snapshot<DB>], indent: &str) {
        let opcode_snapshots: Vec<_> = snapshots
            .iter()
            .filter_map(|s| {
                if let SnapshotDetail::Opcode(ref opcode) = s.detail { Some(opcode) } else { None }
            })
            .collect();

        if opcode_snapshots.is_empty() {
            return;
        }

        let pc_range = if opcode_snapshots.len() == 1 {
            format!("PC {}", opcode_snapshots[0].pc)
        } else {
            let min_pc = opcode_snapshots.iter().map(|s| s.pc).min().unwrap_or(0);
            let max_pc = opcode_snapshots.iter().map(|s| s.pc).max().unwrap_or(0);
            format!("PC {min_pc}..{max_pc}")
        };

        let avg_stack: f64 = if !opcode_snapshots.is_empty() {
            opcode_snapshots.iter().map(|s| s.stack.len()).sum::<usize>() as f64
                / opcode_snapshots.len() as f64
        } else {
            0.0
        };

        println!("{indent}├─ Range: \x1b[36m{pc_range}\x1b[0m");
        println!("{indent}└─ Avg stack depth: \x1b[36m{avg_stack:.1}\x1b[0m");
    }
}
