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

//! Display panel for variables, stack, memory, and other debugging information
//!
//! This panel can switch between different display modes based on context.

use super::{EventResponse, PanelTr, PanelType};
use crate::{
    data::DataManager,
    panels::utils,
    ui::{
        borders::BorderPresets,
        colors::ColorScheme,
        status::StatusBar,
        syntax::{SyntaxHighlighter, SyntaxType},
    },
};
use alloy_primitives::{Address, Bytes, U256};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use edb_common::types::{
    Breakpoint, EdbSolValue, HookSnapshotInfoDetail, OpcodeSnapshotInfoDetail, SnapshotInfoDetail,
    SolValueFormatterContext,
};
use eyre::Result;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
};
use revm::state::TransientStorage;
use std::{
    cmp::{self, Ordering},
    collections::{HashMap, HashSet},
    mem,
    sync::Arc,
};
use tracing::debug;

/// Variable display category for hooked snapshots
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VariableCategory {
    /// Expression watches
    Expression,
    /// Local variables from current scope
    Local,
    /// State variables (contract storage)
    State,
}

/// Display modes for the panel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// Show variables (both local and state, for hooked snapshots)
    /// Special note: variables will also include expressions in hooked snapshots
    Variables,
    /// Show expression watches (for both opcode and hooked snapshots)
    Expressions,
    /// Show breakpoints (for both opcode and hooked snapshots)
    Breakpoints,
    /// Show current stack (for opcode snapshots)
    Stack,
    /// Show memory contents (for opcode snapshots)
    Memory,
    /// Show call data (for opcode snapshots)
    CallData,
    /// Show storage state (for opcode snapshots)
    Storage,
    /// Show transient storage state (for opcode snapshots)
    TransientStorage,
}

/// Represents a variable entry for display
#[derive(Debug, Clone, PartialEq)]
struct VariableEntry {
    name: String,
    value: Option<Arc<EdbSolValue>>,
    category: VariableCategory,
    is_multi_line: bool,
    /// For expressions, stores the original expression text
    expression: Option<String>,
}

/// Represents a breakpoint entry for display
#[derive(Debug, Clone, PartialEq)]
struct BreakpointEntry {
    id: usize,
    breakpoint: Breakpoint,
    enabled: bool,
    is_hit: bool,
}

impl cmp::PartialOrd for VariableEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self.category, other.category) {
            (VariableCategory::Expression, VariableCategory::Expression) => {
                // Expressions are always equal in ordering - keep original order
                Some(Ordering::Equal)
            }
            (VariableCategory::Expression, _) => Some(Ordering::Less),
            (_, VariableCategory::Expression) => Some(Ordering::Greater),
            _ => {
                // Both are non-expressions - sort by name
                let a_has_value = if self.value.is_some() { 0 } else { 1 };
                let b_has_value = if other.value.is_some() { 0 } else { 1 };

                Some(
                    self.category
                        .cmp(&other.category)
                        .then(a_has_value.cmp(&b_has_value))
                        .then(self.name.cmp(&other.name)),
                )
            }
        }
    }
}

impl DisplayMode {
    /// Get display name for the mode
    pub fn name(&self) -> &'static str {
        match self {
            Self::Variables => "Variables",
            Self::Expressions => "Expressions",
            Self::Breakpoints => "Breakpoints",
            Self::Stack => "Stack",
            Self::Memory => "Memory",
            Self::CallData => "Call Data",
            Self::Storage => "Storage",
            Self::TransientStorage => "Transient Storage",
        }
    }
}

/// Represents a stack item with potential diff
#[derive(Debug, Clone)]
struct StackItem {
    index: usize,
    value: U256,
    diff_status: DiffStatus,
}

/// Represents a memory chunk with potential diff
#[derive(Debug, Clone)]
struct MemoryChunk {
    offset: usize,
    data: Vec<u8>,
    changed_bytes: Vec<usize>, // Indices of changed bytes within chunk
}

/// Status of an item compared to previous snapshot
#[derive(Debug, Clone, Copy, PartialEq)]
enum DiffStatus {
    Unchanged,
    New,
    Modified,
}

/// Style for storage display items
#[derive(Debug, Clone, Copy)]
enum StorageItemStyle {
    StoreHeader,
    StoreInfo,
    StoreWrite,
    Header,
    SlotLine,
    ChangeLine,
    Normal,
}

/// Display panel implementation
#[derive(Debug)]
pub struct DisplayPanel {
    // ========== Display ==========
    /// Current display mode
    mode: DisplayMode,
    /// Previous display mode before switch mode
    prev_mode: DisplayMode,
    /// Available modes based on snapshot type
    available_modes: Vec<DisplayMode>,
    /// Selected item index
    selected_index: usize,
    /// Scroll offset for auto-scrolling
    scroll_offset: usize,
    /// Content height for viewport calculations
    context_height: usize,
    /// Content width for viewport calculations
    content_width: usize,
    /// Horizontal scroll offset
    horizontal_offset: usize,
    /// Maximum line width for horizontal scrolling
    max_line_width: usize,
    /// Whether this panel is focused
    focused: bool,
    /// Syntax highlighter for Sol values
    syntax_highlighter: SyntaxHighlighter,

    // ========== Data (Flag) ==========
    /// Current execution snapshot
    current_execution_snapshot: Option<usize>,
    /// Whether current snapshot is opcode or hooked
    is_opcode_snapshot: bool,

    // ========== Data (Cached) ==========
    /// Stack items with diff status
    stack_items: Vec<StackItem>,
    /// Memory chunks with diff status
    memory_chunks: Vec<MemoryChunk>,
    /// Raw calldata
    calldata: Bytes,
    /// Storage changes (slot -> (old_value, new_value))
    storage_changes: HashMap<U256, (U256, U256)>,
    /// Transient storage data
    transient_storage: HashMap<U256, U256>,
    /// Previous opcode info for SSTORE/TSTORE detection
    prev_opcode: Option<u8>,
    /// Previous stack for SSTORE/TSTORE slot extraction
    prev_stack: Option<Vec<U256>>,
    /// Breakpoint entries from execution manager
    breakpoints: Vec<BreakpointEntry>,
    /// Hit breakpoint IDs for current snapshot
    hit_breakpoints: Vec<usize>,
    /// All variable entries (local and state) from hook snapshots
    variables: Vec<VariableEntry>,
    /// Variables that are toggled to multi-line (persisted by name)
    multi_line_variables: HashSet<String>,
    /// Expression watch entries
    expressions: Vec<VariableEntry>,
    /// Expressions that are toggled to multi-line (persisted by expression)
    multi_line_expressions: HashSet<String>,
    /// Cached display line count for storage mode
    storage_display_lines: usize,
    /// Cached display line count for transient storage mode
    tstorage_display_lines: usize,
    /// Current display line count (cached)
    displayed_line_count: usize,
}

impl DisplayPanel {
    /// Create a new display panel
    pub fn new() -> Self {
        Self {
            mode: DisplayMode::Stack,
            prev_mode: DisplayMode::Variables,
            available_modes: vec![DisplayMode::Stack],
            selected_index: 0,
            scroll_offset: 0,
            context_height: 0,
            content_width: 0,
            horizontal_offset: 0,
            max_line_width: 0,
            focused: false,
            current_execution_snapshot: None,
            is_opcode_snapshot: true,
            stack_items: Vec::new(),
            memory_chunks: Vec::new(),
            calldata: Bytes::new(),
            storage_changes: HashMap::new(),
            transient_storage: HashMap::new(),
            prev_opcode: None,
            prev_stack: None,
            variables: Vec::new(),
            multi_line_variables: HashSet::new(),
            expressions: Vec::new(),
            multi_line_expressions: HashSet::new(),
            breakpoints: Vec::new(),
            hit_breakpoints: Vec::new(),
            storage_display_lines: 0,
            tstorage_display_lines: 0,
            syntax_highlighter: SyntaxHighlighter::new(),
            displayed_line_count: 0,
        }
    }

