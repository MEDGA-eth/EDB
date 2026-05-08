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

//! Trace panel for displaying execution trace
//!
//! This panel shows the call trace and allows navigation through trace entries.

use super::{EventResponse, PanelTr, PanelType};
use crate::{
    data::DataManager,
    ui::{
        borders::BorderPresets,
        status::StatusBar,
        syntax::{SyntaxHighlighter, SyntaxType},
    },
};
use alloy_dyn_abi::DynSolValue;
use alloy_primitives::{Bytes, hex};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use edb_common::types::{CallResult, CallType, Trace, TraceEntry};
use eyre::{Result, bail};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
};
use revm::{
    context::CreateScheme,
    interpreter::{CallScheme, InstructionResult},
};
use std::{
    collections::HashSet,
    ops::{Deref, DerefMut},
};
use tracing::debug;

/// Represents different types of trace lines for multi-line display
#[derive(Debug, Clone)]
enum TraceLineType {
    /// Main call/create line  
    Call(usize), // trace entry id
    /// Event line
    Event(usize, usize), // trace entry id, event index
    /// Return value line
    Return(usize), // trace entry id
}

#[derive(Debug)]
pub struct TracePanel {
    inner: TracePanelInner,
    trace: Option<Trace>,
}

impl Deref for TracePanel {
    type Target = TracePanelInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for TracePanel {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl TracePanel {
    pub fn new() -> Self {
        Self { inner: TracePanelInner::new(), trace: None }
    }
}

/// Inner implementation details for the trace panel
/// This is a hacky trick to avoid heavy data movement over trace
#[derive(Debug)]
pub struct TracePanelInner {
    // ========== Display ==========
    /// Currently selected trace entry index
    selected_index: usize,
    /// Scroll offset
    scroll_offset: usize,
    /// Current content height
    context_height: usize,
    /// Current content width
    content_width: usize,
    /// Horizontal scroll offset
    horizontal_offset: usize,
    /// Maximum line width for horizontal scrolling
    max_line_width: usize,
    /// Whether this panel is focused
    focused: bool,
    /// Syntax highlighter for Solidity values
    syntax_highlighter: SyntaxHighlighter,
    /// Set of collapsed trace entry IDs (when collapsed, we hide children)
    collapsed_entries: HashSet<usize>,
    /// VIM mode number prefix for repetition (e.g., "10" in "10j")
    vim_number_prefix: String,
    /// VIM command buffer for commands like ":n"
    vim_command_buffer: String,
    /// Whether we're in VIM command mode (after pressing :)
    vim_command_mode: bool,
    /// Number of lines that will be displayed (for scrolling calculations)
    displayed_line_count: usize,

    // ========== Execution Tracking ==========
    /// Currently executing trace entry ID (from execution snapshot)
    current_execution_entry: Option<usize>,
    /// Current execution snapshot ID for tracking changes
    current_execution_snapshot: Option<usize>,
}

impl TracePanelInner {
    /// Create a new trace panel
    pub fn new() -> Self {
        Self {
            selected_index: 0,
            focused: false,
            scroll_offset: 0,
            context_height: 0,
            content_width: 0,
            horizontal_offset: 0,
            max_line_width: 0,
            displayed_line_count: 0,
            syntax_highlighter: SyntaxHighlighter::new(),
            collapsed_entries: HashSet::new(),
            current_execution_entry: None,
            current_execution_snapshot: None,
            vim_number_prefix: String::new(),
            vim_command_buffer: String::new(),
            vim_command_mode: false,
        }
    }

    /// Calculate the maximum line width for horizontal scrolling
    fn calculate_max_line_width(&mut self, trace: &Trace, dm: &mut DataManager) {
        let display_lines = self.generate_display_lines(trace);

        self.max_line_width = display_lines
            .iter()
            .map(|line_type| {
                let formatted_line = self.format_display_line(line_type, trace, dm);
                // Calculate the visual width of the line including cursor indicators
                // Note: format_display_line already adds the cursor indicators, so we just sum all
                // spans
                formatted_line.spans.iter().map(|span| span.content.chars().count()).sum()
            })
            .max()
            .unwrap_or(0);
    }

    /// Apply horizontal offset to a line for horizontal scrolling
    fn apply_horizontal_offset<'a>(
        &self,
        line: ratatui::text::Line<'a>,
    ) -> ratatui::text::Line<'a> {
        use ratatui::text::{Line, Span};

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
            new_spans.push(Span::raw(""));
        }