    /// Update snapshot data from data manager
    fn update_snapshot_data(&mut self, dm: &mut DataManager) -> Option<()> {
        let current_snapshot = dm.execution.get_current_snapshot();

        // Expressions could be updated anytime (and hence forcing variables to refresh)
        if self.update_expression_watches(dm, current_snapshot) {
            self.variables.retain(|var| var.category != VariableCategory::Expression);
            self.variables.splice(0..0, self.expressions.iter().cloned());
        }

        // Update breakpoints data
        self.update_breakpoints_data(dm, current_snapshot);

        // Check if snapshot changed
        if self.current_execution_snapshot == Some(current_snapshot) {
            return Some(());
        }

        // Get snapshot info
        let snapshot_info = dm.execution.get_snapshot_info(current_snapshot)?.clone();

        // Determine snapshot type and update available modes
        match snapshot_info.detail() {
            SnapshotInfoDetail::Opcode(opcode_detail) => {
                self.is_opcode_snapshot = true;
                self.available_modes = vec![
                    DisplayMode::Stack,
                    DisplayMode::Memory,
                    DisplayMode::CallData,
                    DisplayMode::Storage,
                    DisplayMode::TransientStorage,
                    DisplayMode::Expressions,
                    DisplayMode::Breakpoints,
                ];

                // Update opcode-specific data
                self.update_opcode_data(opcode_detail, current_snapshot, dm)?;

                // Switch to Stack mode if not already in an opcode mode
                if !self.available_modes.contains(&self.mode) {
                    mem::swap(&mut self.prev_mode, &mut self.mode);
                    self.selected_index = 0;
                    self.scroll_offset = 0;
                }
            }
            SnapshotInfoDetail::Hook(hook_detail) => {
                self.is_opcode_snapshot = false;
                self.available_modes = vec![DisplayMode::Variables, DisplayMode::Breakpoints];

                // Update hook-specific data
                self.update_hook_data(hook_detail);

                // Switch to Variables mode if not already in a hook mode
                if !self.available_modes.contains(&self.mode) {
                    // Swap prev_mode and mode
                    mem::swap(&mut self.prev_mode, &mut self.mode);
                    self.selected_index = 0;
                    self.scroll_offset = 0;
                }
            }
        }

        self.current_execution_snapshot = Some(current_snapshot);

        Some(())
    }

    /// Update data for hook snapshots
    fn update_hook_data(&mut self, hook_detail: &HookSnapshotInfoDetail) {
        // Let's include expression first
        self.variables.retain(|var| var.category == VariableCategory::Expression);

        // Extract local variables
        for (name, value_opt) in &hook_detail.locals {
            let is_multi_line = self.multi_line_variables.contains(name);
            self.variables.push(VariableEntry {
                name: name.clone(),
                value: value_opt.clone(),
                category: VariableCategory::Local,
                is_multi_line,
                expression: None,
            });
        }

        // Extract state variables from actual snapshot data
        for (name, value_opt) in &hook_detail.state_variables {
            let is_multi_line = self.multi_line_variables.contains(name);
            self.variables.push(VariableEntry {
                name: name.clone(),
                value: value_opt.clone(),
                category: VariableCategory::State,
                is_multi_line,
                expression: None,
            });
        }

        self.variables.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    }

    /// Update data for opcode snapshots
    fn update_opcode_data(
        &mut self,
        opcode_detail: &OpcodeSnapshotInfoDetail,
        current_id: usize,
        dm: &mut DataManager,
    ) -> Option<()> {
        let current_addr = dm.execution.get_current_address()?;

        // Get previous snapshot for diff calculation
        let prev_snapshot_id = dm.execution.get_snapshot_info(current_id)?.prev_id();

        // Get storage changes from execution manager
        let storage_changes = dm.execution.get_storage_diff(current_id)?;

        // At this point, we have collected all the information and we can update
        // data safely
        self.storage_changes = storage_changes.clone();

        let prev_snapshot_detail =
            dm.execution.get_snapshot_info(prev_snapshot_id).and_then(|info| match info.detail() {
                SnapshotInfoDetail::Opcode(detail) => Some(detail.clone()),
                _ => None,
            });

        // Store previous opcode and stack for SSTORE/TSTORE detection
        if let Some(prev_detail) = &prev_snapshot_detail {
            self.prev_opcode = Some(prev_detail.opcode);
            self.prev_stack = Some(prev_detail.stack.clone());
        } else {
            self.prev_opcode = None;
            self.prev_stack = None;
        }

        // Update stack with diff
        self.update_stack_diff(
            &opcode_detail.stack,
            prev_snapshot_detail.as_ref().map(|d| &d.stack),
        );

        // Update memory with diff
        self.update_memory_diff(
            &opcode_detail.memory,
            prev_snapshot_detail.as_ref().map(|d| &d.memory),
        );

        // Update calldata
        self.calldata = opcode_detail.calldata.clone();

        // Update transient storage (only for current address)
        self.update_transient_storage(&opcode_detail.transient_storage, current_addr);

        Some(())
    }

    /// Update stack items with diff status
    fn update_stack_diff(&mut self, current_stack: &[U256], prev_stack: Option<&Vec<U256>>) {
        self.stack_items.clear();

        if let Some(prev) = prev_stack {
            // Compare stacks to find diffs
            for (idx, value) in current_stack.iter().enumerate() {
                let diff_status = if idx >= prev.len() {
                    DiffStatus::New
                } else if prev[idx] != *value {
                    DiffStatus::Modified
                } else {
                    DiffStatus::Unchanged
                };

                self.stack_items.push(StackItem { index: idx, value: *value, diff_status });
            }
        } else {
            // First snapshot - all items are new
            for (idx, value) in current_stack.iter().enumerate() {
                self.stack_items.push(StackItem {
                    index: idx,
                    value: *value,
                    diff_status: DiffStatus::New,
                });
            }
        }
    }

    /// Update memory chunks with diff status
    fn update_memory_diff(&mut self, current_memory: &[u8], prev_memory: Option<&Vec<u8>>) {
        self.memory_chunks.clear();

        // Process memory in 32-byte chunks
        const CHUNK_SIZE: usize = 32;

        for offset in (0..current_memory.len()).step_by(CHUNK_SIZE) {
            let end = (offset + CHUNK_SIZE).min(current_memory.len());
            let chunk_data = current_memory[offset..end].to_vec();

            // Find changed bytes if we have previous memory
            let changed_bytes = if let Some(prev) = prev_memory {
                let mut changes = Vec::new();
                for (i, item) in chunk_data.iter().enumerate() {
                    let mem_idx = offset + i;
                    if mem_idx >= prev.len() || prev[mem_idx] != *item {
                        changes.push(i);
                    }
                }
                changes
            } else {
                // All bytes are new
                (0..chunk_data.len()).collect()
            };

            if !chunk_data.is_empty() {
                self.memory_chunks.push(MemoryChunk { offset, data: chunk_data, changed_bytes });
            }
        }
    }

    /// Update transient storage
    fn update_transient_storage(&mut self, tstorage: &TransientStorage, current_addr: Address) {
        self.transient_storage.clear();

        // Only copy transient storage data for the current address
        for ((addr, slot), value) in tstorage.iter() {
            if *addr == current_addr {
                self.transient_storage.insert(*slot, *value);
            }
        }
    }

    /// Calculate the maximum line width for horizontal scrolling
    fn calculate_max_line_width(&mut self, dm: &mut DataManager) {
        self.max_line_width = match self.mode {
            DisplayMode::Stack => {
                // Find longest stack item (index + hex + decoded + diff indicator)
                self.stack_items
                    .iter()
                    .map(|item| {
                        let index_str = format!("[{:2}]", item.index);
                        let value_str = utils::format_value_with_decode(&item.value);
                        let diff_indicator = match item.diff_status {
                            DiffStatus::New => " [NEW]",
                            DiffStatus::Modified => " [CHG]",
                            _ => "",
                        };
                        format!("{index_str} {value_str} {diff_indicator}").len()
                    })
                    .max()
                    .unwrap_or(0)
            }
            DisplayMode::Memory => {
                // Memory lines: "0x1234: " + hex bytes + "  " + ascii
                // 32 bytes = 64 hex chars + 31 spaces + 2 spaces + 32 ascii = ~129 chars
                // Plus offset (8 chars) = ~137 chars
                145
            }
            DisplayMode::CallData => {
                // Calldata: hex only format
                // 32 bytes = 64 hex chars + 31 spaces + offset (8 chars) = ~103 chars
                110
            }
            DisplayMode::Storage => {
                // Calculate actual max width by checking all storage items
                let mut max_width = 0;

                // Check SSTORE/TSTORE operation headers if they exist
                if self.prev_opcode == Some(0x55) || self.prev_opcode == Some(0x5D) {
                    max_width = max_width.max(20); // "▶ SSTORE Operation" etc.
                }

                // Check storage changes
                for (slot, (old_value, new_value)) in &self.storage_changes {
                    let slot_line = format!("• Slot: {slot:#066x}");
                    let old_line =
                        format!("  Old:  {}", utils::format_value_with_decode(old_value));
                    let new_line =
                        format!("  New:  {}", utils::format_value_with_decode(new_value));

                    max_width = max_width.max(slot_line.len());
                    max_width = max_width.max(old_line.len());
                    max_width = max_width.max(new_line.len());
                }

                // Add some buffer for headers
                let header_width = "─── All Storage Changes ───".len();
                max_width.max(header_width).max(80) // At least 80 chars
            }
            DisplayMode::TransientStorage => {
                // Calculate actual max width by checking all transient storage items
                let mut max_width = 0;

                // Check TSTORE operation headers if they exist
                if self.prev_opcode == Some(0x5D) {
                    max_width = max_width.max(20); // "▶ TSTORE Operation"
                }

                // Check transient storage items
                for (slot, value) in &self.transient_storage {
                    let slot_line = format!("• Slot: {slot:#066x}");
                    let val_line = format!("  Val:  {}", utils::format_value_with_decode(value));

                    max_width = max_width.max(slot_line.len());
                    max_width = max_width.max(val_line.len());
                }

                // Add some buffer for headers
                let header_width = "─── Transient Storage ───".len();
                max_width.max(header_width).max(80) // At least 80 chars
            }
            DisplayMode::Variables => {
                // Calculate actual max width for variables by formatting each entry
                self.calculate_variables_max_width(dm)
            }
            DisplayMode::Expressions => {
                // Calculate actual max width for expressions by formatting each entry
                self.calculate_expressions_max_width(dm)
            }
            DisplayMode::Breakpoints => self.calculate_breakpoints_max_width(dm),
        };
    }

    /// Switch to next display mode
    fn next_mode(&mut self) {
        if let Some(current_idx) = self.available_modes.iter().position(|m| *m == self.mode) {
            let next_idx = (current_idx + 1) % self.available_modes.len();
            self.mode = self.available_modes[next_idx];
            self.selected_index = 0;
            self.scroll_offset = 0;
            self.horizontal_offset = 0;
            debug!("Switched to display mode: {:?}", self.mode);
        }
    }

    /// Switch to previous display mode
    fn prev_mode(&mut self) {
        if let Some(current_idx) = self.available_modes.iter().position(|m| *m == self.mode) {
            let prev_idx =
                if current_idx == 0 { self.available_modes.len() - 1 } else { current_idx - 1 };
            self.mode = self.available_modes[prev_idx];
            self.selected_index = 0;
            self.scroll_offset = 0;
            self.horizontal_offset = 0;
            debug!("Switched to display mode: {:?}", self.mode);
        }
    }

    /// Calculate the number of display lines for storage
    fn calculate_storage_display_lines(&self, dm: &mut DataManager) -> usize {
        let mut count = 0;

        // Check for SSTORE/TSTORE indicator
        if let Some(current_snapshot) = self.current_execution_snapshot
            && let Some(info) = dm.execution.get_snapshot_info(current_snapshot)
        {
            let prev_snapshot_id = info.prev_id();
            if let Some(prev_info) = dm.execution.get_snapshot_info(prev_snapshot_id)
                && let SnapshotInfoDetail::Opcode(detail) = prev_info.detail()
                && (detail.opcode == 0x55 || detail.opcode == 0x5D)
                && !detail.stack.is_empty()
            {
                count += 7; // SSTORE/TSTORE operation lines (simpler format)
            }
        }

        if !self.storage_changes.is_empty() {
            if count > 0 {
                count += 2; // Header lines for "Storage Changes"
            }
            count += self.storage_changes.len() * 4; // 4 lines per change (slot, old, new, separator)
        }

        count.max(1) // At least 1 line
    }

    /// Calculate the number of display lines for transient storage
    fn calculate_tstorage_display_lines(&self, dm: &mut DataManager) -> usize {
        let mut count = 0;

        // Check for TSTORE indicator
        if let Some(current_snapshot) = self.current_execution_snapshot
            && let Some(info) = dm.execution.get_snapshot_info(current_snapshot)
        {
            let prev_snapshot_id = info.prev_id();
            if let Some(prev_info) = dm.execution.get_snapshot_info(prev_snapshot_id)
                && let SnapshotInfoDetail::Opcode(detail) = prev_info.detail()
                && detail.opcode == 0x5D
                && !detail.stack.is_empty()
            {
                count += 5; // TSTORE operation lines (simpler format)
            }
        }

        if !self.transient_storage.is_empty() {
            if count > 0 {
                count += 2; // Header lines
            }
            count += self.transient_storage.len() * 3; // 3 lines per item (slot, value, separator)
        }

        count.max(1) // At least 1 line
    }

    /// Format a variable entry for display with syntax highlighting
    fn format_variable_entry(
        &self,
        entry: &VariableEntry,
        dm: &mut DataManager,
    ) -> Vec<Span<'static>> {
        // Add category prefix for better organization
        let prefix = match entry.category {
            VariableCategory::Local => "🔵 ",      // Blue circle for local
            VariableCategory::State => "🟠 ",      // Orange circle for state
            VariableCategory::Expression => "🟢 ", // Green circle for expressions
        };

        let mut spans = vec![Span::raw(prefix.to_string())];
        spans.push(Span::raw(format!("{} = ", entry.name)));

        // Format and highlight the value
        match &entry.value {
            Some(value) => {
                let ctx = if entry.is_multi_line {
                    SolValueFormatterContext::new().with_ty(true).multi_line(true)
                } else {
                    SolValueFormatterContext::new().with_ty(true)
                };
                let value_str = dm.resolver.resolve_sol_value(value, Some(ctx));

                // Apply syntax highlighting to the value string
                spans.extend(self.highlight_solidity_value(value_str, dm));
            }
            None => {
                // Use comment color for unknown values
                spans.push(Span::styled(
                    "???".to_string(),
                    Style::default().fg(dm.theme.comment_color).add_modifier(Modifier::ITALIC),
                ));
            }
        };

        // Add multi-line indicator
        if entry.is_multi_line {
            spans.push(Span::raw(" ⬇".to_string()));
        }