        Line::from(new_spans)
    }

    /// Get repetition count from vim_number_prefix, defaulting to 1
    fn get_vim_repetition(&self) -> usize {
        if self.vim_number_prefix.is_empty() {
            1
        } else {
            self.vim_number_prefix.parse().unwrap_or(1).max(1)
        }
    }

    /// Clear vim state after executing a command
    fn clear_vim_state(&mut self) {
        self.vim_number_prefix.clear();
    }

    /// Move selection up with repetition support
    fn move_up(&mut self, count: usize) {
        self.selected_index = self.selected_index.saturating_sub(count);
        // Auto-scroll up if needed
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        }
    }

    /// Move selection down with repetition support
    fn move_down(&mut self, count: usize) {
        let max_lines = self.displayed_line_count;
        self.selected_index =
            self.selected_index.saturating_add(count).min(max_lines.saturating_sub(1));
        // Auto-scroll down if needed
        let viewport_height = self.context_height;
        if self.selected_index >= self.scroll_offset + viewport_height {
            self.scroll_offset = (self.selected_index + 1).saturating_sub(viewport_height);
        }
    }

    /// Move to specific line number (1-based)
    fn move_to(&mut self, line_num: usize) {
        let max_lines = self.displayed_line_count;
        self.selected_index = line_num.saturating_sub(1);
        // Center the view on this line if possible
        let viewport_height = self.context_height;
        let half_viewport = viewport_height / 2;

        if self.selected_index <= half_viewport || max_lines <= viewport_height {
            self.scroll_offset = 0;
        } else if self.selected_index > max_lines.saturating_sub(viewport_height) {
            self.scroll_offset = max_lines.saturating_sub(viewport_height);
        } else {
            self.scroll_offset = self.selected_index.saturating_sub(half_viewport);
        }
    }

    /// Execute VIM command from command buffer
    fn execute_vim_command(&mut self) {
        let command = self.vim_command_buffer.trim();
        if let Ok(line_number) = command.parse::<usize>() {
            self.move_to(line_number);
        }
        // Clear command mode
        self.vim_command_buffer.clear();
        self.vim_command_mode = false;
    }

    /// Get currently selected trace entry
    pub fn selected_entry<'a>(&mut self, trace: &'a Trace) -> Option<&'a TraceEntry> {
        let display_lines = self.generate_display_lines(trace);
        if let Some(line_type) = display_lines.get(self.selected_index) {
            let entry_id = match line_type {
                TraceLineType::Call(id) => *id,
                TraceLineType::Event(id, _) => *id,
                TraceLineType::Return(id) => *id,
            };
            trace.iter().find(|e| e.id == entry_id)
        } else {
            None
        }
    }

    /// Toggle expansion/collapse for current selected entry
    pub fn toggle_expansion(&mut self, trace: &Trace) {
        let display_lines = self.generate_display_lines(trace);
        if let Some(line_type) = display_lines.get(self.selected_index) {
            let entry_id = match line_type {
                TraceLineType::Call(id) => *id,
                TraceLineType::Event(id, _) => *id,
                TraceLineType::Return(id) => *id,
            };

            // Find the entry and only handle children collapse (details always shown)
            if trace.iter().any(|e| e.id == entry_id) {
                // Check if this entry has children that can be collapsed
                let has_children = trace.iter().any(|e| e.parent_id == Some(entry_id));

                if has_children {
                    // Handle parent/child collapse only
                    if self.collapsed_entries.contains(&entry_id) {
                        // Expand children: remove from collapsed set
                        self.collapsed_entries.remove(&entry_id);
                    } else {
                        // Collapse children: add to collapsed set
                        self.collapsed_entries.insert(entry_id);
                    }
                    // Adjust selection after parent collapse
                    self.adjust_selection_after_expansion(entry_id, trace);
                }
            }
        }
    }

    /// Adjust selection to stay on the main call line after expansion/collapse
    fn adjust_selection_after_expansion(&mut self, entry_id: usize, trace: &Trace) {
        let display_lines = self.generate_display_lines(trace);
        // Find the call line for this entry
        if let Some(call_line_index) = display_lines
            .iter()
            .position(|line| matches!(line, TraceLineType::Call(id) if *id == entry_id))
        {
            self.selected_index = call_line_index;

            // Ensure selection is within viewport
            let viewport_height = self.context_height;
            if self.selected_index < self.scroll_offset {
                self.scroll_offset = self.selected_index;
            } else if self.selected_index >= self.scroll_offset + viewport_height {
                self.scroll_offset = (self.selected_index + 1).saturating_sub(viewport_height);
            }
        }
    }

    /// Check if an entry should be visible (not hidden by collapsed parent)
    fn is_entry_visible(&self, entry: &TraceEntry, trace: &Trace) -> bool {
        if let Some(parent_id) = entry.parent_id {
            // Check if any ancestor is collapsed
            let mut current_parent_id = Some(parent_id);
            while let Some(pid) = current_parent_id {
                if self.collapsed_entries.contains(&pid) {
                    return false;
                }
                // Find the parent entry to check its parent
                if let Some(parent_entry) = trace.iter().find(|e| e.id == pid) {
                    current_parent_id = parent_entry.parent_id;
                } else {
                    break;
                }
            }
        }
        true
    }

    /// Generate display lines for the trace, always showing details
    fn generate_display_lines(&self, trace: &Trace) -> Vec<TraceLineType> {
        let mut lines = Vec::new();

        for entry in trace.iter() {
            if !self.is_entry_visible(entry, trace) {
                continue;
            }

            // Always show the main call line
            lines.push(TraceLineType::Call(entry.id));

            // Always show events and return values (no expansion toggle for details)
            // Add event lines
            for (i, _) in entry.events.iter().enumerate() {
                lines.push(TraceLineType::Event(entry.id, i));
            }

            // Add return value line if there's meaningful return data
            if entry.result.is_some() && self.should_show_return_line(entry) {
                lines.push(TraceLineType::Return(entry.id));
            }
        }

        lines
    }

    /// Check if we should show a separate return line for this entry
    fn should_show_return_line(&self, entry: &TraceEntry) -> bool {
        match &entry.result {
            Some(CallResult::Success { output, .. }) => !output.is_empty(),
            Some(CallResult::Revert { .. }) => true,
            Some(CallResult::Error { .. }) => true,
            None => false,
        }
    }

    /// Format a display line based on its type
    fn format_display_line(
        &mut self,
        line_type: &TraceLineType,
        trace: &Trace,
        dm: &mut DataManager,
    ) -> Line<'static> {
        // Get the base formatted line
        match line_type {
            TraceLineType::Call(entry_id) => {
                if let Some(entry) = trace.iter().find(|e| e.id == *entry_id) {
                    self.format_trace_entry_compact(entry, entry.depth, dm)
                } else {
                    Line::from("Unknown entry")
                }
            }
            TraceLineType::Event(entry_id, event_idx) => {
                if let Some(entry) = trace.iter().find(|e| e.id == *entry_id) {
                    self.format_event_line(entry, *event_idx, dm)
                } else {
                    Line::from("Unknown event")
                }
            }
            TraceLineType::Return(entry_id) => {
                if let Some(entry) = trace.iter().find(|e| e.id == *entry_id) {
                    self.format_return_line(entry, dm)
                } else {
                    Line::from("Unknown return")
                }
            }
        }
    }

    /// Check if this entry is the last child of its parent
    fn is_last_child(&self, entry: &TraceEntry, trace: &Trace) -> bool {
        if let Some(parent_id) = entry.parent_id {
            // Find all visible siblings (same parent)
            let visible_siblings: Vec<_> = trace
                .iter()
                .filter(|e| e.parent_id == Some(parent_id) && self.is_entry_visible(e, trace))
                .collect();

            // Check if this is the last visible sibling
            if let Some(last) = visible_siblings.last() {
                return last.id == entry.id;
            }
        }
        false
    }

    /// Check if this entry has children
    fn has_children(&self, entry: &TraceEntry, trace: &Trace) -> bool {
        trace.iter().any(|e| e.parent_id == Some(entry.id) && self.is_entry_visible(e, trace))
    }

    /// Check if an ancestor at a given depth is the last child
    fn is_ancestor_last_child(
        &self,
        entry: &TraceEntry,
        ancestor_depth: usize,
        trace: &Trace,
    ) -> bool {
        if ancestor_depth >= entry.depth {
            return false;
        }

        // Walk up the parent chain to find the ancestor at the target depth
        let mut current = Some(entry);
        let mut current_depth = entry.depth;

        while let Some(e) = current {
            if current_depth == ancestor_depth {
                return self.is_last_child(e, trace);
            }

            if let Some(parent_id) = e.parent_id {
                current = trace.iter().find(|p| p.id == parent_id);
                current_depth = current_depth.saturating_sub(1);
            } else {
                break;
            }
        }

        false
    }

    /// Build clean tree indentation for the new design
    fn build_tree_indent_clean(&self, entry: &TraceEntry, trace: &Trace) -> String {
        if entry.depth == 0 {
            return String::new();
        }

        let mut indent_parts = Vec::new();

        // For each ancestor level, determine if we need a vertical line
        for ancestor_depth in 0..entry.depth.saturating_sub(1) {
            if self.is_ancestor_last_child(entry, ancestor_depth + 1, trace) {
                indent_parts.push("  "); // Spaces for completed branches
            } else {
                indent_parts.push("│ "); // Vertical line for continuing branches
            }
        }

        indent_parts.join("")
    }

    /// Format a compact trace entry (main call line without events/returns)
    fn format_trace_entry_compact(
        &mut self,
        entry: &TraceEntry,
        depth: usize,
        dm: &mut DataManager,
    ) -> Line<'static> {
        let trace = dm.execution.get_trace();

        // Check if this entry has children
        let has_children = trace.iter().any(|e| e.parent_id == Some(entry.id));

        // Build the line prefix as a single string
        let line_prefix = if depth == 0 {
            // Root level: collapse indicator at start, then tree structure if has siblings
            if has_children {
                self.get_enhanced_collapse_indicator(entry.id, trace)
            } else {
                "  ".to_string() // spaces for alignment when no children
            }
        } else {
            // Child level: tree structure with proper spacing
            let tree_indent = self.build_tree_indent_clean(entry, trace);
            let connector = if self.is_last_child(entry, trace) { "└─" } else { "├─" };

            // Add collapse indicator if has children, otherwise just a space
            if has_children {
                let collapse_char = self.get_enhanced_collapse_indicator(entry.id, trace);
                format!("  {tree_indent}{connector}{collapse_char}")
            } else {
                format!("  {tree_indent}{connector}  ")
            }
        };

        let (call_type_str, call_color) = match &entry.call_type {
            CallType::Call(CallScheme::Call) => ("CALL", dm.theme.call_color),
            CallType::Call(CallScheme::CallCode) => ("CALLCODE", dm.theme.call_color),
            CallType::Call(CallScheme::DelegateCall) => ("DELEGATECALL", dm.theme.call_color),
            CallType::Call(CallScheme::StaticCall) => ("STATICCALL", dm.theme.call_color),
            CallType::Create(CreateScheme::Create) => ("CREATE", dm.theme.create_color),
            CallType::Create(CreateScheme::Create2 { .. }) => ("CREATE2", dm.theme.create_color),
            CallType::Create(CreateScheme::Custom { .. }) => {
                ("CUSTOM_CREATE", dm.theme.create_color)
            }
        };

        // Build spans with the new format
        let mut spans = vec![
            Span::styled(line_prefix, Style::default().fg(dm.theme.comment_color)),
            Span::styled(call_type_str, Style::default().fg(call_color)),
            Span::raw(" "),
            Span::styled(
                dm.resolver.resolve_sol_value(&DynSolValue::Address(entry.code_address), None),
                Style::default().fg(dm.theme.accent_color),
            ),
        ];

        // Add function call details
        if matches!(entry.call_type, CallType::Create(_)) {
            if let Some(constructor_call) = dm.resolver.resolve_constructor_call(entry.code_address)
            {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    constructor_call,
                    Style::default().fg(dm.theme.keyword_color),
                ));
            } else {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    "constructor(...)",
                    Style::default().fg(dm.theme.error_color),
                ));
            }
        } else if let Some(call_str) =
            dm.resolver.resolve_function_call(&entry.input, Some(entry.code_address))
        {
            spans.push(Span::raw(" "));
            spans.extend(self.highlight_solidity_code(call_str, dm));
        } else if !entry.input.is_empty() {
            spans.push(Span::raw(" "));
            if entry.input.len() >= 4 {
                let selector = hex::encode(&entry.input[..4]);
                let data_size = entry.input.len() - 4;
                if data_size > 0 {
                    spans.push(Span::styled(
                        format!("0x{selector}...({data_size} bytes)"),
                        Style::default().fg(dm.theme.syntax_string_color),
                    ));
                } else {
                    spans.push(Span::styled(
                        format!("0x{selector}"),
                        Style::default().fg(dm.theme.syntax_string_color),
                    ));
                }
            }
        }

        // Add self-destruct indicator if present
        if let Some((beneficiary, value)) = &entry.self_destruct {
            spans.push(Span::styled(" [SELFDESTRUCT]", Style::default().fg(dm.theme.error_color)));
            spans.push(Span::styled(
                format!(
                    " → {} ({} ETH)",
                    dm.resolver.resolve_sol_value(&DynSolValue::Address(*beneficiary), None),
                    dm.resolver.resolve_ether(*value)
                ),
                Style::default().fg(dm.theme.warning_color),
            ));
        }

        // Result indicator with more specific symbols based on InstructionResult
        let (result_char, result_color) = match &entry.result {
            Some(CallResult::Success { result, .. }) => match result {
                InstructionResult::Return => ("✓", dm.theme.success_color),
                InstructionResult::Stop => ("•", dm.theme.success_color),
                InstructionResult::SelfDestruct => ("†", dm.theme.warning_color),
                _ => ("✓", dm.theme.success_color),
            },
            Some(CallResult::Revert { result, .. }) => match result {
                InstructionResult::Revert => ("↩", dm.theme.error_color),
                _ => ("✗", dm.theme.error_color),
            },
            Some(CallResult::Error { result, .. }) => match result {
                InstructionResult::OutOfGas => ("G", dm.theme.warning_color),
                InstructionResult::StackOverflow => ("S", dm.theme.warning_color),
                InstructionResult::OpcodeNotFound => ("X", dm.theme.error_color),
                InstructionResult::OutOfFunds => ("F", dm.theme.warning_color),
                _ => ("E", dm.theme.warning_color),
            },
            None => ("?", dm.theme.comment_color),
        };

        spans.push(Span::raw(" "));
        spans.push(Span::styled(result_char, Style::default().fg(result_color)));

        // Add ether value if present
        if entry.value > 0 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("[{} ETH]", dm.resolver.resolve_ether(entry.value)),
                Style::default().fg(dm.theme.warning_color),
            ));
        }

        Line::from(spans)
    }

    /// Format an event line
    fn format_event_line(
        &mut self,
        entry: &TraceEntry,
        event_idx: usize,
        dm: &mut DataManager,
    ) -> Line<'static> {
        let trace = dm.execution.get_trace();
        if let Some(event) = entry.events.get(event_idx) {
            // Detail line: vertical connector + spaces + dot
            let child_line = if self.has_children(entry, trace) { "│" } else { " " };
            let full_indent = if entry.depth == 0 {
                // Root level details - check if this root entry has more siblings
                if self.is_last_child(entry, trace) {
                    "      ".to_string()
                } else {
                    "  │   ".to_string()
                }
            } else {
                // Child level details - always start with 2 spaces, then tree indent, then
                // connector
                let tree_indent = self.build_tree_indent_clean(entry, trace);
                if self.is_last_child(entry, trace) {
                    format!("  {tree_indent}  {child_line}   ")
                } else {
                    format!("  {tree_indent}│ {child_line}   ")
                }
            };

            let event_text = match dm.resolver.resolve_event(event, Some(entry.code_address)) {
                Some(text) => text,
                None if event.topics().is_empty() => {
                    format!("Anonymous event ({} bytes data)", event.data.len())
                }
                None => format!(
                    "Event 0x{}... ({} indexed, {} bytes data)",
                    hex::encode(&event.topics()[0].as_slice()[..4]),
                    event.topics().len() - 1,
                    event.data.len()
                ),
            };

            Line::from(vec![
                Span::styled(full_indent, Style::default().fg(dm.theme.comment_color)),
                Span::styled("· ", Style::default().fg(dm.theme.comment_color)),
                Span::styled("[EVENT] ", Style::default().fg(dm.theme.comment_color)),
                Span::styled(event_text, Style::default().fg(dm.theme.accent_color)),
            ])
        } else {
            Line::from("Invalid event")
        }
    }

    /// Format a return value line  
    fn format_return_line(&mut self, entry: &TraceEntry, dm: &mut DataManager) -> Line<'static> {
        let trace = dm.execution.get_trace();
        // Detail line: vertical connector + spaces + dot
        let child_line = if self.has_children(entry, trace) { "│" } else { " " };
        let full_indent = if entry.depth == 0 {
            // Root level details - check if this root entry has more siblings
            if self.is_last_child(entry, trace) {
                "      ".to_string()
            } else {
                "  │   ".to_string()
            }
        } else {
            // Child level details - always start with 2 spaces, then tree indent, then connector
            let tree_indent = self.build_tree_indent_clean(entry, trace);
            if self.is_last_child(entry, trace) {
                format!("  {tree_indent}  {child_line}   ")
            } else {
                format!("  {tree_indent}│ {child_line}   ")
            }
        };

        match &entry.result {
            Some(CallResult::Success { output, .. }) => {
                let return_text = if output.is_empty() {
                    "()".to_string()
                } else {
                    // Try to decode return value using ABI
                    match dm.resolver.resolve_function_return(
                        &entry.input,
                        output,
                        Some(entry.code_address),
                    ) {
                        Some(return_str) => return_str,
                        None => {
                            if output.len() <= 32 {
                                format!("0x{}", hex::encode(output))
                            } else {
                                format!(
                                    "0x{}...({} bytes)",
                                    hex::encode(&output[..8]),
                                    output.len()
                                )
                            }
                        }
                    }
                };

                Line::from(vec![
                    Span::styled(full_indent, Style::default().fg(dm.theme.comment_color)),
                    Span::styled("· ", Style::default().fg(dm.theme.comment_color)),
                    Span::styled("[RETURN] ", Style::default().fg(dm.theme.comment_color)),
                    Span::styled(return_text, Style::default().fg(dm.theme.syntax_string_color)),
                ])
            }
            Some(CallResult::Revert { output, .. }) => {
                let revert_text = self.decode_revert_reason(output);
                Line::from(vec![
                    Span::styled(full_indent, Style::default().fg(dm.theme.comment_color)),
                    Span::styled("· ", Style::default().fg(dm.theme.comment_color)),
                    Span::styled("[REVERT] ", Style::default().fg(dm.theme.error_color)),
                    Span::styled(revert_text, Style::default().fg(dm.theme.syntax_string_color)),
                ])
            }
            Some(CallResult::Error { output, result }) => {
                let error_text = self.format_instruction_result(*result, output);
                Line::from(vec![
                    Span::styled(full_indent, Style::default().fg(dm.theme.comment_color)),
                    Span::styled("· ", Style::default().fg(dm.theme.comment_color)),
                    Span::styled("[ERROR] ", Style::default().fg(dm.theme.error_color)),
                    Span::styled(error_text, Style::default().fg(dm.theme.error_color)),
                ])
            }
            None => Line::from("Unknown result"),
        }
    }

    /// Decode revert reason from output data
    fn decode_revert_reason(&self, output: &Bytes) -> String {
        if output.is_empty() {
            return "(empty revert)".to_string();
        }

        // Check if it's a standard Error(string) revert (0x08c379a0)
        if output.len() >= 4 && output.starts_with(&[0x08, 0xc3, 0x79, 0xa0]) {
            // Try to decode the string from Error(string) signature
            if let Ok(DynSolValue::String(reason)) =
                alloy_dyn_abi::DynSolType::String.abi_decode(&output[4..])
            {
                return format!("\"{reason}\"");
            }
        }

        // Check if it's a Panic(uint256) revert (0x4e487b71)
        if output.len() >= 4
            && output.starts_with(&[0x4e, 0x48, 0x7b, 0x71])
            && let Ok(DynSolValue::Uint(panic_code, _)) =
                alloy_dyn_abi::DynSolType::Uint(256).abi_decode(&output[4..])
        {
            let panic_reason = match panic_code.to_string().as_str() {
                "1" => "assertion failed",
                "17" => "arithmetic overflow/underflow",
                "18" => "division by zero",
                "33" => "enum conversion error",
                "34" => "invalid storage byte array access",
                "49" => "pop() on empty array",
                "50" => "array index out of bounds",
                "65" => "memory allocation overflow",
                "81" => "zero initialization of invalid type",
                _ => "unknown panic",
            };
            return format!("Panic({panic_code}: {panic_reason})");
        }

        // Fallback: show hex data
        if output.len() <= 32 {
            format!("0x{}", hex::encode(output))
        } else {
            format!("0x{}...({} bytes)", hex::encode(&output[..8]), output.len())
        }
    }

    /// Format InstructionResult with context
    fn format_instruction_result(&self, result: InstructionResult, output: &Bytes) -> String {
        match result {
            InstructionResult::Stop => "stop".to_string(),
            InstructionResult::Return => "return".to_string(),
            InstructionResult::SelfDestruct => "selfdestruct".to_string(),
            InstructionResult::Revert => {
                // This shouldn't happen in Error variant, but handle it
                self.decode_revert_reason(output)
            }
            InstructionResult::CallTooDeep => "call stack too deep".to_string(),
            InstructionResult::OutOfFunds => "insufficient funds".to_string(),
            InstructionResult::InvalidJump => "invalid jump destination".to_string(),
            InstructionResult::StackOverflow => "stack overflow".to_string(),
            InstructionResult::StackUnderflow => "stack underflow".to_string(),
            InstructionResult::OutOfGas => "out of gas".to_string(),
            InstructionResult::MemoryOOG => "out of gas (memory)".to_string(),
            InstructionResult::MemoryLimitOOG => "memory limit exceeded".to_string(),
            InstructionResult::PrecompileOOG => "precompile out of gas".to_string(),
            InstructionResult::InvalidOperandOOG => "invalid operand (OOG)".to_string(),
            InstructionResult::OpcodeNotFound => "opcode not found".to_string(),
            InstructionResult::CreateInitCodeSizeLimit => "init code size limit".to_string(),
            InstructionResult::CreateContractSizeLimit => "contract size limit".to_string(),
            InstructionResult::OverflowPayment => "payment overflow".to_string(),
            InstructionResult::StateChangeDuringStaticCall => {
                "state change in static call".to_string()
            }
            InstructionResult::CallNotAllowedInsideStatic => {
                "call not allowed in static".to_string()
            }
            InstructionResult::OutOfOffset => "out of offset".to_string(),
            InstructionResult::CreateCollision => "create collision".to_string(),
            InstructionResult::FatalExternalError => "fatal external error".to_string(),
            _ => format!("unknown error ({result:?})"),
        }
    }

    /// Apply syntax highlighting to Solidity code using the existing syntax highlighter (for owned
    /// strings)
    fn highlight_solidity_code(&self, code: String, dm: &DataManager) -> Vec<Span<'static>> {
        let tokens = self.syntax_highlighter.tokenize(&code, SyntaxType::Solidity);

        let mut spans = Vec::new();
        let mut last_end = 0;

        // Apply syntax highlighting to tokens (same pattern as code panel)
        for token in tokens {
            // Add any unhighlighted text before this token
            if token.start > last_end {
                let unhighlighted = code[last_end..token.start].to_owned();
                if !unhighlighted.is_empty() {
                    spans.push(Span::raw(unhighlighted));
                }
            }

            // Add the highlighted token
            let token_text = code[token.start..token.end].to_owned();
            let token_style = self.syntax_highlighter.get_token_style(token.token_type, &dm.theme);
            spans.push(Span::styled(token_text, token_style));

            last_end = token.end;
        }

        // Add any remaining unhighlighted text
        if last_end < code.len() {
            let remaining = code[last_end..].to_owned();
            if !remaining.is_empty() {
                spans.push(Span::raw(remaining));
            }
        }

        spans
    }

    /// Update execution info from DataManager
    fn update_execution_info(&mut self, dm: &mut DataManager) -> Option<()> {
        let snapshot_id = dm.execution.get_current_snapshot();

        // Check if snapshot changed
        if self.current_execution_snapshot == Some(snapshot_id) {
            return Some(());
        }

        // Get the trace entry ID from snapshot
        let snapshot_info = dm.execution.get_snapshot_info(snapshot_id)?;
        let trace_entry_id = snapshot_info.frame_id().trace_entry_id();

        self.current_execution_entry = Some(trace_entry_id);
        self.current_execution_snapshot = Some(snapshot_id);

        Some(())
    }

    /// Calculate the depth of execution relative to a given entry
    /// Returns None if execution is not within this entry's subtree
    fn calculate_execution_depth(&self, entry_id: usize, trace: &Trace) -> Option<usize> {
        if let Some(current_exec_entry) = self.current_execution_entry {
            // If this is the executing entry itself, depth is 0
            if entry_id == current_exec_entry {
                return Some(0);
            }

            // If execution is a descendant, calculate the depth
            if self.is_descendant_of(current_exec_entry, entry_id, trace) {
                let entry_depth = trace.get(entry_id)?.depth;
                let exec_depth = trace.get(current_exec_entry)?.depth;
                Some(exec_depth - entry_depth)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Check if child_id is a descendant of parent_id in the trace tree
    fn is_descendant_of(&self, child_id: usize, parent_id: usize, trace: &Trace) -> bool {
        if let Some(child_entry) = trace.get(child_id) {
            let mut current_parent = child_entry.parent_id;

            while let Some(parent) = current_parent {
                if parent == parent_id {
                    return true;
                }

                // Move up the tree
                if let Some(parent_entry) = trace.get(parent) {
                    current_parent = parent_entry.parent_id;
                } else {
                    break;
                }
            }
        }
        false
    }

    /// Generate enhanced collapse indicator with numeric depth for execution
    fn get_enhanced_collapse_indicator(&self, entry_id: usize, trace: &Trace) -> String {
        let is_collapsed = self.collapsed_entries.contains(&entry_id);
        let base_indicator = if is_collapsed { "▶" } else { "▼" };

        // Check if we should add numeric depth indicator
        if let Some(depth) = self.calculate_execution_depth(entry_id, trace) {
            if depth == 0 {
                // This entry is currently executing
                format!("{base_indicator} ")
            } else if depth > 0 {
                // Entry with execution inside - show subtle superscript numeric indicator
                let superscript = self.number_to_superscript(depth);
                format!("{base_indicator}{superscript} ")
            } else {
                // Standard indicator
                format!("{base_indicator} ")
            }
        } else {
            // No execution involvement - standard indicator
            format!("{base_indicator} ")
        }
    }

    /// Convert a number to Unicode superscript characters
    fn number_to_superscript(&self, num: usize) -> String {
        const SUPERSCRIPT_DIGITS: [&str; 10] = ["⁰", "¹", "²", "³", "⁴", "⁵", "⁶", "⁷", "⁸", "⁹"];

        if num == 0 {
            return SUPERSCRIPT_DIGITS[0].to_string();
        }

        let mut result = String::new();
        let mut n = num;

        // Handle multi-digit numbers by building from right to left
        let mut digits = Vec::new();
        while n > 0 {
            digits.push(n % 10);
            n /= 10;
        }

        // Reverse to get correct order and convert to superscript
        for digit in digits.iter().rev() {
            if *digit < 10 {
                result.push_str(SUPERSCRIPT_DIGITS[*digit]);
            }
        }

        result
    }
}

impl PanelTr for TracePanel {
    fn panel_type(&self) -> PanelType {
        PanelType::Trace
    }

    fn title(&self, dm: &mut DataManager) -> String {
        let trace = dm.execution.get_trace();
        let visible_entries =
            trace.iter().filter(|entry| self.is_entry_visible(entry, trace)).count();
        format!("Trace ({} lines, {} entries)", self.displayed_line_count, visible_entries)
    }

    fn render(&mut self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        // Update context height and width for viewport calculations
        self.context_height = if self.focused && area.height > 10 {
            area.height.saturating_sub(4) // Account for borders and status lines
        } else {
            area.height.saturating_sub(2) // Just borders
        } as usize;
        self.content_width = area.width.saturating_sub(2) as usize; // Account for borders

        // Update execution info from DataManager
        self.inner.update_execution_info(dm);

        if self.trace.is_none() {
            self.trace = Some(dm.execution.get_trace().clone());
        }

        let trace = self.trace.as_ref().unwrap(); // This must be safe
        if trace.is_empty() {
            let paragraph = Paragraph::new("Trace is empty").block(BorderPresets::trace(
                self.focused,
                self.title(dm),
                dm.theme.focused_border,
                dm.theme.unfocused_border,
            ));
            frame.render_widget(paragraph, area);
            return;
        }

        // Generate display lines with expansion/collapse support
        let display_lines = self.generate_display_lines(trace);
        self.inner.displayed_line_count = display_lines.len();

        // Calculate max line width for horizontal scrolling
        self.inner.calculate_max_line_width(trace, dm);

        let items: Vec<ListItem<'_>> = display_lines
            .iter()
            .enumerate()
            .skip(self.scroll_offset)
            .take(self.context_height)
            .map(|(global_index, line_type)| {
                // Check if this line is the current execution
                let entry_id = match line_type {
                    TraceLineType::Call(id)
                    | TraceLineType::Return(id)
                    | TraceLineType::Event(id, _) => *id,
                };
                let is_execution = self.inner.current_execution_entry == Some(entry_id);
                let is_selected = global_index == self.inner.selected_index;

                let mut formatted_line = self.inner.format_display_line(line_type, trace, dm);

                // Apply horizontal scrolling offset
                formatted_line = self.inner.apply_horizontal_offset(formatted_line);

                // Determine style based on execution and selection state
                let style = if is_execution && is_selected && self.inner.focused {
                    // Both execution and selection on same line
                    Style::default().bg(dm.theme.current_line_bg).fg(dm.theme.selection_fg)
                } else if is_execution && matches!(line_type, TraceLineType::Call(_)) {
                    // Execution line (only for the call line)
                    Style::default().bg(dm.theme.current_line_bg)
                } else if is_selected && self.inner.focused {
                    // Selected line (focused)
                    Style::default().bg(dm.theme.selection_bg).fg(dm.theme.selection_fg)
                } else if is_selected {
                    // Selected line (unfocused)
                    Style::default().bg(dm.theme.highlight_bg)
                } else {
                    Style::default()
                };

                ListItem::new(formatted_line).style(style)
            })
            .collect();

        let list = List::new(items)
            .block(BorderPresets::trace(
                self.focused,
                self.title(dm),
                dm.theme.focused_border,
                dm.theme.unfocused_border,
            ))
            .highlight_style(Style::default().bg(dm.theme.selection_bg));

        frame.render_widget(list, area);

        // Add status and help text at the bottom if focused
        if self.focused && area.height > 10 {
            // Status line
            let status_area = Rect {
                x: area.x + 1,
                y: area.y + area.height - 3,
                width: area.width - 2,
                height: 1,
            };

            let selected_entry_id =
                self.inner.selected_entry(trace).map(|e| e.id).unwrap_or_default();

            let exec_entry = self
                .inner
                .current_execution_entry
                .map_or("None".to_string(), |id| format!("{}", id + 1));

            let status_bar = StatusBar::new()
                .current_panel("Trace".to_string())
                .message(format!(
                    "Exec: {} | User: {}/{}",
                    exec_entry,
                    self.selected_index + 1,
                    display_lines.len()
                ))
                .message(format!("Trace: {}/{}", selected_entry_id + 1, trace.len()));

            let status_text = status_bar.build();

            // Add horizontal scroll indicator if content is scrollable
            let final_status_text = if self.inner.max_line_width > self.inner.content_width {
                let scrollable_width =
                    self.inner.max_line_width.saturating_sub(self.inner.content_width);
                let scroll_percentage = if scrollable_width > 0 {
                    (self.inner.horizontal_offset as f32 / scrollable_width as f32).min(1.0)
                } else {
                    0.0
                };

                let indicator_width = 15;
                let thumb_position = (scroll_percentage * (indicator_width - 3) as f32) as usize;

                let mut indicator = String::from(" [");
                for i in 0..indicator_width {
                    if i >= thumb_position && i < thumb_position + 3 {
                        indicator.push('█');
                    } else {
                        indicator.push('─');
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

            let help_area = Rect {
                x: area.x + 1,
                y: area.y + area.height - 2,
                width: area.width - 2,
                height: 1,
            };
            let help_text = if self.inner.vim_command_mode {
                // Show VIM command mode prompt
                format!(":{}", self.inner.vim_command_buffer)
            } else {
                let mut help = String::from("Vim-like Navigation");
                help.push_str(" • V/C: View/Goto code");
                help.push_str(" • Enter: Expand");
                help.push_str(" • Space: Code Panel");
                help.push_str(" • ?: Help");
                help
            };

            let help_style = if self.inner.vim_command_mode {
                // Use a different style for VIM command mode to make it more prominent
                Style::default().fg(dm.theme.help_text_color).bg(dm.theme.highlight_bg)
            } else {
                Style::default().fg(dm.theme.help_text_color)
            };

            let help_paragraph = Paragraph::new(help_text).style(help_style);
            frame.render_widget(help_paragraph, help_area);
        }
    }

    fn handle_key_event(&mut self, event: KeyEvent, dm: &mut DataManager) -> Result<EventResponse> {
        if !self.focused || event.kind != KeyEventKind::Press {
            return Ok(EventResponse::NotHandled);
        }

        if self.trace.is_none() {
            self.trace = Some(dm.execution.get_trace().clone());
        }

        let trace = self.trace.as_ref().unwrap(); // This must be safe

        // Handle VIM command mode first
        if self.inner.vim_command_mode {
            match event.code {
                KeyCode::Backspace => {
                    self.inner.vim_command_buffer.pop();
                    Ok(EventResponse::Handled)
                }
                KeyCode::Enter => {
                    self.inner.execute_vim_command();
                    Ok(EventResponse::Handled)
                }
                KeyCode::Esc => {
                    self.inner.vim_command_buffer.clear();
                    self.inner.vim_command_mode = false;
                    Ok(EventResponse::Handled)
                }
                KeyCode::Char('[') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    // Ctrl-[ is equivalent to Esc
                    self.inner.vim_command_buffer.clear();
                    self.inner.vim_command_mode = false;
                    Ok(EventResponse::Handled)
                }
                KeyCode::Char(c) => {
                    self.inner.vim_command_buffer.push(c);
                    Ok(EventResponse::Handled)
                }
                _ => Ok(EventResponse::Handled),
            }
        } else {
            match event.code {
                // Handle numeric input for VIM repetition
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    self.inner.vim_number_prefix.push(c);
                    Ok(EventResponse::Handled)
                }
                // VIM-like navigation: k (up)
                KeyCode::Up | KeyCode::Char('k') => {
                    let count = self.inner.get_vim_repetition();
                    self.inner.move_up(count);
                    self.inner.clear_vim_state();
                    Ok(EventResponse::Handled)
                }
                // VIM-like navigation: j (down)
                KeyCode::Down | KeyCode::Char('j') => {
                    let count = self.inner.get_vim_repetition();
                    self.inner.move_down(count);
                    self.inner.clear_vim_state();
                    Ok(EventResponse::Handled)
                }
                // VIM-like navigation: h (left scroll)
                KeyCode::Left | KeyCode::Char('h') => {
                    let count = self.inner.get_vim_repetition();
                    if self.inner.horizontal_offset > 0 {
                        self.inner.horizontal_offset =
                            self.inner.horizontal_offset.saturating_sub(count * 5);
                        debug!("Scrolled left to offset {}", self.inner.horizontal_offset);
                    }
                    self.inner.clear_vim_state();
                    Ok(EventResponse::Handled)
                }
                // VIM-like navigation: l (right scroll)
                KeyCode::Right | KeyCode::Char('l') => {
                    let count = self.inner.get_vim_repetition();
                    if self.inner.max_line_width > self.inner.content_width {
                        let max_scroll =
                            self.inner.max_line_width.saturating_sub(self.inner.content_width);
                        if self.inner.horizontal_offset < max_scroll {
                            self.inner.horizontal_offset =
                                (self.inner.horizontal_offset + count * 5).min(max_scroll);
                            debug!("Scrolled right to offset {}", self.inner.horizontal_offset);
                        }
                    }
                    self.inner.clear_vim_state();
                    Ok(EventResponse::Handled)
                }
                // VIM-like navigation: g (might be gg)
                KeyCode::Char('g') => {
                    // Handle 'gg' sequence
                    if self.inner.vim_number_prefix == "g" {
                        self.inner.move_to(1);
                        self.inner.vim_number_prefix.clear();
                    } else {
                        self.inner.vim_number_prefix = String::from("g");
                    }
                    Ok(EventResponse::Handled)
                }
                // VIM-like navigation: G (go to bottom or specific line)
                KeyCode::Char('G') => {
                    if self.inner.vim_number_prefix.is_empty() {
                        // G without number - go to bottom
                        let max_lines = self.displayed_line_count;
                        self.inner.move_to(max_lines);
                    } else if self.inner.vim_number_prefix != "g" {
                        // nG - go to line n
                        let line = self.inner.get_vim_repetition();
                        self.inner.move_to(line);
                    }
                    self.inner.clear_vim_state();
                    Ok(EventResponse::Handled)
                }
                // VIM command mode
                KeyCode::Char(':') => {
                    self.inner.vim_command_mode = true;
                    self.inner.vim_command_buffer.clear();
                    Ok(EventResponse::Handled)
                }
                // Page navigation
                KeyCode::PageUp => {
                    self.inner.move_up(self.inner.context_height / 2);
                    Ok(EventResponse::Handled)
                }
                KeyCode::PageDown => {
                    self.inner.move_down(self.inner.context_height / 2);
                    Ok(EventResponse::Handled)
                }
                // Other operations
                KeyCode::Enter => {
                    self.inner.toggle_expansion(trace);
                    Ok(EventResponse::Handled)
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    if let Some(entry) = self.inner.selected_entry(trace) {
                        debug!("Selected trace entry ID: {} at depth: {}", entry.id, entry.depth);

                        // Jump to the first snapshot of this trace entry if available
                        if let Some(snapshot_id) = entry.first_snapshot_id {
                            debug!("Jumping to snapshot: {}", snapshot_id);
                            dm.execution.goto(snapshot_id, false)?;
                        } else {
                            bail!(
                                "The selected trace entry has no associated snapshot (likely a state variable view function). We cannot change execution to this entry."
                            );
                        }
                    } else {
                        bail!(
                            "Internal Error: No trace entry selected. Please report this issue to https://github.com/edb-rs/edb/issues."
                        );
                    }
                    Ok(EventResponse::ChangeFocus(PanelType::Code))
                }
                KeyCode::Char('v') | KeyCode::Char('V') => {
                    if let Some(entry) = self.inner.selected_entry(trace) {
                        debug!("Selected trace entry ID: {} at depth: {}", entry.id, entry.depth);

                        // Jump to the first snapshot of this trace entry if available
                        if let Some(snapshot_id) = entry.first_snapshot_id {
                            debug!("Jumping to snapshot: {}", snapshot_id);
                            dm.execution.display(snapshot_id)?;
                        } else {
                            bail!(
                                "The selected trace entry has no associated snapshot (likely a state variable view function). We cannot display code view."
                            );
                        }
                    } else {
                        bail!(
                            "Internal Error: No trace entry selected. Please report this issue to https://github.com/edb-rs/edb/issues."
                        );
                    }
                    Ok(EventResponse::ChangeFocus(PanelType::Code))
                }
                _ => Ok(EventResponse::NotHandled),
            }
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
                self.inner.move_up(1);
                Ok(EventResponse::Handled)
            }
            MouseEventKind::ScrollDown => {
                self.inner.move_down(1);
                Ok(EventResponse::Handled)
            }
            _ => Ok(EventResponse::NotHandled),
        }
    }

    fn on_focus(&mut self) {
        self.focused = true;
        debug!("Trace panel gained focus");
    }

    fn on_blur(&mut self) {
        self.focused = false;
        debug!("Trace panel lost focus");
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