        spans
    }

    /// Apply syntax highlighting to Solidity value strings
    fn highlight_solidity_value(&self, value: String, dm: &DataManager) -> Vec<Span<'static>> {
        let tokens = self.syntax_highlighter.tokenize(&value, SyntaxType::Solidity);
        let mut spans = Vec::new();
        let mut last_end = 0;

        // Apply syntax highlighting to tokens (same pattern as code and trace panels)
        for token in tokens {
            // Add any unhighlighted text before this token
            if token.start > last_end {
                let unhighlighted = value[last_end..token.start].to_owned();
                if !unhighlighted.is_empty() {
                    spans.push(Span::raw(unhighlighted));
                }
            }

            // Add the highlighted token
            let token_text = value[token.start..token.end].to_owned();
            let token_style = self.syntax_highlighter.get_token_style(token.token_type, &dm.theme);
            spans.push(Span::styled(token_text, token_style));
            last_end = token.end;
        }

        // Add any remaining unhighlighted text
        if last_end < value.len() {
            let remaining = value[last_end..].to_owned();
            if !remaining.is_empty() {
                spans.push(Span::raw(remaining));
            }
        }

        spans
    }

    /// Update expression watches for current snapshot
    /// Returns true if the expressions data has actually changed
    fn update_expression_watches(&mut self, dm: &mut DataManager, current_snapshot: usize) -> bool {
        // Store old values for comparison
        let old_values = mem::take(&mut self.expressions);

        for (expr_id, expression) in dm.watcher.list_expressions() {
            let is_multi_line = self.multi_line_expressions.contains(expression);
            let name = format!("${expr_id}: {expression}");

            // Evaluate expression on current snapshot
            let value = match dm.resolver.eval_on_snapshot(current_snapshot, expression) {
                Some(Ok(sol_value)) => Some(Arc::new(sol_value.clone())),
                _ => None, // N/A for None or Some(Err)
            };

            self.expressions.push(VariableEntry {
                name,
                value,
                category: VariableCategory::Expression,
                is_multi_line,
                expression: Some(expression.clone()),
            });
        }

        old_values.len() != self.expressions.len()
            || self.expressions.iter().zip(old_values.iter()).any(|(a, b)| a != b)
    }

    /// Update breakpoints data from execution manager
    fn update_breakpoints_data(&mut self, dm: &mut DataManager, current_snapshot: usize) {
        // Get hit breakpoints for current snapshot
        self.hit_breakpoints = dm.execution.get_hit_breakpoints(current_snapshot);

        // Update breakpoint entries
        self.breakpoints.clear();
        for (id, breakpoint, enabled) in dm.execution.list_breakpoints() {
            let is_hit = self.hit_breakpoints.contains(&id);
            self.breakpoints.push(BreakpointEntry {
                id,
                breakpoint: breakpoint.clone(),
                enabled,
                is_hit,
            });
        }
    }

    /// Calculate the maximum width for variables display
    fn calculate_variables_max_width(&self, dm: &mut DataManager) -> usize {
        let mut max_width = 0;

        for entry in &self.variables {
            // Calculate prefix width (Unicode emojis take more space)
            let prefix_width = 4; // all 🔵, 🟠, and 🟢 take roughly 4 display chars
            let name_width = entry.name.len() + 4; // " = "

            // Calculate value width by actually formatting it
            let value_width = match &entry.value {
                Some(value) => {
                    let ctx = if entry.is_multi_line {
                        SolValueFormatterContext::new().with_ty(true).multi_line(true)
                    } else {
                        SolValueFormatterContext::new().with_ty(true)
                    };
                    let formatted = dm.resolver.resolve_sol_value(value, Some(ctx));

                    // For multi-line values, find the longest line
                    if entry.is_multi_line {
                        formatted.lines().map(|line| line.len()).max().unwrap_or(formatted.len())
                    } else {
                        formatted.len()
                    }
                }
                None => 3, // "???"
            };

            let indicator_width = if entry.is_multi_line { 3 } else { 0 }; // " ⬇"

            let total_width = prefix_width + name_width + value_width + indicator_width;
            max_width = max_width.max(total_width);
        }

        max_width.max(50) // Minimum width
    }

    /// Calculate the actual number of display lines for variables (accounting for multi-line)
    fn calculate_variables_display_lines(&self, dm: &mut DataManager) -> usize {
        let mut total_lines = 0;

        for entry in &self.variables {
            if entry.is_multi_line && entry.value.is_some() {
                // Multi-line variables take multiple lines - actually count them
                if let Some(value) = &entry.value {
                    let ctx = SolValueFormatterContext::new().with_ty(true).multi_line(true);
                    let formatted = dm.resolver.resolve_sol_value(value, Some(ctx));
                    total_lines += formatted.lines().count().max(1);
                } else {
                    total_lines += 1;
                }
            } else {
                total_lines += 1;
            }
        }

        total_lines.max(1)
    }

    /// Calculate the maximum width for expressions display
    fn calculate_expressions_max_width(&self, dm: &mut DataManager) -> usize {
        let mut max_width = 0;

        for entry in &self.expressions {
            // Calculate prefix width (Unicode emojis take more space)
            let prefix_width = 4; // 🔹 takes roughly 4 display chars
            let name_width = entry.name.len() + 3; // " = "

            // Calculate value width by actually formatting it
            let value_width = match &entry.value {
                Some(value) => {
                    let ctx = if entry.is_multi_line {
                        SolValueFormatterContext::new().with_ty(true).multi_line(true)
                    } else {
                        SolValueFormatterContext::new().with_ty(true)
                    };
                    let formatted = dm.resolver.resolve_sol_value(value, Some(ctx));

                    // For multi-line values, find the longest line
                    if entry.is_multi_line {
                        formatted.lines().map(|line| line.len()).max().unwrap_or(formatted.len())
                    } else {
                        formatted.len()
                    }
                }
                None => 3, // "N/A"
            };

            let indicator_width = if entry.is_multi_line { 3 } else { 0 }; // " ⬇"

            let total_width = prefix_width + name_width + value_width + indicator_width;
            max_width = max_width.max(total_width);
        }

        max_width.max(50) // Minimum width
    }

    /// Calculate the actual number of display lines for expressions (accounting for multi-line)
    fn calculate_expressions_display_lines(&self, dm: &mut DataManager) -> usize {
        let mut total_lines = 0;

        for entry in &self.expressions {
            if entry.is_multi_line && entry.value.is_some() {
                // Multi-line expressions take multiple lines - actually count them
                if let Some(value) = &entry.value {
                    let ctx = SolValueFormatterContext::new().with_ty(true).multi_line(true);
                    let formatted = dm.resolver.resolve_sol_value(value, Some(ctx));
                    total_lines += formatted.lines().count().max(1);
                } else {
                    total_lines += 1;
                }
            } else {
                total_lines += 1;
            }
        }

        total_lines.max(1)
    }

    /// Calculate the maximum width for breakpoints display
    fn calculate_breakpoints_max_width(&self, dm: &mut DataManager) -> usize {
        let mut max_width = 0;

        for entry in &self.breakpoints {
            // Calculate width for the formatted breakpoint entry
            let mut width = 0;

            // Status indicator: 🔵/🔴 + space = ~4 chars
            width += 4;

            // Hit indicator: 🎯 + space = ~4 chars (if hit)
            if entry.is_hit {
                width += 4;
            }

            // ID: #1 + space = variable
            width += format!("#{} ", entry.id).len();

            // Location display
            if let Some(loc) = &entry.breakpoint.loc {
                width +=
                    loc.display(dm.resolver.resolve_address_label(loc.bytecode_address())).len();
            } else {
                width += 12; // "no location"
            }

            // Condition display (if present)
            if let Some(condition) = &entry.breakpoint.condition {
                width += format!(" if {condition}").len();
            }

            max_width = max_width.max(width);
        }

        max_width.max(50) // Minimum width
    }

    /// Format a breakpoint entry for display with visual indicators
    fn format_breakpoint_entry(
        &self,
        entry: &BreakpointEntry,
        dm: &mut DataManager,
    ) -> Vec<Span<'static>> {
        let mut spans = Vec::new();

        // Determine base style for the entire entry based on state
        let base_style = if entry.is_hit {
            Style::default().fg(dm.theme.warning_color).add_modifier(Modifier::BOLD)
        } else if !entry.enabled {
            Style::default().fg(dm.theme.comment_color).add_modifier(Modifier::DIM)
        } else {
            Style::default()
        };

        // Breakpoint status indicator (3 states)
        let status_indicator = match (entry.enabled, entry.is_hit) {
            (true, true) => "🎯 ",  // Enabled and hit (red circle with hole)
            (true, false) => "🔵 ", // Enabled but not hit (filled circle)
            (false, _) => "🔴 ",    // Disabled (empty circle)
        };
        spans.push(Span::styled(status_indicator.to_string(), base_style));

        // Breakpoint ID (use base style)
        spans.push(Span::styled(format!("#{} ", entry.id), base_style));

        // Location (use info color unless hit or disabled)
        let location_style = if entry.is_hit {
            Style::default().fg(dm.theme.warning_color).add_modifier(Modifier::BOLD)
        } else if !entry.enabled {
            Style::default().fg(dm.theme.comment_color).add_modifier(Modifier::DIM)
        } else {
            Style::default().fg(dm.theme.info_color)
        };

        if let Some(loc) = &entry.breakpoint.loc {
            spans.push(Span::styled(
                loc.display(dm.resolver.resolve_address_label(loc.bytecode_address())),
                location_style,
            ));
        } else {
            spans.push(Span::styled("no location".to_string(), location_style));
        }

        // Condition (if present)
        if let Some(condition) = &entry.breakpoint.condition {
            spans.push(Span::styled(" if ", base_style));

            // Condition value gets success color unless hit or disabled
            let condition_style = if entry.is_hit {
                Style::default().fg(dm.theme.warning_color).add_modifier(Modifier::BOLD)
            } else if !entry.enabled {
                Style::default().fg(dm.theme.comment_color).add_modifier(Modifier::DIM)
            } else {
                Style::default().fg(dm.theme.success_color)
            };

            spans.push(Span::styled(condition.to_string(), condition_style));
        }

        spans
    }

    /// Apply horizontal offset to a line for horizontal scrolling
    fn apply_horizontal_offset<'a>(&self, line: Line<'a>) -> Line<'a> {
        if self.horizontal_offset == 0 {
            return line;
        }
        let mut accumulated_width = 0;
        let mut new_spans = Vec::new();
        let mut started_content = false;
        for span in line.spans {
            let span_width = span.content.chars().count();
            if accumulated_width + span_width <= self.horizontal_offset {
                accumulated_width += span_width;
            } else if accumulated_width >= self.horizontal_offset {
                new_spans.push(span);
                started_content = true;
            } else {
                let skip_chars = self.horizontal_offset - accumulated_width;
                let visible_content: String = span.content.chars().skip(skip_chars).collect();
                if !visible_content.is_empty() {
                    new_spans.push(Span::styled(visible_content, span.style));
                    started_content = true;
                }
                accumulated_width += span_width;
            }
        }
        if !started_content {
            return Line::from("");
        }
        Line::from(new_spans)
    }

    /// Toggle multi-line display for the currently selected variable or expression
    fn toggle_multiline(&mut self, _dm: &mut DataManager) {
        match self.mode {
            DisplayMode::Variables => {
                if let Some(entry) = self.variables.get_mut(self.selected_index) {
                    entry.is_multi_line = !entry.is_multi_line;

                    // Persist the toggle state
                    if entry.is_multi_line {
                        self.multi_line_variables.insert(entry.name.clone());
                    } else {
                        self.multi_line_variables.remove(&entry.name);
                    }
                }
            }
            DisplayMode::Expressions => {
                if let Some(entry) = self.expressions.get_mut(self.selected_index)
                    && let Some(expr) = &entry.expression
                {
                    // Toggle the multi-line state
                    entry.is_multi_line = !entry.is_multi_line;

                    // Persist the toggle state
                    if entry.is_multi_line {
                        self.multi_line_expressions.insert(expr.clone());
                    } else {
                        self.multi_line_expressions.remove(expr);
                    }
                }
            }
            _ => {}
        }
    }

    /// Toggle enable/disable state for the currently selected breakpoint
    fn toggle_breakpoint(&mut self, dm: &mut DataManager) {
        if let Some(entry) = self.breakpoints.get(self.selected_index) {
            let breakpoint_id = entry.id;

            if entry.enabled {
                // Disable the breakpoint
                if let Err(e) = dm.execution.disable_breakpoint(breakpoint_id) {
                    debug!("Failed to disable breakpoint {}: {}", breakpoint_id, e);
                } else {
                    debug!("Disabled breakpoint #{}", breakpoint_id);
                }
            } else {
                // Enable the breakpoint
                if let Err(e) = dm.execution.enable_breakpoint(breakpoint_id) {
                    debug!("Failed to enable breakpoint {}: {}", breakpoint_id, e);
                } else {
                    debug!("Enabled breakpoint #{}", breakpoint_id);
                }
            }

            // Update the local breakpoint data to reflect the change immediately
            // This will be properly updated on the next data refresh
            if let Some(entry) = self.breakpoints.get_mut(self.selected_index) {
                entry.enabled = !entry.enabled;
            }
        }
    }

    /// Move selection up with auto-scrolling
    fn move_up(&mut self, n: usize) {
        self.selected_index = self.selected_index.saturating_sub(n);

        // Auto-scroll up if selection moves above visible area
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        }
    }

    /// Move selection down with auto-scrolling
    fn move_down(&mut self, n: usize) {
        let max_items = self.displayed_line_count;

        self.selected_index =
            self.selected_index.saturating_add(n).min(max_items.saturating_sub(1));
        let viewport_height = self.context_height;
        // Auto-scroll down if selection moves below visible area
        if self.selected_index >= self.scroll_offset + viewport_height {
            self.scroll_offset = (self.selected_index + 1).saturating_sub(viewport_height);
        }
    }

    /// Calculate the number of lines that will be displayed
    fn calculate_context_lines(&self, dm: &mut DataManager) -> usize {
        match self.mode {
            DisplayMode::Stack => self.stack_items.len(),
            DisplayMode::Memory => self.memory_chunks.len(),
            DisplayMode::CallData => {
                if self.calldata.is_empty() {
                    0
                } else {
                    self.calldata.len().div_ceil(32)
                }
            }
            DisplayMode::Storage => self.storage_display_lines.max(1),
            DisplayMode::TransientStorage => self.tstorage_display_lines.max(1),
            DisplayMode::Variables => self.calculate_variables_display_lines(dm),
            DisplayMode::Expressions => self.calculate_expressions_display_lines(dm),
            DisplayMode::Breakpoints => self.breakpoints.len(),
        }
    }

    /// Render stack display
    fn render_stack(&mut self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        if self.stack_items.is_empty() {
            let paragraph = Paragraph::new("Stack is empty").block(BorderPresets::display(
                self.focused,
                self.title(dm),
                dm.theme.focused_border,
                dm.theme.unfocused_border,
            ));
            frame.render_widget(paragraph, area);
            return;
        }

        // Create list items for stack display (reversed - stack top first)
        let items: Vec<ListItem<'_>> = self
            .stack_items
            .iter()
            .rev() // Reverse to show stack top first
            .enumerate()
            .skip(self.scroll_offset)
            .take(self.context_height)
            .map(|(display_idx, item)| {
                let is_selected = display_idx == self.selected_index;

                // Format the stack item
                let index_str = format!("[{:2}]", item.index);
                let value_str = utils::format_value_with_decode(&item.value);

                // Add diff indicator
                let diff_indicator = match item.diff_status {
                    DiffStatus::New => " [NEW]",
                    DiffStatus::Modified => " [CHG]",
                    _ => "",
                };

                // Create line with spans for better styling
                let mut line_spans =
                    vec![Span::raw(index_str), Span::raw(" "), Span::raw(value_str)];

                if !diff_indicator.is_empty() {
                    line_spans.push(Span::raw(diff_indicator));
                }

                let line = Line::from(line_spans);
                let formatted_line = self.apply_horizontal_offset(line);

                // Apply styling based on diff status and selection
                let mut style = if is_selected && self.focused {
                    Style::default().bg(dm.theme.selection_bg).fg(dm.theme.selection_fg)
                } else {
                    Style::default()
                };

                // Highlight based on diff status
                if item.diff_status == DiffStatus::New {
                    style = style.fg(dm.theme.success_color);
                } else if item.diff_status == DiffStatus::Modified {
                    style = style.fg(dm.theme.warning_color);
                }

                ListItem::new(formatted_line).style(style)
            })
            .collect();

        let list = List::new(items).block(BorderPresets::display(
            self.focused,
            self.title(dm),
            dm.theme.focused_border,
            dm.theme.unfocused_border,
        ));

        frame.render_widget(list, area);
        self.render_status_and_help(frame, area, dm);
    }

    /// Render memory display
    fn render_memory(&mut self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        if self.memory_chunks.is_empty() {
            let paragraph = Paragraph::new("Memory is empty").block(BorderPresets::display(
                self.focused,
                self.title(dm),
                dm.theme.focused_border,
                dm.theme.unfocused_border,
            ));
            frame.render_widget(paragraph, area);
            return;
        }

        // Create list items for memory display
        let items: Vec<ListItem<'_>> = self
            .memory_chunks
            .iter()
            .enumerate()
            .skip(self.scroll_offset)
            .take(self.context_height)
            .map(|(display_idx, chunk)| {
                let is_selected = display_idx == self.selected_index;

                // Format offset
                let offset_str = format!("0x{:04x}:", chunk.offset);

                // Format bytes with highlighting
                let byte_spans =
                    format_bytes_with_decode(&chunk.data, &chunk.changed_bytes, &dm.theme);

                let mut line = vec![Span::raw(offset_str), Span::raw(" ")];
                line.extend(byte_spans);

                let line_obj = Line::from(line);
                let formatted_line = self.apply_horizontal_offset(line_obj);

                let style = if is_selected && self.focused {
                    Style::default().bg(dm.theme.selection_bg).fg(dm.theme.selection_fg)
                } else {
                    Style::default()
                };

                ListItem::new(formatted_line).style(style)
            })
            .collect();

        let list = List::new(items).block(BorderPresets::display(
            self.focused,
            self.title(dm),
            dm.theme.focused_border,
            dm.theme.unfocused_border,
        ));

        frame.render_widget(list, area);
        self.render_status_and_help(frame, area, dm);
    }

    /// Render calldata display
    fn render_calldata(&mut self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        if self.calldata.is_empty() {
            let paragraph = Paragraph::new("No calldata").block(BorderPresets::display(
                self.focused,
                self.title(dm),
                dm.theme.focused_border,
                dm.theme.unfocused_border,
            ));
            frame.render_widget(paragraph, area);
            return;
        }

        // Display calldata in 32-byte chunks
        let mut items = Vec::new();
        let calldata_bytes = &self.calldata[..];

        for (chunk_idx, chunk) in calldata_bytes.chunks(32).enumerate() {
            let offset = chunk_idx * 32;
            let offset_str = format!("0x{offset:04x}:");

            // Format bytes (hex only for calldata, no ASCII decoding)
            let byte_spans = format_bytes_hex_only(chunk, &[], &dm.theme);

            let mut line = vec![Span::raw(offset_str), Span::raw(" ")];
            line.extend(byte_spans);

            let line_obj = Line::from(line);
            let formatted_line = self.apply_horizontal_offset(line_obj);
            items.push(ListItem::new(formatted_line));
        }

        // Apply scrolling and selection
        let visible_items: Vec<ListItem<'_>> = items
            .into_iter()
            .enumerate()
            .skip(self.scroll_offset)
            .take(self.context_height)
            .map(|(display_idx, item)| {
                let is_selected = display_idx == self.selected_index;
                let style = if is_selected && self.focused {
                    Style::default().bg(dm.theme.selection_bg).fg(dm.theme.selection_fg)
                } else {
                    Style::default()
                };
                item.style(style)
            })
            .collect();

        let list = List::new(visible_items).block(BorderPresets::display(
            self.focused,
            self.title(dm),
            dm.theme.focused_border,
            dm.theme.unfocused_border,
        ));

        frame.render_widget(list, area);
        self.render_status_and_help(frame, area, dm);
    }

    /// Render storage display
    fn render_storage(&mut self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        // Update cached display line count
        self.storage_display_lines = self.calculate_storage_display_lines(dm);

        let mut display_items = Vec::new();
        let mut item_styles = Vec::new();

        // Get current snapshot ID
        let current_snapshot = if let Some(id) = self.current_execution_snapshot {
            id
        } else {
            // No snapshot available
            let paragraph =
                Paragraph::new("No snapshot data available").block(BorderPresets::display(
                    self.focused,
                    self.title(dm),
                    dm.theme.focused_border,
                    dm.theme.unfocused_border,
                ));
            frame.render_widget(paragraph, area);
            return;
        };

        // Get previous snapshot info to check for SSTORE/TSTORE
        let prev_snapshot_id = if let Some(info) = dm.execution.get_snapshot_info(current_snapshot)
        {
            info.prev_id()
        } else {
            0
        };

        let prev_snapshot_detail =
            dm.execution.get_snapshot_info(prev_snapshot_id).and_then(|info| match info.detail() {
                SnapshotInfoDetail::Opcode(detail) => Some(detail.clone()),
                _ => None,
            });

        // Check for SSTORE (0x55) operation
        if let Some(prev_detail) = prev_snapshot_detail {
            let is_sstore = prev_detail.opcode == 0x55;

            if is_sstore && !prev_detail.stack.is_empty() {
                let target_slot = prev_detail.stack[prev_detail.stack.len() - 1]; // Top of stack is the slot
                let value_to_write = if prev_detail.stack.len() > 1 {
                    Some(prev_detail.stack[prev_detail.stack.len() - 2]) // Second item is the value
                } else {
                    None
                };

                // Query the storage value before the operation
                let prev_storage = dm.execution.get_storage(prev_snapshot_id, target_slot).cloned();

                // Simple header with operation name
                display_items.push("▶ SSTORE Operation".to_string());
                item_styles.push(StorageItemStyle::StoreHeader);
                display_items.push(String::new());
                item_styles.push(StorageItemStyle::Normal);

                // Target slot
                display_items.push(format!("  Slot: {target_slot:#066x}"));
                item_styles.push(StorageItemStyle::StoreInfo);

                // Previous value
                if let Some(prev_val) = prev_storage {
                    display_items
                        .push(format!("  Old:  {}", utils::format_value_with_decode(&prev_val)));
                    item_styles.push(StorageItemStyle::Normal);
                }

                // Value being written
                if let Some(val) = value_to_write {
                    display_items
                        .push(format!("  New:  {}", utils::format_value_with_decode(&val)));
                    item_styles.push(StorageItemStyle::StoreWrite);
                }

                // Add separator
                display_items.push(String::new());
                item_styles.push(StorageItemStyle::Normal);
            }
        }

        if self.storage_changes.is_empty() && display_items.is_empty() {
            let paragraph = Paragraph::new("No storage changes").block(BorderPresets::display(
                self.focused,
                self.title(dm),
                dm.theme.focused_border,
                dm.theme.unfocused_border,
            ));
            frame.render_widget(paragraph, area);
            return;
        }

        // Regular storage changes with clean formatting
        if !self.storage_changes.is_empty() {
            if !display_items.is_empty() {
                display_items.push("─── All Storage Changes ───".to_string());
                item_styles.push(StorageItemStyle::Header);
                display_items.push(String::new());
                item_styles.push(StorageItemStyle::Normal);
            }

            let mut sorted_changes: Vec<_> = self.storage_changes.iter().collect();
            sorted_changes.sort_by_key(|(slot, _)| **slot);

            for (slot, (old_value, new_value)) in sorted_changes {
                // Slot line
                display_items.push(format!("• Slot: {slot:#066x}"));
                item_styles.push(StorageItemStyle::SlotLine);

                // Old value
                display_items
                    .push(format!("  Old:  {}", utils::format_value_with_decode(old_value)));
                item_styles.push(StorageItemStyle::Normal);

                // New value with arrow
                display_items
                    .push(format!("  New:  {}", utils::format_value_with_decode(new_value)));
                item_styles.push(StorageItemStyle::ChangeLine);

                // Add small separator between items
                display_items.push(String::new());
                item_styles.push(StorageItemStyle::Normal);
            }
        }

        // Create list items with proper scrolling and selection
        let items: Vec<ListItem<'_>> = display_items
            .iter()
            .zip(item_styles.iter())
            .enumerate()
            .skip(self.scroll_offset)
            .take(self.context_height)
            .map(|(display_idx, (line, item_style))| {
                let is_selected = display_idx == self.selected_index;

                let style = match item_style {
                    StorageItemStyle::StoreHeader => {
                        Style::default().fg(dm.theme.warning_color).add_modifier(Modifier::BOLD)
                    }
                    StorageItemStyle::StoreInfo => Style::default().fg(dm.theme.info_color),
                    StorageItemStyle::StoreWrite => {
                        Style::default().fg(dm.theme.success_color).add_modifier(Modifier::BOLD)
                    }
                    StorageItemStyle::Header => {
                        Style::default().fg(dm.theme.accent_color).add_modifier(Modifier::BOLD)
                    }
                    StorageItemStyle::SlotLine => {
                        if is_selected && self.focused {
                            Style::default()
                                .bg(dm.theme.selection_bg)
                                .fg(dm.theme.selection_fg)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(dm.theme.accent_color).add_modifier(Modifier::BOLD)
                        }
                    }
                    StorageItemStyle::ChangeLine => {
                        if is_selected && self.focused {
                            Style::default().bg(dm.theme.selection_bg).fg(dm.theme.selection_fg)
                        } else {
                            Style::default().fg(dm.theme.success_color)
                        }
                    }
                    StorageItemStyle::Normal => {
                        if is_selected && self.focused {
                            Style::default().bg(dm.theme.selection_bg).fg(dm.theme.selection_fg)
                        } else {
                            Style::default().fg(dm.theme.comment_color)
                        }
                    }
                };

                let formatted_line = self.apply_horizontal_offset(Line::from(line.clone()));
                ListItem::new(formatted_line).style(style)
            })
            .collect();

        let list = List::new(items).block(BorderPresets::display(
            self.focused,
            self.title(dm),
            dm.theme.focused_border,
            dm.theme.unfocused_border,
        ));

        frame.render_widget(list, area);
        self.render_status_and_help(frame, area, dm);
    }

    /// Render transient storage display
    fn render_transient_storage(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        dm: &mut DataManager,
    ) {
        // Update cached display line count
        self.tstorage_display_lines = self.calculate_tstorage_display_lines(dm);

        let mut display_items = Vec::new();
        let mut item_styles = Vec::new();

        // Get current snapshot ID
        let current_snapshot = if let Some(id) = self.current_execution_snapshot {
            id
        } else {
            // No snapshot available
            let paragraph =
                Paragraph::new("No snapshot data available").block(BorderPresets::display(
                    self.focused,
                    self.title(dm),
                    dm.theme.focused_border,
                    dm.theme.unfocused_border,
                ));
            frame.render_widget(paragraph, area);
            return;
        };

        // Get previous snapshot info to check for TSTORE
        let prev_snapshot_id = if let Some(info) = dm.execution.get_snapshot_info(current_snapshot)
        {
            info.prev_id()
        } else {
            0
        };

        let prev_snapshot_detail =
            dm.execution.get_snapshot_info(prev_snapshot_id).and_then(|info| match info.detail() {
                SnapshotInfoDetail::Opcode(detail) => Some(detail.clone()),
                _ => None,
            });

        // Check for TSTORE (0x5D) operation
        if let Some(prev_detail) = prev_snapshot_detail {
            let is_tstore = prev_detail.opcode == 0x5D;

            if is_tstore && !prev_detail.stack.is_empty() {
                let target_slot = prev_detail.stack[prev_detail.stack.len() - 1]; // Top of stack is the slot
                let value_to_write = if prev_detail.stack.len() > 1 {
                    Some(prev_detail.stack[prev_detail.stack.len() - 2]) // Second item is the value
                } else {
                    None
                };

                // Add special TSTORE indicator
                display_items.push("▶ TSTORE Operation".to_string());
                item_styles.push(StorageItemStyle::StoreHeader);
                display_items.push(String::new());
                item_styles.push(StorageItemStyle::Normal);

                // Target slot
                display_items.push(format!("  Slot: {target_slot:#066x}"));
                item_styles.push(StorageItemStyle::StoreInfo);

                // Value being written
                if let Some(val) = value_to_write {
                    display_items
                        .push(format!("  New:  {}", utils::format_value_with_decode(&val)));
                    item_styles.push(StorageItemStyle::StoreWrite);
                }

                // Add separator
                display_items.push(String::new());
                item_styles.push(StorageItemStyle::Normal);
            }
        }

        if self.transient_storage.is_empty() && display_items.is_empty() {
            let paragraph =
                Paragraph::new("No transient storage data").block(BorderPresets::display(
                    self.focused,
                    self.title(dm),
                    dm.theme.focused_border,
                    dm.theme.unfocused_border,
                ));
            frame.render_widget(paragraph, area);
            return;
        }

        // Regular transient storage items with clean formatting
        if !self.transient_storage.is_empty() {
            if !display_items.is_empty() {
                display_items.push("─── Transient Storage ───".to_string());
                item_styles.push(StorageItemStyle::Header);
                display_items.push(String::new());
                item_styles.push(StorageItemStyle::Normal);
            }

            // Convert to sorted list for consistent display
            let mut tstorage_items: Vec<_> = self.transient_storage.iter().collect();
            tstorage_items.sort_by_key(|(slot, _)| *slot);

            for (slot, value) in tstorage_items {
                // Slot and value with clean formatting
                display_items.push(format!("• Slot: {slot:#066x}"));
                item_styles.push(StorageItemStyle::SlotLine);

                display_items.push(format!("  Val:  {}", utils::format_value_with_decode(value)));
                item_styles.push(StorageItemStyle::ChangeLine);

                // Add separator
                display_items.push(String::new());
                item_styles.push(StorageItemStyle::Normal);
            }
        }

        let items: Vec<ListItem<'_>> = display_items
            .iter()
            .zip(item_styles.iter())
            .enumerate()
            .skip(self.scroll_offset)
            .take(self.context_height)
            .map(|(display_idx, (line, item_style))| {
                let is_selected = display_idx == self.selected_index;

                let style = match item_style {
                    StorageItemStyle::StoreHeader => {
                        Style::default().fg(dm.theme.warning_color).add_modifier(Modifier::BOLD)
                    }
                    StorageItemStyle::StoreInfo => Style::default().fg(dm.theme.info_color),
                    StorageItemStyle::StoreWrite => {
                        Style::default().fg(dm.theme.success_color).add_modifier(Modifier::BOLD)
                    }
                    StorageItemStyle::Header => {
                        Style::default().fg(dm.theme.accent_color).add_modifier(Modifier::BOLD)
                    }
                    StorageItemStyle::SlotLine => {
                        if is_selected && self.focused {
                            Style::default()
                                .bg(dm.theme.selection_bg)
                                .fg(dm.theme.selection_fg)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(dm.theme.accent_color).add_modifier(Modifier::BOLD)
                        }
                    }
                    StorageItemStyle::ChangeLine => {
                        if is_selected && self.focused {
                            Style::default().bg(dm.theme.selection_bg).fg(dm.theme.selection_fg)
                        } else {
                            Style::default().fg(dm.theme.success_color)
                        }
                    }
                    StorageItemStyle::Normal => {
                        if is_selected && self.focused {
                            Style::default().bg(dm.theme.selection_bg).fg(dm.theme.selection_fg)
                        } else {
                            Style::default().fg(dm.theme.comment_color)
                        }
                    }
                };

                let formatted_line = self.apply_horizontal_offset(Line::from(line.clone()));
                ListItem::new(formatted_line).style(style)
            })
            .collect();

        let list = List::new(items).block(BorderPresets::display(
            self.focused,
            self.title(dm),
            dm.theme.focused_border,
            dm.theme.unfocused_border,
        ));

        frame.render_widget(list, area);
        self.render_status_and_help(frame, area, dm);
    }

    /// Render expressions display (for both opcode and hooked snapshots)
    fn render_expressions(&mut self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        if self.expressions.is_empty() {
            let empty_msg = "No expressions watched";
            let paragraph = Paragraph::new(empty_msg).block(BorderPresets::display(
                self.focused,
                self.title(dm),
                dm.theme.focused_border,
                dm.theme.unfocused_border,
            ));
            frame.render_widget(paragraph, area);
            return;
        }

        let items: Vec<ListItem<'_>> = self
            .expressions
            .iter()
            .enumerate()
            .skip(self.scroll_offset)
            .take(self.context_height)
            .map(|(display_idx, entry)| {
                let is_selected = display_idx == self.selected_index;

                // Format the expression entry with syntax highlighting
                let formatted_spans = self.format_variable_entry(entry, dm);

                // Create a line from the spans
                let line = Line::from(formatted_spans);
                let formatted_line = self.apply_horizontal_offset(line);

                // Apply selection style if selected
                let style = if is_selected && self.focused {
                    Style::default().bg(dm.theme.selection_bg).fg(dm.theme.selection_fg)
                } else {
                    // Default style - the highlighting is already applied to individual spans
                    Style::default()
                };

                ListItem::new(formatted_line).style(style)
            })
            .collect();

        let list = List::new(items).block(BorderPresets::display(
            self.focused,
            self.title(dm),
            dm.theme.focused_border,
            dm.theme.unfocused_border,
        ));

        frame.render_widget(list, area);
        self.render_status_and_help(frame, area, dm);
    }

    /// Render variables display (for hooked snapshots)
    fn render_variables(&mut self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        let current_variables = &self.variables;

        if current_variables.is_empty() {
            let empty_msg = "No variables available";
            let paragraph = Paragraph::new(empty_msg).block(BorderPresets::display(
                self.focused,
                self.title(dm),
                dm.theme.focused_border,
                dm.theme.unfocused_border,
            ));
            frame.render_widget(paragraph, area);
            return;
        }

        let items: Vec<ListItem<'_>> = current_variables
            .iter()
            .enumerate()
            .skip(self.scroll_offset)
            .take(self.context_height)
            .map(|(display_idx, entry)| {
                let is_selected = display_idx == self.selected_index;

                // Format the variable entry with syntax highlighting
                let formatted_spans = self.format_variable_entry(entry, dm);

                // Create a line from the spans
                let line = Line::from(formatted_spans);
                let formatted_line = self.apply_horizontal_offset(line);

                // Apply selection style if selected
                let style = if is_selected && self.focused {
                    Style::default().bg(dm.theme.selection_bg).fg(dm.theme.selection_fg)
                } else {
                    // Default style - the highlighting is already applied to individual spans
                    Style::default()
                };

                ListItem::new(formatted_line).style(style)
            })
            .collect();

        let list = List::new(items).block(BorderPresets::display(
            self.focused,
            self.title(dm),
            dm.theme.focused_border,
            dm.theme.unfocused_border,
        ));

        frame.render_widget(list, area);
        self.render_status_and_help(frame, area, dm);
    }

    /// Render breakpoints display (for both opcode and hooked snapshots)
    fn render_breakpoints(&mut self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        if self.breakpoints.is_empty() {
            let empty_msg = "No breakpoints set";
            let paragraph = Paragraph::new(empty_msg).block(BorderPresets::display(
                self.focused,
                self.title(dm),
                dm.theme.focused_border,
                dm.theme.unfocused_border,
            ));
            frame.render_widget(paragraph, area);
            return;
        }

        let items: Vec<ListItem<'_>> = self
            .breakpoints
            .iter()
            .enumerate()
            .skip(self.scroll_offset)
            .take(self.context_height)
            .map(|(display_idx, entry)| {
                let is_selected = display_idx == self.selected_index;

                // Format the breakpoint entry with visual indicators
                let formatted_spans = self.format_breakpoint_entry(entry, dm);

                // Create a line from the spans
                let line = Line::from(formatted_spans);
                let formatted_line = self.apply_horizontal_offset(line);

                // Apply background styling for selection only
                // Don't apply foreground colors here to preserve span-specific colors
                let style = if is_selected && self.focused {
                    Style::default().bg(dm.theme.selection_bg)
                } else {
                    Style::default()
                };

                ListItem::new(formatted_line).style(style)
            })
            .collect();

        let list = List::new(items).block(BorderPresets::display(
            self.focused,
            self.title(dm),
            dm.theme.focused_border,
            dm.theme.unfocused_border,
        ));

        frame.render_widget(list, area);
        self.render_status_and_help(frame, area, dm);
    }

    /// Render status and help text
    fn render_status_and_help(&self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        if !self.focused || area.height <= 10 {
            return;
        }

        // Status line
        let status_area =
            Rect { x: area.x + 1, y: area.y + area.height - 3, width: area.width - 2, height: 1 };

        let item_count = self.displayed_line_count;

        let status_bar = StatusBar::new()
            .current_panel("Display".to_string())
            .message(format!("Mode: {}", self.mode.name()))
            .message(format!("Items: {item_count}"));

        let status_text = status_bar.build();

        // Add horizontal scroll indicator if content is scrollable
        let final_status_text = if self.max_line_width > self.content_width {
            let scrollable_width = self.max_line_width.saturating_sub(self.content_width);
            let scroll_percentage = if scrollable_width > 0 {
                (self.horizontal_offset as f32 / scrollable_width as f32).min(1.0)
            } else {
                0.0
            };

            let indicator_width = 15;
            let thumb_position = (scroll_percentage * (indicator_width - 3) as f32) as usize;

            let mut indicator = String::from(" [");
            for i in 0..indicator_width {
                if i >= thumb_position && i < thumb_position + 3 {
                    indicator.push('█'); // Full block character
                } else {
                    indicator.push('─'); // Horizontal line character
                }
            }
            indicator.push(']');

            format!("{status_text}{indicator}")
        } else {
            status_text
        };

        let status_paragraph =
            Paragraph::new(final_status_text).style(Style::default().fg(dm.theme.accent_color));
        frame.render_widget(status_paragraph, status_area);

        // Help line
        let help_area =
            Rect { x: area.x + 1, y: area.y + area.height - 2, width: area.width - 2, height: 1 };

        let help_text = match self.mode {
            DisplayMode::Variables | DisplayMode::Expressions => {
                "Vim-like Navigation • s/S: Switch mode • Enter: Toggle multi-line • ?: Help"
                    .to_string()
            }
            DisplayMode::Breakpoints => {
                "Vim-like Navigation • s/S: Switch mode • Enter: Toggle enable/disable • ?: Help"
                    .to_string()
            }
            _ => "Vim-like Navigation • s/S: Switch mode • ?: Help".to_string(),
        };

        let help_paragraph =
            Paragraph::new(help_text).style(Style::default().fg(dm.theme.help_text_color));
        frame.render_widget(help_paragraph, help_area);
    }
}

impl PanelTr for DisplayPanel {
    fn panel_type(&self) -> PanelType {
        PanelType::Display
    }

    fn title(&self, _dm: &mut DataManager) -> String {
        let item_count = match self.mode {
            DisplayMode::Stack => self.stack_items.len(),
            DisplayMode::Memory => self.memory_chunks.len(),
            DisplayMode::CallData => {
                if self.calldata.is_empty() {
                    0
                } else {
                    self.calldata.len().div_ceil(32)
                }
            }
            DisplayMode::Storage => self.storage_display_lines,
            DisplayMode::TransientStorage => self.tstorage_display_lines,
            DisplayMode::Variables => self.variables.len(),
            DisplayMode::Expressions => self.expressions.len(),
            DisplayMode::Breakpoints => self.breakpoints.len(),
        };

        let snapshot_type = if self.is_opcode_snapshot { "Opcode" } else { "Hook" };
        format!("{} - {} ({} items)", snapshot_type, self.mode.name(), item_count)
    }

    fn render(&mut self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        // Calculate context height and width for viewport calculations
        self.context_height = if self.focused && area.height > 10 {
            area.height.saturating_sub(4) // Account for borders and status lines
        } else {
            area.height.saturating_sub(2) // Just borders
        } as usize;
        self.content_width = area.width.saturating_sub(2) as usize; // Account for borders

        // Update snapshot data if needed
        let _ = self.update_snapshot_data(dm);

        // Calculate max line width for horizontal scrolling
        self.calculate_max_line_width(dm);

        // Cache the display line counts
        self.displayed_line_count = self.calculate_context_lines(dm);

        // Render based on current mode
        match self.mode {
            DisplayMode::Stack => self.render_stack(frame, area, dm),
            DisplayMode::Memory => self.render_memory(frame, area, dm),
            DisplayMode::CallData => self.render_calldata(frame, area, dm),
            DisplayMode::Storage => self.render_storage(frame, area, dm),
            DisplayMode::TransientStorage => self.render_transient_storage(frame, area, dm),
            DisplayMode::Variables => self.render_variables(frame, area, dm),
            DisplayMode::Expressions => self.render_expressions(frame, area, dm),
            DisplayMode::Breakpoints => self.render_breakpoints(frame, area, dm),
        }
    }

    fn handle_key_event(&mut self, event: KeyEvent, dm: &mut DataManager) -> Result<EventResponse> {
        if !self.focused || event.kind != KeyEventKind::Press {
            return Ok(EventResponse::NotHandled);
        }

        match event.code {
            KeyCode::Char('s') => {
                self.next_mode();
                Ok(EventResponse::Handled)
            }
            KeyCode::Char('S') => {
                self.prev_mode();
                Ok(EventResponse::Handled)
            }
            KeyCode::Up => {
                self.move_up(1);
                Ok(EventResponse::Handled)
            }
            KeyCode::Down => {
                self.move_down(1);
                Ok(EventResponse::Handled)
            }
            KeyCode::Left => {
                // Scroll left
                if self.horizontal_offset > 0 {
                    self.horizontal_offset = self.horizontal_offset.saturating_sub(5);
                    debug!("Scrolled left to offset {}", self.horizontal_offset);
                }
                Ok(EventResponse::Handled)
            }
            KeyCode::Right => {
                // Scroll right
                if self.max_line_width > self.content_width {
                    let max_scroll = self.max_line_width.saturating_sub(self.content_width);
                    if self.horizontal_offset < max_scroll {
                        self.horizontal_offset = (self.horizontal_offset + 5).min(max_scroll);
                        debug!("Scrolled right to offset {}", self.horizontal_offset);
                    }
                }
                Ok(EventResponse::Handled)
            }
            KeyCode::PageUp => {
                self.move_up(5);
                Ok(EventResponse::Handled)
            }
            KeyCode::PageDown => {
                self.move_down(5);
                Ok(EventResponse::Handled)
            }
            KeyCode::Home => {
                self.selected_index = 0;
                Ok(EventResponse::Handled)
            }
            KeyCode::End => {
                let max_items = self.displayed_line_count;
                self.selected_index = max_items.saturating_sub(1);
                Ok(EventResponse::Handled)
            }
            KeyCode::Enter => {
                // Toggle multi-line display for variables and expressions, or toggle breakpoint
                // enable/disable
                match self.mode {
                    DisplayMode::Variables | DisplayMode::Expressions => {
                        self.toggle_multiline(dm);
                        Ok(EventResponse::Handled)
                    }
                    DisplayMode::Breakpoints => {
                        self.toggle_breakpoint(dm);
                        Ok(EventResponse::Handled)
                    }
                    _ => Ok(EventResponse::NotHandled),
                }
            }
            _ => Ok(EventResponse::NotHandled),
        }
    }

    fn handle_mouse_event(
        &mut self,
        event: crossterm::event::MouseEvent,
        _dm: &mut DataManager,
    ) -> Result<EventResponse> {
        use crossterm::event::MouseEventKind;

        match event.kind {
            MouseEventKind::ScrollUp => {
                self.move_up(1);
                Ok(EventResponse::Handled)
            }
            MouseEventKind::ScrollDown => {
                self.move_down(1);
                Ok(EventResponse::Handled)
            }
            _ => Ok(EventResponse::NotHandled),
        }
    }

    fn on_focus(&mut self) {
        self.focused = true;
        debug!("Display panel gained focus");
    }

    fn on_blur(&mut self) {
        self.focused = false;
        debug!("Display panel lost focus");
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// Helper functions
/// Format bytes as hex with ASCII decode (like xxd)
fn format_bytes_with_decode<'a>(
    bytes: &'a [u8],
    highlight_indices: &'a [usize],
    theme: &ColorScheme,
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();

    // Hex part
    for (i, byte) in bytes.iter().enumerate() {
        let hex = format!("{byte:02x}");
        if highlight_indices.contains(&i) {
            spans.push(Span::styled(
                hex,
                Style::default().fg(theme.warning_color).add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::raw(hex));
        }
        if i < bytes.len() - 1 {
            spans.push(Span::raw(" "));
        }
    }

    // ASCII part
    spans.push(Span::raw("  "));
    for (i, byte) in bytes.iter().enumerate() {
        let ch = if byte.is_ascii_graphic() || *byte == b' ' {
            *byte as char
        } else {
            '.' // Show all non-printable bytes (including 0) as dots
        };

        if highlight_indices.contains(&i) {
            spans.push(Span::styled(ch.to_string(), Style::default().fg(theme.warning_color)));
        } else {
            spans.push(Span::raw(ch.to_string()));
        }
    }

    spans
}

/// Format bytes as hex only (no ASCII decode) - for calldata
fn format_bytes_hex_only<'a>(
    bytes: &'a [u8],
    highlight_indices: &'a [usize],
    theme: &ColorScheme,
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();

    // Hex part only
    for (i, byte) in bytes.iter().enumerate() {
        let hex = format!("{byte:02x}");
        if highlight_indices.contains(&i) {
            spans.push(Span::styled(
                hex,
                Style::default().fg(theme.warning_color).add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::raw(hex));
        }
        if i < bytes.len() - 1 {
            spans.push(Span::raw(" "));
        }
    }

    spans
}
