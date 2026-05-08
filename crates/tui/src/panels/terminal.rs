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

//! Terminal panel for command input and output
//!
//! This panel provides a command-line interface for debugging commands.

use super::{EventResponse, PanelTr, PanelType};
use crate::data::DataManager;
use crate::panels::utils;
use crate::ui::borders::BorderPresets;
use crate::ui::icons::Icons;
use crate::ui::status::{ConnectionStatus, ExecutionStatus, StatusBar};
use crate::ui::syntax::{SyntaxHighlighter, SyntaxType};
use crate::{Spinner, SpinnerStyles};
use alloy_dyn_abi::DynSolValue;
use alloy_primitives::{Address, U256};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use edb_common::normalize_expression;
use edb_common::types::{
    Breakpoint, BreakpointLocation, Code, SnapshotInfoDetail, SolValueFormatterContext,
};
use eyre::{bail, eyre, Result};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use std::collections::VecDeque;
use std::str::FromStr;
use std::time::Instant;
use tracing::debug;

/// Maximum number of terminal lines to keep in history
const MAX_TERMINAL_LINES: usize = 1000;

/// Maximum number of command history entries
const MAX_COMMAND_HISTORY: usize = 100;

/// Type of terminal line
#[derive(Debug, Clone, PartialEq)]
enum LineType {
    /// User command (prefixed with ">")
    Command,
    /// Command output
    Output,
    /// Error message (red color)
    Error,
    /// System message (blue color)
    System,
}

/// Terminal line with content and type
#[derive(Debug, Clone)]
struct TerminalLine {
    /// Line content
    content: String,
    /// Type of line for styling
    line_type: LineType,
}

/// Terminal interaction mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalMode {
    /// Normal typing mode (default)
    Insert,
    /// Navigation mode for scrolling (vim-style)
    Vim,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingCommand {
    /// Step forward in execution
    StepForward(usize),
    /// Step backward in execution
    StepBackward(usize),
    /// Goto next call in execution
    NextCall(usize),
    /// Goto previous call in execution
    PrevCall(usize),
    /// Step forward without going into callees
    StepForwardNoCallees(usize),
    /// Step backward without going into callees
    StepBackwardNoCallees(usize),
    /// Goto to a specific snapshot
    Goto(usize, Option<&'static str>),
    /// Fetch callable ABI for address
    CallableAbi(usize, Option<Address>),
    /// Show stack (top N items)
    ShowStack(usize, usize),
    /// Show memory at offset (N bytes)
    ShowMemory(usize, usize, usize),
    /// Show calldata
    ShowCalldata(usize),
    /// Show storage at slot
    ShowStorage(usize, U256),
    /// Show transient storage at slot
    ShowTransientStorage(usize, U256),
    /// Show current address information
    ShowAddress(usize),
    /// Evaluate Solidity expression
    EvalExpr(usize, String),
    /// Resolve a breakpoint
    BreakpointHits(Breakpoint),
}

impl PendingCommand {
    /// Check whether the command is still pending
    fn is_pending(&self, dm: &mut DataManager) -> bool {
        fn inner(cmd: &PendingCommand, dm: &mut DataManager) -> Option<()> {
            match cmd {
                PendingCommand::StepForward(_) | PendingCommand::StepBackward(_) => {
                    let id = dm.execution.get_current_snapshot();
                    dm.execution.get_snapshot_info(id)?;
                    dm.execution.get_code(id)?;
                }
                PendingCommand::NextCall(src_id) => {
                    let id = dm.execution.get_next_call(*src_id)?;
                    dm.execution.get_snapshot_info(id)?;
                    dm.execution.get_code(id)?;
                }
                PendingCommand::PrevCall(src_id) => {
                    let id = dm.execution.get_prev_call(*src_id)?;
                    dm.execution.get_snapshot_info(id)?;
                    dm.execution.get_code(id)?;
                }
                PendingCommand::StepForwardNoCallees(src_id) => {
                    let id = dm.execution.get_snapshot_info(*src_id)?.next_id;
                    dm.execution.get_snapshot_info(id)?;
                    dm.execution.get_code(id)?;
                }
                PendingCommand::StepBackwardNoCallees(src_id) => {
                    let id = dm.execution.get_snapshot_info(*src_id)?.prev_id;
                    dm.execution.get_snapshot_info(id)?;
                    dm.execution.get_code(id)?;
                }
                PendingCommand::Goto(id, _) => {
                    dm.execution.get_snapshot_info(*id)?;
                    dm.execution.get_code(*id)?;
                }
                PendingCommand::CallableAbi(id, address) => {
                    if let Some(addr) = address {
                        dm.resolver.get_callable_abi_list(*addr)?;
                    } else {
                        let info = dm.execution.get_snapshot_info(*id)?;
                        dm.resolver.get_callable_abi_list(info.bytecode_address)?;
                    }
                }
                PendingCommand::ShowStack(id, ..)
                | PendingCommand::ShowMemory(id, ..)
                | PendingCommand::ShowCalldata(id)
                | PendingCommand::ShowTransientStorage(id, ..)
                | PendingCommand::ShowAddress(id) => {
                    dm.execution.get_snapshot_info(*id)?;
                }
                PendingCommand::ShowStorage(id, slot) => {
                    let info = dm.execution.get_snapshot_info(*id)?;
                    let target_address = info.target_address;
                    let expr = format!(
                        "edb_sload(address({}), {})",
                        target_address.to_checksum(None),
                        slot
                    );
                    dm.resolver.eval_on_snapshot(*id, &expr)?;
                }
                PendingCommand::EvalExpr(id, expr) => {
                    dm.resolver.eval_on_snapshot(*id, expr)?;
                }
                PendingCommand::BreakpointHits(bp) => {
                    dm.execution.get_breakpoint_hits(bp)?;
                }
            }
            Some(())
        }

        inner(self, dm).is_none()
    }

    /// Output when command is finished
    fn output_finished(&self, dm: &mut DataManager) -> Result<String> {
        let id = dm.execution.get_current_snapshot();
        match self {
            Self::StepForward(count) => Ok(format!("Stepped forward {count} times to Step {id}")),
            Self::StepBackward(count) => Ok(format!("Stepped backward {count} times to Step {id}")),
            Self::NextCall(src_id) => {
                let next_id =
                    dm.execution.get_next_call(*src_id).ok_or(eyre!("No next call found"))?;
                Ok(format!("Goto Next Call at Step {next_id}"))
            }
            Self::PrevCall(src_id) => {
                let prev_id =
                    dm.execution.get_prev_call(*src_id).ok_or(eyre!("No previous call found"))?;
                Ok(format!("Goto Previous Call at Step {prev_id}"))
            }
            Self::StepForwardNoCallees(src_id) => {
                let next_id = dm
                    .execution
                    .get_snapshot_info(*src_id)
                    .ok_or(eyre!("No next step found"))?
                    .next_id;
                Ok(format!("Stepped to Step {next_id} without going into callees"))
            }
            Self::StepBackwardNoCallees(src_id) => {
                let prev_id = dm
                    .execution
                    .get_snapshot_info(*src_id)
                    .ok_or(eyre!("No previous step found"))?
                    .prev_id;
                Ok(format!("Stepped to Step {prev_id} without going into callees"))
            }
            Self::Goto(id, msg) => match msg {
                Some(m) => Ok(m.to_string()),
                None => Ok(format!("Goto Step {id}")),
            },
            Self::CallableAbi(id, address) => {
                let address = if let Some(addr) = address {
                    *addr
                } else {
                    let info = dm
                        .execution
                        .get_snapshot_info(*id)
                        .ok_or(eyre!("No snapshot info found"))?;
                    info.bytecode_address
                };

                let abi_list = dm
                    .resolver
                    .get_callable_abi_list(address)
                    .ok_or(eyre!("No callable ABI found"))?;

                if abi_list.is_empty() {
                    Ok(format!("No callable ABI found for address {address}"))
                } else {
                    Ok(abi_list.iter().map(|info| format!("{info}")).collect::<Vec<_>>().join("\n"))
                }
            }
            Self::ShowStack(id, count) => {
                let info =
                    dm.execution.get_snapshot_info(*id).ok_or(eyre!("No stack info found"))?;
                match info.detail() {
                    SnapshotInfoDetail::Opcode(detail) => {
                        let stack = &detail.stack;
                        if stack.is_empty() {
                            Ok("Stack is empty".to_string())
                        } else {
                            let top_n =
                                stack.iter().rev().take(*count).cloned().collect::<Vec<_>>();
                            Ok(format!(
                                "Top {} Stack Items (most recent first):\n{}",
                                top_n.len(),
                                top_n
                                    .iter()
                                    .enumerate()
                                    .map(|(i, val)| format!(
                                        "{:>4} | {}",
                                        i,
                                        utils::format_value_with_decode(val)
                                    ))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            ))
                        }
                    }
                    SnapshotInfoDetail::Hook(_) => bail!("No stack info available in source mode."),
                }
            }
            Self::ShowMemory(id, offset, count) => {
                let info =
                    dm.execution.get_snapshot_info(*id).ok_or(eyre!("No memory info found"))?;
                match info.detail() {
                    SnapshotInfoDetail::Opcode(detail) => {
                        let memory = &detail.memory;
                        let start = *offset;
                        let end = start.saturating_add(*count);

                        // Determine overlap between requested range and actual memory
                        let memory_end = memory.len();

                        let slice: Vec<u8> = if start >= memory_end {
                            // Completely out of bounds - all zeros
                            vec![0; end - start]
                        } else if end <= memory_end {
                            // Completely in bounds - direct slice
                            memory[start..end].to_vec()
                        } else {
                            // Partially in bounds - slice + padding
                            let mut result = memory[start..memory_end].to_vec();
                            result.resize(end - start, 0);
                            result
                        };

                        Ok(format!(
                            "Memory from offset {} ({} bytes):\n{}",
                            start,
                            slice.len(),
                            utils::format_bytes_with_decode(&slice)
                                .split('\n')
                                .enumerate()
                                .map(|(i, line)| format!("{:>4} | {}", start + i * 32, line))
                                .collect::<Vec<_>>()
                                .join("\n")
                        ))
                    }
                    SnapshotInfoDetail::Hook(_) => {
                        bail!("No memory info available in source mode.")
                    }
                }
            }
            Self::ShowCalldata(id) => {
                let info =
                    dm.execution.get_snapshot_info(*id).ok_or(eyre!("No calldata info found"))?;
                match info.detail() {
                    SnapshotInfoDetail::Opcode(detail) => {
                        let calldata = &detail.calldata;
                        if calldata.is_empty() {
                            Ok("Calldata is empty".to_string())
                        } else {
                            Ok(format!(
                                "Calldata ({} bytes):\n{}",
                                calldata.len(),
                                utils::format_bytes_with_decode(calldata)
                                    .split('\n')
                                    .enumerate()
                                    .map(|(i, line)| format!("{:>4} | {}", i * 32, line))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            ))
                        }
                    }
                    SnapshotInfoDetail::Hook(_) => {
                        bail!("No calldata info available in source mode.")
                    }
                }
            }
            Self::ShowStorage(id, slot) => {
                let info =
                    dm.execution.get_snapshot_info(*id).ok_or(eyre!("No storage info found"))?;
                let target_address = info.target_address;

                let expr =
                    format!("edb_sload(address({}), {})", target_address.to_checksum(None), slot);

                let value = dm
                    .resolver
                    .eval_on_snapshot(*id, &expr)
                    .ok_or(eyre!("No value found"))?
                    .as_ref()
                    .map_err(|e| eyre!("{e}"))?;

                if let DynSolValue::Uint(v, _) = &**value {
                    Ok(utils::format_value_with_decode(v))
                } else {
                    bail!("Expression evaluation did not return a uint value")
                }
            }
            Self::ShowTransientStorage(id, slot) => {
                let info =
                    dm.execution.get_snapshot_info(*id).ok_or(eyre!("No storage info found"))?;
                match info.detail() {
                    SnapshotInfoDetail::Opcode(detail) => {
                        let value = detail
                            .transient_storage
                            .get(&(info.target_address, *slot))
                            .cloned()
                            .unwrap_or(U256::ZERO);
                        Ok(format!("   {}: {}", slot, utils::format_value_with_decode(&value)))
                    }
                    SnapshotInfoDetail::Hook(_) => {
                        bail!("No transient storage info available in source mode.")
                    }
                }
            }
            Self::ShowAddress(id) => {
                let info =
                    dm.execution.get_snapshot_info(*id).ok_or(eyre!("No snapshot info found"))?;
                Ok(format!(
                    "Address:          {}\nBytecode Address: {}",
                    info.target_address, info.bytecode_address
                ))
            }
            Self::EvalExpr(_, expr) => {
                let value =
                    dm.resolver.eval_on_snapshot(id, expr).ok_or(eyre!("No value found"))?.clone();

                let ctx = SolValueFormatterContext::new().with_ty(true).multi_line(true);

                value
                    .map(|v| format!("{} = {}", expr, dm.resolver.resolve_sol_value(&v, Some(ctx))))
                    .map_err(|e| eyre!(e))
            }
            Self::BreakpointHits(bp) => Ok(format!("Breakpoint added: {bp}")),
        }
    }
}

/// Terminal panel implementation with vim-style navigation
#[derive(Debug)]
pub struct TerminalPanel {
    // ========== Display ==========
    /// All terminal content (commands + output intermixed)
    lines: Vec<TerminalLine>,
    /// Current interaction mode
    mode: TerminalMode,
    /// Current input buffer (only active in INSERT mode)
    input_buffer: String,
    /// Cursor position in input buffer
    cursor_position: usize,
    /// Command history for ↑/↓ navigation in INSERT mode
    command_history: VecDeque<String>,
    /// Current position in command history (None = no history browsing)
    history_position: Option<usize>,
    /// Scroll position in terminal history (0 = bottom/latest)
    scroll_offset: usize,
    /// Content height for the current area
    content_height: usize,
    /// Content width for the current area
    content_width: usize,
    /// Horizontal scroll offset (vim mode only)
    horizontal_offset: usize,
    /// Maximum line width for horizontal scrolling
    max_line_width: usize,
    /// Whether this panel is focused
    focused: bool,
    /// Whether we're connected to the RPC server
    connected: bool,
    /// Current snapshot info (current_index, total_count)
    snapshot_info: Option<(usize, usize)>,
    /// Last Ctrl+C press time for double-press detection
    last_ctrl_c: Option<Instant>,
    /// Number prefix for vim navigation (e.g., "5" in "5j")
    vim_number_prefix: String,
    /// VIM mode cursor absolute line number in terminal history (1-based, like code panel)
    vim_cursor_line: usize,
    /// Current pending command
    pending_command: Option<PendingCommand>,
    /// Spinner for command execution
    spinner: Spinner,
    /// Syntax highlighter for commands and output
    syntax_highlighter: SyntaxHighlighter,
}

impl TerminalPanel {
    /// Create a new terminal panel
    pub fn new() -> Self {
        let mut panel = Self {
            lines: Vec::new(),
            mode: TerminalMode::Insert,
            input_buffer: String::new(),
            cursor_position: 0,
            command_history: VecDeque::new(),
            history_position: None,
            scroll_offset: 0,
            content_height: 0,
            content_width: 0,
            horizontal_offset: 0,
            max_line_width: 0,
            focused: false,
            connected: true,
            snapshot_info: Some((127, 348)),
            last_ctrl_c: None,
            vim_number_prefix: String::new(),
            vim_cursor_line: 1, // Start at first line (1-based like code panel)
            pending_command: None,
            spinner: Spinner::new(Some(SpinnerStyles::SQUARE), None),
            syntax_highlighter: SyntaxHighlighter::new(),
        };

        // Add welcome message with fancy styling
        panel.add_output(&format!(
            "{} EDB Time-Travel Debugger v{}",
            Icons::TARGET_REACHED,
            env!("CARGO_PKG_VERSION")
        ));
        panel.add_output(&format!("{} Connected to RPC server", Icons::CONNECTED));
        panel.add_output(&format!("{} Type 'help' for available commands", Icons::INFO));
        panel.add_output("");

        panel
    }

    /// Calculate the maximum line width for horizontal scrolling
    fn calculate_max_line_width(&mut self) {
        self.max_line_width = self
            .lines
            .iter()
            .map(|line| {
                // Account for line type prefix ("> " for commands, "⚡ " for system)
                let prefix_len = match line.line_type {
                    LineType::Command => 2, // "> "
                    LineType::System => 2,  // "⚡ " counts as 2 width
                    _ => 0,
                };
                line.content.len() + prefix_len
            })
            .max()
            .unwrap_or(0);
    }

    /// Apply horizontal offset to a styled line for horizontal scrolling (exactly like code panel)
    fn apply_horizontal_offset_to_line<'a>(
        &self,
        line: ratatui::text::Line<'a>,
    ) -> ratatui::text::Line<'a> {
        use ratatui::text::{Line, Span};

        if self.mode != TerminalMode::Vim || self.horizontal_offset == 0 {
            return line;
        }

        // Calculate the visual width of each span
        let mut accumulated_width = 0;
        let mut new_spans = Vec::new();
        let mut started_content = false;

        for span in line.spans {
            let span_width = span.content.chars().count();

            if accumulated_width + span_width <= self.horizontal_offset {
                // This span is completely before the viewport
                accumulated_width += span_width;
            } else if accumulated_width >= self.horizontal_offset {
                // This span is completely within the viewport
                new_spans.push(span);
                started_content = true;
            } else {
                // This span is partially visible - need to trim the beginning
                let skip_chars = self.horizontal_offset - accumulated_width;
                let visible_content: String = span.content.chars().skip(skip_chars).collect();
                if !visible_content.is_empty() {
                    new_spans.push(Span::styled(visible_content, span.style));
                    started_content = true;
                }
                accumulated_width += span_width;
            }
        }

        // If we've scrolled past all content, show empty line
        if !started_content {
            new_spans.push(Span::raw(""));
        }

        Line::from(new_spans)
    }

    /// Add a line to the terminal with specified type
    fn add_line(&mut self, content: &str, line_type: LineType) {
        if self.lines.len() >= MAX_TERMINAL_LINES {
            self.lines.remove(0);
        }
        self.lines.push(TerminalLine { content: content.to_string(), line_type });
    }

    /// Add output line (convenience method)
    pub fn add_output(&mut self, line: &str) {
        self.add_line(line, LineType::Output);
    }

    /// Add error line (convenience method)
    pub fn add_error(&mut self, line: &str) {
        self.add_line(line, LineType::Error);
    }

    /// Add system message (convenience method)
    pub fn add_system(&mut self, line: &str) {
        self.add_line(&format!("⚡ {line}"), LineType::System);
    }

    /// Add command line (convenience method)
    pub fn add_command(&mut self, command: &str) {
        self.add_line(&format!("> {command}"), LineType::Command);
    }

    /// Execute a command
    fn execute_command(&mut self, command: &str, dm: &mut DataManager) -> Result<EventResponse> {
        debug!("Executing command: {}", command);

        // Add command to history
        if !command.trim().is_empty()
            && self.command_history.back().is_none_or(|last| last != command)
        {
            if self.command_history.len() >= MAX_COMMAND_HISTORY {
                self.command_history.pop_front();
            }
            self.command_history.push_back(command.to_string());
        }

        // Empty command will be treated as the previous command
        if command.trim().is_empty()
            && let Some(cmd) = self.command_history.back().cloned() {
                return self.execute_command(cmd.as_str(), dm);
            }

        // Add command to terminal history
        self.add_command(command);

        // Handle built-in commands
        match command.trim() {
            "" => {
                // We do not have any previous command
                self.add_output("Empty command");
                self.add_output("Type 'help' for available commands");
            }
            "quit" | "q" | "exit" => {
                self.add_system("🚪 Exiting debugger...");
                return Ok(EventResponse::Exit);
            }
            "help" | "h" => {
                self.show_help();
            }
            "clear" | "cls" => {
                self.lines.clear();
                self.add_system("Terminal cleared");
            }
            "history" => {
                self.show_history();
            }
            "theme" => {
                self.show_themes(dm);
            }
            cmd if cmd.starts_with("theme ") => {
                self.handle_theme_command(&cmd[6..], dm);
            }
            cmd if cmd.starts_with("watch") => {
                self.handle_watch_command(cmd[5..].trim(), dm);
            }
            cmd if cmd.starts_with('$') => {
                // Solidity expression evaluation
                let id = dm.execution.get_current_snapshot();
                let expr = cmd[1..].trim();

                self.pending_command = Some(PendingCommand::EvalExpr(id, expr.to_string()));
                self.spinner.start_loading("Fetching evaluation result...");
            }
            cmd => {
                // Debug commands
                if let Err(e) = self.handle_debug_command(cmd, dm) {
                    let error_msg = format!("Internal Error: {e}");
                    self.add_output(&error_msg);
                }
            }
        }

        Ok(EventResponse::Handled)
    }

    /// Handle debug commands
    fn handle_debug_command(&mut self, command: &str, dm: &mut DataManager) -> Result<()> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }

        if dm.execution.get_execution_status().is_waiting() {
            // We should notifiy the user that the backend is still waiting, so they should wait as well
            self.add_error("The backend is waiting for an execution request");
            return Ok(());
        }

        match parts[0] {
            "next" | "n" => {
                let id = dm.execution.get_current_snapshot();
                self.pending_command = Some(PendingCommand::StepForwardNoCallees(id));
                self.spinner.start_loading("Stepping forward without going into callees...");
                dm.execution.next()?;
            }
            "prev" | "p" => {
                let id = dm.execution.get_current_snapshot();
                self.pending_command = Some(PendingCommand::StepBackwardNoCallees(id));
                self.spinner.start_loading("Stepping backward without going into callees...");
                dm.execution.prev()?;
            }
            "step" | "s" => {
                let count =
                    if parts.len() > 1 { parts[1].parse::<usize>().unwrap_or(1) } else { 1 };
                self.pending_command = Some(PendingCommand::StepForward(count));
                self.spinner.start_loading(&format!("Stepping {count} times..."));
                dm.execution.step(count)?;
            }
            "reverse" | "rs" => {
                let count =
                    if parts.len() > 1 { parts[1].parse::<usize>().unwrap_or(1) } else { 1 };
                self.pending_command = Some(PendingCommand::StepBackward(count));
                self.spinner.start_loading(&format!("Reverse stepping {count} times..."));
                dm.execution.reverse_step(count)?;
            }
            "call" | "c" => {
                let id = dm.execution.get_current_snapshot();
                self.pending_command = Some(PendingCommand::NextCall(id));
                self.spinner.start_loading("Stepping to next function call...");
                dm.execution.next_call()?;
            }
            "rcall" | "rc" => {
                let id = dm.execution.get_current_snapshot();
                self.pending_command = Some(PendingCommand::PrevCall(id));
                self.spinner.start_loading("Stepping to previous function call...");
                dm.execution.prev_call()?;
            }
            "run" | "r" => {
                self.pending_command = Some(PendingCommand::Goto(
                    usize::MAX,
                    Some("Running until next breakpoint or end..."),
                ));
                self.spinner.start_loading("Running until next breakpoint or end...");
                dm.execution.goto(usize::MAX, true)?;
            }
            "runback" | "rb" => {
                self.pending_command = Some(PendingCommand::Goto(
                    0,
                    Some("Running backward until previous breakpoint or start..."),
                ));
                self.spinner
                    .start_loading("Running backward until previous breakpoint or start...");
                dm.execution.goto(0, true)?;
            }
            "abi" => {
                let id = dm.execution.get_current_snapshot();
                let address = if parts.len() > 1 {
                    Some(parts[1].parse::<Address>().map_err(|e| eyre!("Invalid address: {}", e))?)
                } else {
                    None
                };

                if let Some(addr) = address {
                    self.spinner.start_loading(&format!("Fetching callable ABI for {addr}..."));
                } else {
                    self.spinner.start_loading("Fetching callable ABI for current address...");
                }
                self.pending_command = Some(PendingCommand::CallableAbi(id, address));
            }
            "goto" | "g" => {
                // A secret debugging cmd
                let mut id =
                    if parts.len() > 1 { parts[1].parse::<usize>().unwrap_or(1) } else { 0 };
                id = dm.execution.get_sanitized_id(id);
                self.pending_command = Some(PendingCommand::Goto(id, None));
                self.spinner.start_loading(&format!("Going to snapshot {id}..."));
                dm.execution.goto(id, false)?; // we do not stop at breakpoints
            }
            "info" => {
                // A secret debugging cmd
                let id = dm.execution.get_current_snapshot();
                if let Some(info) = dm.execution.get_snapshot_info(id) {
                    let info_str = format!("{info:#?}");
                    for line in info_str.lines() {
                        self.add_output(line);
                    }
                } else {
                    self.add_error(&format!("No snapshot info found for id {id}"));
                }
            }
            "break" => {
                if parts.len() < 2 {
                    self.add_output("Usage:");
                    self.add_output("  break add [@<loc>] [if $<expr>] - Add breakpoint");
                    self.add_output("        <loc> := <addr>:<path>:<line> (source)");
                    self.add_output("               | <addr>:<pc>          (opcode)");
                    self.add_output("  break remove <id>               - Remove breakpoint");
                    self.add_output("  break enable [id]               - Enable breakpoint(s)");
                    self.add_output("  break disable [id]              - Disable breakpoint(s)");
                    self.add_output(
                        "  break add_expr <id> $<expr>     - Add condition to breakpoint",
                    );
                    self.add_output("  break list                      - List all breakpoints");
                    self.add_output("  break clear                     - Clear all breakpoints");
                    self.add_output("");
                    self.add_output("Breakpoint types:");
                    self.add_output("  • Location only: Stops when execution reaches the location");
                    self.add_output(
                        "  • Condition only: Watchpoint - stops when expression is true",
                    );
                    self.add_output(
                        "  • Both: Stops when location is reached AND condition is true",
                    );
                    self.add_output(
                        "  • Neither: Invalid - breakpoint must have location or condition",
                    );
                    return Ok(());
                }

                match parts[1] {
                    "add" => {
                        // Parse: break add [@<loc>] [if <expr>]
                        let breakpoint = if parts.len() > 2 {
                            // Join the remaining parts after "break add" into a single string
                            let bp_spec = parts[2..].join(" ");
                            match Breakpoint::from_str(&bp_spec) {
                                Ok(bp) => bp,
                                Err(e) => {
                                    self.add_error(&format!("Failed to parse breakpoint: {e}"));
                                    return Ok(());
                                }
                            }
                        } else {
                            // No arguments provided, create breakpoint at current location
                            let current_id = dm.execution.get_current_snapshot();
                            let Some(info) = dm.execution.get_snapshot_info(current_id) else {
                                self.add_error("Failed to get current step info");
                                return Ok(());
                            };

                            let loc = match info.detail().clone() {
                                SnapshotInfoDetail::Opcode(detail) => {
                                    Some(BreakpointLocation::Opcode {
                                        bytecode_address: info.bytecode_address,
                                        pc: detail.pc,
                                    })
                                }
                                SnapshotInfoDetail::Hook(detail) => {
                                    let Some(Code::Source(info)) =
                                        dm.execution.get_code(current_id)
                                    else {
                                        self.add_error("Failed to get code for current step");
                                        return Ok(());
                                    };

                                    let line_number = info
                                        .sources
                                        .get(&detail.path)
                                        .map(|code| code[..detail.offset + 1].lines().count())
                                        .ok_or(eyre!(
                                            "Failed to determine line number for breakpoint"
                                        ))?;

                                    Some(BreakpointLocation::Source {
                                        bytecode_address: info.bytecode_address,
                                        file_path: detail.path,
                                        line_number,
                                    })
                                }
                            };

                            Breakpoint::new(loc, None)
                        };

                        match dm.execution.add_breakpoint(breakpoint) {
                            Ok((bp, true)) => {
                                self.add_output(&format!("Breakpoint added: {bp}"));
                            }
                            Ok((bp, false)) => {
                                self.pending_command = Some(PendingCommand::BreakpointHits(bp));
                                self.spinner.start_loading("Waiting for resolving breakpoint...");
                            }
                            Err(e) => {
                                self.add_error(&format!("Failed to add breakpoint: {e}"));
                            }
                        }
                    }
                    "remove" => {
                        if parts.len() != 3 {
                            self.add_error("Usage: break remove <id>");
                            return Ok(());
                        }

                        match parts[2].parse::<usize>() {
                            Ok(id) => match dm.execution.remove_breakpoint(id) {
                                Ok(()) => self.add_output(&format!("Breakpoint #{id} removed")),
                                Err(e) => {
                                    self.add_error(&format!("Failed to remove breakpoint: {e}"))
                                }
                            },
                            Err(_) => self.add_error("Invalid breakpoint id"),
                        }
                    }
                    "enable" => {
                        if parts.len() != 3 && parts.len() != 2 {
                            self.add_error("Usage: break enable [id]");
                            return Ok(());
                        }

                        if parts.len() == 2 {
                            match dm.execution.enable_all_breakpoints() {
                                Ok(()) => self.add_output("All breakpoints enabled"),
                                Err(e) => {
                                    self.add_error(&format!("Failed to enable breakpoints: {e}"))
                                }
                            }
                            return Ok(());
                        }

                        match parts[2].parse::<usize>() {
                            Ok(id) => match dm.execution.enable_breakpoint(id) {
                                Ok(()) => self.add_output(&format!("Breakpoint #{id} enabled")),
                                Err(e) => {
                                    self.add_error(&format!("Failed to enable breakpoint: {e}"))
                                }
                            },
                            Err(_) => self.add_error("Invalid breakpoint id"),
                        }
                    }
                    "disable" => {
                        if parts.len() != 3 && parts.len() != 2 {
                            self.add_error("Usage: break disable [id]");
                            return Ok(());
                        }

                        if parts.len() == 2 {
                            match dm.execution.disable_all_breakpoints() {
                                Ok(()) => self.add_output("All breakpoints disabled"),
                                Err(e) => {
                                    self.add_error(&format!("Failed to disable breakpoints: {e}"))
                                }
                            }
                            return Ok(());
                        }

                        match parts[2].parse::<usize>() {
                            Ok(id) => match dm.execution.disable_breakpoint(id) {
                                Ok(()) => self.add_output(&format!("Breakpoint #{id} disabled")),
                                Err(e) => {
                                    self.add_error(&format!("Failed to disable breakpoint: {e}"))
                                }
                            },
                            Err(_) => self.add_error("Invalid breakpoint id"),
                        }
                    }
                    "add_expr" => {
                        if parts.len() < 4 {
                            self.add_error("Usage: break add_expr <id> $<expr>");
                            return Ok(());
                        }

                        match parts[2].parse::<usize>() {
                            Ok(id) => {
                                if let Some(expr) = parts[3..].join(" ").strip_prefix("$") {
                                    match dm
                                        .execution
                                        .update_breakpoint_condition(id, normalize_expression(expr))
                                    {
                                        Ok((bp, true)) => self.add_output(&format!(
                                            "Breakpoint #{id} condition updated: {bp}"
                                        )),
                                        Ok((bp, false)) => {
                                            self.pending_command =
                                                Some(PendingCommand::BreakpointHits(bp));
                                            self.spinner.start_loading(
                                                "Waiting for resolving breakpoint...",
                                            );
                                        }
                                        Err(e) => self
                                            .add_error(&format!("Failed to update condition: {e}")),
                                    }
                                } else {
                                    self.add_error(&format!(
                                        "Expression does not start with $: {}",
                                        parts[3..].join(" ")
                                    ))
                                }
                            }
                            Err(_) => self.add_error("Invalid breakpoint id"),
                        }
                    }
                    "list" => {
                        let breakpoints: Vec<_> = dm.execution.list_breakpoints().collect();
                        if breakpoints.is_empty() {
                            self.add_output("No breakpoints set");
                        } else {
                            self.add_output("Breakpoints:");
                            for (id, bp, enabled) in breakpoints {
                                let status = if enabled { "enabled" } else { "disabled" };
                                let loc_desc = bp
                                    .loc
                                    .as_ref()
                                    .map(|loc| {
                                        loc.display(
                                            dm.resolver
                                                .resolve_address_label(loc.bytecode_address()),
                                        )
                                    })
                                    .unwrap_or_else(|| "no location".to_string());

                                let mut line_str = format!("  #{id}: {loc_desc}");
                                if let Some(code) = &bp.condition {
                                    line_str.push_str(&format!(" if {code}"));
                                }
                                line_str.push_str(&format!(" [{status}]"));

                                self.add_output(&line_str);
                            }
                        }
                    }
                    "clear" => match dm.execution.clear_breakpoints() {
                        Ok(()) => self.add_output("All breakpoints cleared"),
                        Err(e) => self.add_error(&format!("Failed to clear breakpoints: {e}")),
                    },
                    _ => {
                        self.add_error(&format!("Unknown break subcommand: {}", parts[1]));
                        self.add_output("Usage:");
                        self.add_output("  break add [@<loc>] [if $<expr>] - Add breakpoint");
                        self.add_output("        <loc> := <addr>:<path>:<line> (source)");
                        self.add_output("               | <addr>:<pc>          (opcode)");
                        self.add_output("  break remove <id>               - Remove breakpoint");
                        self.add_output("  break enable [id]               - Enable breakpoint(s)");
                        self.add_output(
                            "  break disable [id]              - Disable breakpoint(s)",
                        );
                        self.add_output(
                            "  break add_expr <id> $<expr>     - Add condition to breakpoint",
                        );
                        self.add_output("  break list                      - List all breakpoints");
                        self.add_output(
                            "  break clear                     - Clear all breakpoints",
                        );
                        self.add_output("");
                        self.add_output("Breakpoint types:");
                        self.add_output(
                            "  • Location only: Stops when execution reaches the location",
                        );
                        self.add_output(
                            "  • Condition only: Watchpoint - stops when expression is true",
                        );
                        self.add_output(
                            "  • Both: Stops when location is reached AND condition is true",
                        );
                        self.add_output(
                            "  • Neither: Invalid - breakpoint must have location or condition",
                        );
                    }
                }
            }
            "address" => {
                let id = dm.execution.get_current_snapshot();
                self.pending_command = Some(PendingCommand::ShowAddress(id));
                self.spinner.start_loading("Fetching current address...");
            }
            "stack" => {
                let count =
                    if parts.len() > 1 { parts[1].parse::<usize>().unwrap_or(1) } else { 5 };
                let id = dm.execution.get_current_snapshot();
                self.pending_command = Some(PendingCommand::ShowStack(id, count));
                self.spinner.start_loading(&format!("Fetching stack (top {count} items)..."));
            }
            "memory" => {
                let offset =
                    if parts.len() > 1 { parts[1].parse::<usize>().unwrap_or(0) } else { 0 };
                let count =
                    if parts.len() > 2 { parts[2].parse::<usize>().unwrap_or(256) } else { 256 };
                let id = dm.execution.get_current_snapshot();
                self.pending_command = Some(PendingCommand::ShowMemory(id, offset, count));
                self.spinner.start_loading(&format!(
                    "Fetching memory at offset {offset} ({count} bytes)..."
                ));
            }
            "calldata" => {
                let id = dm.execution.get_current_snapshot();
                self.pending_command = Some(PendingCommand::ShowCalldata(id));
                self.spinner.start_loading("Fetching calldata...");
            }
            "sload" => {
                let id = dm.execution.get_current_snapshot();
                let slot = if parts.len() > 1 {
                    parts[1].parse::<U256>().unwrap_or(U256::ZERO)
                } else {
                    bail!("Usage: sload <slot>");
                };
                self.pending_command = Some(PendingCommand::ShowStorage(id, slot));
                self.spinner.start_loading(&format!("Fetching storage at slot {slot}..."));
            }
            "tsload" => {
                let id = dm.execution.get_current_snapshot();
                let slot = if parts.len() > 1 {
                    parts[1].parse::<U256>().unwrap_or(U256::ZERO)
                } else {
                    bail!("Usage: tsload <slot>");
                };
                self.pending_command = Some(PendingCommand::ShowTransientStorage(id, slot));
                self.spinner
                    .start_loading(&format!("Fetching transient storage at slot {slot}..."));
            }
            _ => {
                self.add_output(&format!("Unknown command: {}", parts[0]));
                self.add_output("Type 'help' for available commands");
            }
        }

        Ok(())
    }

    /// Show help information
    fn show_help(&mut self) {
        self.add_output("📋 EDB Terminal Help");
        self.add_output("");
        self.add_output("🔄 Terminal Navigation (Vim-Style):");
        self.add_output("  Esc                - Switch to VIM mode for navigation");
        self.add_output("  (VIM mode) j/k/↑/↓ - Navigate lines (with auto-scroll)");
        self.add_output("  (VIM mode) 5j/3↓   - Move multiple lines with number prefix");
        self.add_output("  (VIM mode) {/}     - Jump to previous/next blank line");
        self.add_output("  (VIM mode) 3{/2}   - Jump multiple blank lines with number prefix");
        self.add_output("  (VIM mode) gg/G    - Go to top/bottom");
        self.add_output("  (VIM mode) i       - Return to INSERT mode");
        self.add_output("");
        self.add_output("🚀 Debug Commands:");
        self.add_output("  next, n             - Step to next snapshot");
        self.add_output("  prev, p             - Step to previous snapshot");
        self.add_output("  step, s <count>     - Step multiple snapshots");
        self.add_output("  reverse, rs <count> - Reverse step multiple snapshots");
        self.add_output("  call, c             - Step to next function call");
        self.add_output("  rcall, rc           - Step back from function call");
        self.add_output("  run, r              - Run until next breakpoint or end");
        self.add_output("  runback, rb         - Run backward until previous breakpoint or start");
        self.add_output("");
        self.add_output("🔍 Inspection:");
        self.add_output("  address                 - Show current address");
        self.add_output("  abi [address]           - Show callable ABI");
        self.add_output("  stack [count]           - Show current stack");
        self.add_output("  memory [offset] [count] - Show memory");
        self.add_output("  calldata                - Show calldata");
        self.add_output("  sload <slot>            - Show storage at slot");
        self.add_output("  tsload <slot>           - Show transient storage at slot");
        self.add_output("");
        self.add_output("👁️ Watcher:");
        self.add_output("  watch add $<expr>   - Add watch expression");
        self.add_output("  watch remove <id>   - Remove watch expression");
        self.add_output("🔴 Breakpoints:");
        self.add_output("  break add [@<loc>] [if $<expr>] - Add breakpoint");
        self.add_output("        <loc> := <addr>:<path>:<line> (source)");
        self.add_output("               | <addr>:<pc>          (opcode)");
        self.add_output("  break remove <id>               - Remove breakpoint");
        self.add_output("  break enable [id]               - Enable breakpoint (all if no id)");
        self.add_output("  break disable [id]              - Disable breakpoint (all if no id)");
        self.add_output("  break add_expr <id> $<expr>     - Add condition expression");
        self.add_output("  break list                      - List all breakpoints");
        self.add_output("  break clear                     - Clear all breakpoints");
        self.add_output("");
        self.add_output("  Types: - Location-only (stops at location)");
        self.add_output("         - Condition-only (watchpoint)");
        self.add_output("         - Both (location AND condition)");
        self.add_output("         - Neither (invalid)");
        self.add_output("");
        self.add_output("💻 Solidity expressions (prefix with $):");
        self.add_output("  $<expr>          - Evaluate expression");
        self.add_output("  $edb_help()      - Show more help on expressions");
        self.add_output("");
        self.add_output("⚙️  Other:");
        self.add_output("  help, h          - Show this help");
        self.add_output("  clear, cls       - Clear terminal");
        self.add_output("  theme            - Switch theme");
        self.add_output("  history          - Show command history");
        self.add_output("  quit, q, exit    - Exit debugger");
        self.add_output("");
    }

    /// Show command history
    fn show_history(&mut self) {
        if self.command_history.is_empty() {
            self.add_output("No command history");
        } else {
            self.add_output("Command history:");
            let history_lines: Vec<String> = self
                .command_history
                .iter()
                .enumerate()
                .map(|(i, cmd)| format!("  {}: {}", i + 1, cmd))
                .collect();
            for line in history_lines {
                self.add_output(&line);
            }
        }
    }

    /// Show available themes
    fn show_themes(&mut self, dm: &mut DataManager) {
        let themes = dm.theme.list_themes();
        let active_theme = dm.theme.get_active_theme_name();

        self.add_output("Available themes:");
        for (name, _display_name, description) in themes {
            let marker = if name == active_theme { "→" } else { " " };
            self.add_output(&format!("{marker} {name} | {description}"));
        }
        self.add_output("");
        self.add_output("Usage:");
        self.add_output("  theme <name>    Switch to theme");
        self.add_output("  theme           List available themes");
    }

    /// Handle watch command
    fn handle_watch_command(&mut self, args: &str, dm: &mut DataManager) {
        let parts: Vec<&str> = args.split_whitespace().collect();
        if parts.is_empty() {
            self.add_output("Usage:");
            self.add_output("  watch add $<expr>   - Add watch expression");
            self.add_output("  watch remove <id>  - Remove watch expression");
            self.add_output("  watch list          - List all watch expressions");
            self.add_output("  watch clear         - Clear all watch expressions");
            return;
        }

        match parts[0] {
            "add" => {
                if parts.len() < 2 {
                    self.add_error("Usage: watch add $<expr>");
                    return;
                }
                let expr = args[3..].trim();
                if !expr.starts_with('$') {
                    self.add_error("Watch expression must start with '$'");
                    return;
                }
                match dm.watcher.add_expression(expr[1..].trim().to_string()) {
                    Some(id) => {
                        self.add_output(&format!("Added watch #{id}: {expr}"));
                    }
                    None => {
                        self.add_output(&format!("expression already being watched: {expr}"));
                    }
                }
            }
            "remove" => {
                if parts.len() != 2 {
                    self.add_error("Usage: watch remove <id>");
                    return;
                }
                match parts[1].parse::<usize>() {
                    Ok(id) => match dm.watcher.remove_expression(id) {
                        Some(expr) => {
                            self.add_output(&format!("Removed watch #{id}: {expr}"));
                        }
                        None => {
                            self.add_error(&format!("No watch found with id {id}"));
                        }
                    },
                    Err(_) => {
                        self.add_error("Invalid watch id");
                    }
                }
            }
            "list" => {
                if parts.len() != 1 {
                    self.add_error("Usage: watch list");
                    return;
                }
                if dm.watcher.count() == 0 {
                    self.add_output("No watch expressions set");
                } else {
                    self.add_output("Current watch expressions:");
                    for (id, expr) in dm.watcher.list_expressions() {
                        self.add_output(&format!("  #{id}: {expr}"));
                    }
                }
            }
            "clear" => {
                if parts.len() != 1 {
                    self.add_error("Usage: watch clear");
                } else {
                    dm.watcher.clear();
                    self.add_output("Clearing all watches...");
                }
            }
            _ => {
                self.add_error("Unknown watch command");
                self.add_output("Usage:");
                self.add_output("  watch add $<expr>    - Add watch expression");
                self.add_output("  watch remove <id>   - Remove watch expression");
                self.add_output("  watch list          - List all watch expressions");
                self.add_output("  watch clear         - Clear all watch expressions");
            }
        }
    }

    /// Handle theme switching command
    fn handle_theme_command(&mut self, theme_name: &str, dm: &mut DataManager) {
        let theme_name = theme_name.to_lowercase();
        let theme_name = theme_name.trim();

        if theme_name.is_empty() {
            self.show_themes(dm);
            return;
        }

        let theme = dm.theme.switch_theme(theme_name);
        match theme {
            Ok(_) => {
                self.add_system(&format!(
                    "Switched to '{theme_name}' theme - changes will apply immediately"
                ));
                // Theme changes take effect immediately via the shared ThemeManager
            }
            Err(e) => {
                self.add_error(&format!("Failed to switch theme: {e}"));
                self.add_output("Available themes:");
                let themes = dm.theme.list_themes();
                for (_name, display_name, description) in themes {
                    self.add_output(&format!("  {display_name} - {description}"));
                }
            }
        }
    }

    /// Handle cursor movement
    fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    fn move_cursor_right(&mut self) {
        if self.cursor_position < self.input_buffer.len() {
            self.cursor_position += 1;
        }
    }

    /// Navigate command history
    fn history_up(&mut self) {
        if self.command_history.is_empty() {
            return;
        }

        match self.history_position {
            None => {
                // Start browsing history from the end
                self.history_position = Some(self.command_history.len() - 1);
            }
            Some(pos) if pos > 0 => {
                self.history_position = Some(pos - 1);
            }
            _ => return, // Already at the beginning
        }

        if let Some(pos) = self.history_position
            && let Some(cmd) = self.command_history.get(pos) {
                self.input_buffer = cmd.clone();
                self.cursor_position = self.input_buffer.len();
            }
    }

    fn history_down(&mut self) {
        match self.history_position {
            Some(pos) if pos < self.command_history.len() - 1 => {
                self.history_position = Some(pos + 1);
                if let Some(cmd) = self.command_history.get(pos + 1) {
                    self.input_buffer = cmd.clone();
                    self.cursor_position = self.input_buffer.len();
                }
            }
            Some(_) => {
                // Go back to current input
                self.history_position = None;
                self.input_buffer.clear();
                self.cursor_position = 0;
            }
            None => {} // Not browsing history
        }
    }

    /// Render enhanced status line with comprehensive status information    
    fn render_status_line(&self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        // Build comprehensive status using StatusBar
        let connection_status = if self.connected {
            ConnectionStatus::Connected
        } else {
            ConnectionStatus::Disconnected
        };

        let execution_status = if let Some((current, total)) = self.snapshot_info {
            if current == 0 {
                ExecutionStatus::Start
            } else if current >= total.saturating_sub(1) {
                ExecutionStatus::End
            } else {
                ExecutionStatus::Running
            }
        } else {
            ExecutionStatus::Start
        };

        let mut status_bar = StatusBar::new()
            .connection(connection_status)
            .execution(execution_status)
            .current_panel("Terminal".to_string());

        // Add step info if available
        if let Some((current, total)) = self.snapshot_info {
            status_bar = status_bar.message(format!("Step {}/{}", current + 1, total + 1));
        }

        // Add gas info
        status_bar = status_bar.message("Gas: 2,847,293".to_string());

        let status_text = status_bar.build();

        let status_paragraph = Paragraph::new(Line::from(vec![Span::styled(
            status_text,
            Style::default().fg(dm.theme.info_color),
        )]))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(dm.theme.unfocused_border)),
        );

        frame.render_widget(status_paragraph, area);
    }

    /// Handle keys in INSERT mode (normal typing)
    fn handle_insert_mode_key(
        &mut self,
        event: KeyEvent,
        dm: &mut DataManager,
    ) -> Result<EventResponse> {
        match event.code {
            KeyCode::Enter => {
                let command = self.input_buffer.clone();
                self.input_buffer.clear();
                self.cursor_position = 0;
                self.history_position = None;
                let response = self.execute_command(command.trim_start(), dm)?;
                Ok(response)
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    self.input_buffer.remove(self.cursor_position - 1);
                    self.cursor_position -= 1;
                    self.history_position = None;
                }
                Ok(EventResponse::Handled)
            }
            KeyCode::Delete => {
                if self.cursor_position < self.input_buffer.len() {
                    self.input_buffer.remove(self.cursor_position);
                    self.history_position = None;
                }
                Ok(EventResponse::Handled)
            }
            KeyCode::Left => {
                self.move_cursor_left();
                Ok(EventResponse::Handled)
            }
            KeyCode::Right => {
                self.move_cursor_right();
                Ok(EventResponse::Handled)
            }
            KeyCode::Up => {
                self.history_up();
                Ok(EventResponse::Handled)
            }
            KeyCode::Down => {
                self.history_down();
                Ok(EventResponse::Handled)
            }
            KeyCode::Home => {
                self.cursor_position = 0;
                Ok(EventResponse::Handled)
            }
            KeyCode::End => {
                self.cursor_position = self.input_buffer.len();
                Ok(EventResponse::Handled)
            }
            KeyCode::Char(c) => {
                self.input_buffer.insert(self.cursor_position, c);
                self.cursor_position += 1;
                self.history_position = None;
                // Reset scroll to bottom when typing
                self.scroll_offset = 0;
                Ok(EventResponse::Handled)
            }
            _ => Ok(EventResponse::NotHandled),
        }
    }

    /// Handle keys in VIM mode (navigation)
    fn handle_vim_mode_key(
        &mut self,
        event: KeyEvent,
        _dm: &mut DataManager,
    ) -> Result<EventResponse> {
        match event.code {
            // Return to INSERT mode
            KeyCode::Char('i') | KeyCode::Enter => {
                // Update cursor position to bottom
                self.vim_goto_bottom();
                self.vim_number_prefix.clear();
                self.horizontal_offset = 0; // Reset horizontal scroll when leaving vim mode
                self.mode = TerminalMode::Insert;
                Ok(EventResponse::Handled)
            }

            // Vim navigation commands - both j/k and arrow keys
            KeyCode::Char('j') | KeyCode::Down => {
                self.vim_move_cursor_down(1);
                Ok(EventResponse::Handled)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.vim_move_cursor_up(1);
                Ok(EventResponse::Handled)
            }
            // Horizontal scrolling in vim mode (exactly like code panel)
            KeyCode::Char('h') | KeyCode::Left => {
                let count = if self.vim_number_prefix.is_empty() {
                    1
                } else {
                    self.vim_number_prefix.parse::<usize>().unwrap_or(1)
                };
                if self.horizontal_offset > 0 {
                    self.horizontal_offset = self.horizontal_offset.saturating_sub(count * 5);
                }
                self.vim_number_prefix.clear();
                Ok(EventResponse::Handled)
            }
            KeyCode::Char('l') | KeyCode::Right => {
                let count = if self.vim_number_prefix.is_empty() {
                    1
                } else {
                    self.vim_number_prefix.parse::<usize>().unwrap_or(1)
                };
                if self.max_line_width > self.content_width {
                    let max_scroll = self.max_line_width.saturating_sub(self.content_width);
                    if self.horizontal_offset < max_scroll {
                        self.horizontal_offset =
                            (self.horizontal_offset + count * 5).min(max_scroll);
                    }
                }
                self.vim_number_prefix.clear();
                Ok(EventResponse::Handled)
            }

            // Handle number prefixes for multi-line scrolling
            KeyCode::Char(c) if c.is_ascii_digit() => {
                self.vim_number_prefix.push(c);
                Ok(EventResponse::Handled)
            }

            // Go to top/bottom
            KeyCode::Char('g') => {
                // Handle 'gg' sequence
                if self.vim_number_prefix == "g" {
                    self.vim_goto_top();
                    self.vim_number_prefix.clear();
                } else {
                    self.vim_number_prefix = String::from("g");
                }
                Ok(EventResponse::Handled)
            }
            KeyCode::Char('G') => {
                self.vim_goto_bottom();
                self.vim_number_prefix.clear();
                Ok(EventResponse::Handled)
            }

            // Jump to previous blank line
            KeyCode::Char('{') => {
                let count = if self.vim_number_prefix.is_empty() {
                    1
                } else {
                    self.vim_number_prefix.parse::<usize>().unwrap_or(1)
                };
                self.vim_prev_block(count);
                self.vim_number_prefix.clear();
                Ok(EventResponse::Handled)
            }

            // Jump to next blank line
            KeyCode::Char('}') => {
                let count = if self.vim_number_prefix.is_empty() {
                    1
                } else {
                    self.vim_number_prefix.parse::<usize>().unwrap_or(1)
                };
                self.vim_next_block(count);
                self.vim_number_prefix.clear();
                Ok(EventResponse::Handled)
            }

            _ => {
                // Clear number prefix on unrecognized key
                self.vim_number_prefix.clear();
                Ok(EventResponse::NotHandled)
            }
        }
    }

    /// Move VIM cursor up (k key) with auto-scrolling (exactly like code panel)
    fn vim_move_cursor_up(&mut self, count: usize) {
        let multiplier = if self.vim_number_prefix.is_empty() {
            count
        } else {
            self.vim_number_prefix.parse::<usize>().unwrap_or(1) * count
        };

        // Move cursor up in absolute terminal history (1-based like code panel)
        if self.vim_cursor_line > multiplier {
            self.vim_cursor_line -= multiplier;
        } else {
            self.vim_cursor_line = 1; // Can't go above first line
        }

        // Auto-scroll if cursor moves out of view (EXACTLY like code panel logic)
        if self.vim_cursor_line < self.scroll_offset + 1 {
            self.scroll_offset = (self.vim_cursor_line - 1).max(0);
        }

        self.vim_number_prefix.clear();
    }

    /// Move VIM cursor down (j key) with auto-scrolling (exactly like code panel)
    fn vim_move_cursor_down(&mut self, count: usize) {
        let multiplier = if self.vim_number_prefix.is_empty() {
            count
        } else {
            self.vim_number_prefix.parse::<usize>().unwrap_or(1) * count
        };

        let max_lines = self.lines.len();
        if max_lines == 0 {
            self.vim_number_prefix.clear();
            return;
        }

        // Move cursor down in absolute terminal history (considering input buffer)
        let new_line = (self.vim_cursor_line + multiplier).min(max_lines + 1);
        self.vim_cursor_line = new_line;

        // Auto-scroll if cursor moves out of view
        let viewport_height = self.content_height;
        if self.vim_cursor_line > self.scroll_offset + viewport_height {
            self.scroll_offset = self.vim_cursor_line.saturating_sub(viewport_height);
        }

        self.vim_number_prefix.clear();
    }

    /// Go to top of terminal
    fn vim_goto_top(&mut self) {
        self.vim_cursor_line = 1; // First line (1-based)
        self.scroll_offset = 0; // Show from beginning (like code panel)
    }

    /// Go to bottom of terminal
    fn vim_goto_bottom(&mut self) {
        if !self.lines.is_empty() {
            self.vim_cursor_line = self.lines.len() + 1; // Include prompt line

            let max_lines = self.lines.len() + 1;

            if max_lines <= self.content_height {
                self.scroll_offset = 0; // No scroll needed if content fits
            } else {
                // Scroll to show last line at bottom
                // This is like code panel logic where we show the last lines
                self.scroll_offset = max_lines.saturating_sub(self.content_height);
            }
        }
    }

    /// Find previous block (VIM { command)
    fn vim_prev_block(&mut self, count: usize) {
        if self.lines.is_empty() {
            return;
        }

        let mut blank_lines_found = 0;
        let mut target_line = 1; // Default to first line

        // Search backwards from current position
        for line_num in (1..self.vim_cursor_line).rev() {
            if line_num <= self.lines.len() {
                let line_idx = line_num.saturating_sub(1); // Convert to 0-based index
                if self.lines[line_idx].content.trim().is_empty()
                    || self.lines[line_idx].line_type == LineType::Command
                {
                    blank_lines_found += 1;
                    if blank_lines_found == count {
                        target_line = line_num.max(1);
                        break;
                    }
                }
            }
        }

        // Move to the target line
        self.vim_cursor_line = target_line;

        // Auto-scroll if necessary
        if self.vim_cursor_line < self.scroll_offset + 1 {
            self.scroll_offset = self.vim_cursor_line.saturating_sub(1);
        }
    }

    /// Find next block (VIM } command)
    fn vim_next_block(&mut self, count: usize) {
        if self.lines.is_empty() {
            return;
        }

        let mut blank_lines_found = 0;
        let max_lines = self.lines.len() + 1; // Include prompt line
        let mut target_line = max_lines; // Default to last line

        // Search forward from current position
        for line_num in (self.vim_cursor_line + 1)..=self.lines.len() {
            let line_idx = line_num - 1; // Convert to 0-based index
            if self.lines[line_idx].content.trim().is_empty()
                || self.lines[line_idx].line_type == LineType::Command
            {
                blank_lines_found += 1;
                if blank_lines_found == count {
                    target_line = line_num.min(max_lines);
                    break;
                }
            }
        }

        // Move to the target line
        self.vim_cursor_line = target_line;

        // Auto-scroll if necessary
        let viewport_height = self.content_height;
        if self.vim_cursor_line > self.scroll_offset + viewport_height {
            self.scroll_offset = self.vim_cursor_line.saturating_sub(viewport_height);
        }
    }

    /// Apply syntax highlighting to a terminal line based on its content
    fn highlight_terminal_line<'a>(
        &self,
        content: &'a str,
        line_type: &LineType,
        dm: &DataManager,
    ) -> Line<'a> {
        use ratatui::text::{Line, Span};

        match line_type {
            LineType::Command => {
                // Only highlight commands that start with $ (expression evaluation)
                if content.starts_with("> $") {
                    // Extract the expression part after "> $"
                    let expr_start = content.find('$').unwrap_or(0) + 1;
                    let expression = &content[expr_start..];

                    // Tokenize the expression as Solidity
                    let tokens = self.syntax_highlighter.tokenize(expression, SyntaxType::Solidity);

                    let mut spans = Vec::new();
                    // Add the prompt prefix without highlighting
                    spans.push(Span::styled(
                        &content[..expr_start],
                        Style::default().fg(dm.theme.info_color),
                    ));

                    let mut last_end = 0;

                    // Apply syntax highlighting to the expression tokens
                    for token in tokens {
                        // Add any unhighlighted text before this token
                        if token.start > last_end {
                            let unhighlighted = &expression[last_end..token.start];
                            if !unhighlighted.is_empty() {
                                spans.push(Span::raw(unhighlighted));
                            }
                        }

                        // Add the highlighted token
                        let token_text = &expression[token.start..token.end];
                        let token_style =
                            self.syntax_highlighter.get_token_style(token.token_type, &dm.theme);
                        spans.push(Span::styled(token_text, token_style));

                        last_end = token.end;
                    }

                    // Add any remaining unhighlighted text
                    if last_end < expression.len() {
                        let remaining = &expression[last_end..];
                        if !remaining.is_empty() {
                            spans.push(Span::raw(remaining));
                        }
                    }

                    Line::from(spans)
                } else {
                    // Regular commands without highlighting
                    Line::from(Span::styled(content, Style::default().fg(dm.theme.info_color)))
                }
            }
            LineType::Output => {
                // Determine syntax type for output based on content patterns
                let syntax_type = if content.contains("0x")
                    && (content.contains("PUSH")
                        || content.contains("POP")
                        || content.contains("ADD")
                        || content.contains("SUB"))
                {
                    // Likely opcodes
                    SyntaxType::Opcodes
                } else {
                    // Default to Solidity for general output
                    SyntaxType::Solidity
                };

                // Tokenize the line
                let tokens = self.syntax_highlighter.tokenize(content, syntax_type);

                let mut spans = Vec::new();
                let mut last_end = 0;

                // Apply syntax highlighting to tokens
                for token in tokens {
                    // Add any unhighlighted text before this token
                    if token.start > last_end {
                        let unhighlighted = &content[last_end..token.start];
                        if !unhighlighted.is_empty() {
                            spans.push(Span::raw(unhighlighted));
                        }
                    }

                    // Add the highlighted token
                    let token_text = &content[token.start..token.end];
                    let token_style =
                        self.syntax_highlighter.get_token_style(token.token_type, &dm.theme);
                    spans.push(Span::styled(token_text, token_style));

                    last_end = token.end;
                }

                // Add any remaining unhighlighted text
                if last_end < content.len() {
                    let remaining = &content[last_end..];
                    if !remaining.is_empty() {
                        spans.push(Span::raw(remaining));
                    }
                }

                // If no tokens were found, return the original content
                if spans.is_empty() {
                    Line::from(content)
                } else {
                    Line::from(spans)
                }
            }
            LineType::Error => {
                // Error lines use error color without syntax highlighting
                Line::from(Span::styled(content, Style::default().fg(dm.theme.error_color)))
            }
            LineType::System => {
                // System lines use success color without syntax highlighting
                Line::from(Span::styled(content, Style::default().fg(dm.theme.success_color)))
            }
        }
    }

    /// Render the unified bash-like terminal view
    fn render_unified_terminal(&mut self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        // Update pending command
        if self.pending_command.as_ref().is_some_and(|cmd| !cmd.is_pending(dm)) {
            self.spinner.finish_loading();
            let pending_command = self.pending_command.take().unwrap();
            match pending_command.output_finished(dm) {
                Ok(output) => {
                    for line in output.lines() {
                        self.add_output(line);
                    }
                }
                Err(e) => {
                    let error_msg = format!("{e}");
                    for line in error_msg.lines() {
                        self.add_error(line);
                    }
                }
            }
        }

        // Start with all terminal history
        let mut all_content = self.lines.clone();

        if self.pending_command.is_some() {
            // We still have pending command
            self.input_buffer.clear();
            self.spinner.tick();
            all_content.push(TerminalLine {
                content: self.spinner.display_text(),
                line_type: LineType::System,
            });
        } else {
            // Add current input line
            let prompt = format!(
                "{} edb{} {}",
                if self.connected { Icons::CONNECTED } else { Icons::DISCONNECTED },
                Icons::ARROW_RIGHT,
                self.input_buffer
            );
            all_content.push(TerminalLine { content: prompt, line_type: LineType::Command });
        }

        // Calculate visible area (leave space for status and help text if needed)
        let status_help_height = if self.focused && area.height > 10 { 2 } else { 0 };
        self.content_height = area.height.saturating_sub(2 + status_help_height) as usize; // Account for borders + status/help
        self.content_width = area.width.saturating_sub(2) as usize; // Account for borders

        // Calculate max line width for vim mode
        if self.mode == TerminalMode::Vim {
            self.calculate_max_line_width();
        }

        // In INSERT mode, always stay at bottom to show most recent content
        // In VIM mode, respect user's scroll position
        let total_content = all_content.len();
        let (start_idx, end_idx) = if self.mode == TerminalMode::Insert {
            // INSERT mode: always show the most recent content (bottom)
            if total_content <= self.content_height {
                (0, total_content)
            } else {
                let start = total_content - self.content_height;
                (start, total_content)
            }
        } else {
            // VIM mode: respect scroll_offset (like code panel)
            let start = self.scroll_offset;
            let end = (start + self.content_height).min(total_content);
            (start, end)
        };

        // Create unified terminal content items with VIM cursor support
        let terminal_items: Vec<ListItem<'_>> = all_content
            .iter()
            .skip(start_idx)
            .take(self.content_height)
            .enumerate()
            .map(|(display_row, terminal_line)| {
                let base_style = match terminal_line.line_type {
                    LineType::Command => Style::default().fg(dm.theme.info_color),
                    LineType::Output => Style::default(),
                    LineType::Error => Style::default().fg(dm.theme.error_color),
                    LineType::System => Style::default().fg(dm.theme.success_color),
                };

                // Create the line content with syntax highlighting
                let styled_line = if terminal_line.line_type == LineType::Command
                    && display_row == all_content.len().saturating_sub(1) - start_idx  // Last line (input line)
                    && self.focused
                    && self.mode == TerminalMode::Insert
                {
                    // This is the input line - apply block cursor overlay
                    let content = &terminal_line.content;
                    let mut spans = Vec::new();

                    // Find where the cursor should be in the displayed prompt
                    // The prompt format is: "{icon} edb{arrow} {input}"
                    // We need to find where the input starts
                    let prompt_prefix = if self.connected {
                        format!("{} edb{} ", Icons::CONNECTED, Icons::ARROW_RIGHT)
                    } else {
                        format!("{} edb{} ", Icons::DISCONNECTED, Icons::ARROW_RIGHT)
                    };
                    let prefix_len = prompt_prefix.chars().count();

                    // Convert content to chars for proper indexing
                    let chars: Vec<char> = content.chars().collect();
                    let cursor_pos_in_line = prefix_len + self.cursor_position;

                    // Build the line with cursor overlay
                    for (i, ch) in chars.iter().enumerate() {
                        if i == cursor_pos_in_line {
                            // This is where the cursor should be - show as block
                            spans.push(Span::styled(
                                ch.to_string(),
                                Style::default().bg(dm.theme.cursor_color).fg(dm.theme.panel_bg), // Invert colors for block cursor
                            ));
                        } else if i == chars.len() - 1 && cursor_pos_in_line >= chars.len() {
                            // Cursor at end of line - add the character normally then add block
                            spans.push(Span::styled(ch.to_string(), base_style));
                            spans.push(Span::styled(
                                " ", // Block cursor on empty space at end
                                Style::default().bg(dm.theme.cursor_color).fg(dm.theme.panel_bg),
                            ));
                        } else {
                            // Normal character
                            spans.push(Span::styled(ch.to_string(), base_style));
                        }
                    }

                    Line::from(spans)
                } else {
                    // Use syntax highlighting for non-input lines
                    self.highlight_terminal_line(
                        &terminal_line.content,
                        &terminal_line.line_type,
                        dm,
                    )
                };

                // Apply horizontal scrolling in vim mode (properly handle styled lines)
                let scrolled_line = if self.mode == TerminalMode::Vim && self.horizontal_offset > 0
                {
                    // Apply horizontal offset to styled line (similar to code panel)
                    self.apply_horizontal_offset_to_line(styled_line)
                } else {
                    styled_line
                };

                // Apply full-width highlighting to the ListItem if this is the current VIM cursor line
                let list_item = ListItem::new(scrolled_line);

                // Convert absolute vim_cursor_line to display row (EXACTLY like code panel)
                let cursor_display_row =
                    if self.vim_cursor_line > 0 && self.mode == TerminalMode::Vim {
                        // Convert to 0-based
                        let cursor_absolute_idx = self.vim_cursor_line - 1;
                        // Check if cursor is within visible range
                        if cursor_absolute_idx >= start_idx && cursor_absolute_idx < end_idx {
                            Some(cursor_absolute_idx - start_idx) // Display row within visible content
                        } else {
                            None // Cursor not in visible area
                        }
                    } else {
                        None
                    };

                if self.mode == TerminalMode::Vim
                    && self.focused
                    && Some(display_row) == cursor_display_row
                {
                    // Apply highlighting to entire ListItem (full width like code panel)
                    list_item
                        .style(Style::default().bg(dm.theme.highlight_bg).fg(dm.theme.highlight_fg))
                } else {
                    list_item
                }
            })
            .collect();

        // Show scroll indicator and mode in title (only show scroll indicator in VIM mode)
        let scroll_indicator = if self.mode == TerminalMode::Vim && self.scroll_offset > 0 {
            format!(" [↑{}]", self.scroll_offset)
        } else {
            String::new()
        };

        let mode_indicator = match self.mode {
            TerminalMode::Insert => "INSERT",
            TerminalMode::Vim => "VIM",
        };

        let terminal_title = format!(
            "{} Debug Terminal{} [{}]",
            Icons::PROCESSING,
            scroll_indicator,
            mode_indicator
        );

        let terminal_block = BorderPresets::terminal(
            self.focused,
            terminal_title,
            dm.theme.focused_border,
            dm.theme.unfocused_border,
        );

        // Use the full area since status/help are handled separately
        let main_area = area;

        let terminal_list = List::new(terminal_items).block(terminal_block);
        frame.render_widget(terminal_list, main_area);

        // Add status and help text inside the terminal border like other panels
        if self.focused && area.height > 10 {
            // Status line
            let status_area = Rect {
                x: area.x + 1,
                y: area.y + area.height - 3,
                width: area.width - 2,
                height: 1,
            };

            let status_bar = StatusBar::new()
                .current_panel("Terminal".to_string())
                .message(format!("Mode: {:?}", self.mode));

            let status_text = status_bar.build();

            // Add horizontal scroll indicator if content is scrollable (vim mode only)
            let final_status_text = if self.mode == TerminalMode::Vim
                && self.max_line_width > self.content_width
            {
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

            // Help text
            let help_area = Rect {
                x: area.x + 1,
                y: area.y + area.height - 2,
                width: area.width - 2,
                height: 1,
            };

            let help_text = match self.mode {
                TerminalMode::Insert => {
                    "INSERT mode: Type commands • ↑/↓: History • Esc: VIM mode • ?: Help"
                        .to_string()
                }
                TerminalMode::Vim => {
                    "VIM mode: Vim-like Navigation • i/Enter: INSERT • ?: Help".to_string()
                }
            };

            let help_paragraph =
                Paragraph::new(help_text).style(Style::default().fg(dm.theme.help_text_color));
            frame.render_widget(help_paragraph, help_area);
        }
    }
}

impl PanelTr for TerminalPanel {
    fn panel_type(&self) -> PanelType {
        PanelType::Terminal
    }

    fn title(&self, _dm: &mut DataManager) -> String {
        let status = if let Some((current, total)) = self.snapshot_info {
            format!(" [{current}/{total}]")
        } else {
            String::new()
        };

        let mode_info = match self.mode {
            TerminalMode::Insert => " - INSERT mode",
            TerminalMode::Vim => " - VIM mode",
        };

        format!("{} Debug Terminal{}{}", Icons::PROCESSING, status, mode_info)
    }

    fn render(&mut self, frame: &mut Frame<'_>, area: Rect, dm: &mut DataManager) {
        // Add status line if there's enough space
        let (main_area, status_area) = if area.height > 8 {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Status line
                    Constraint::Min(7),    // Main content
                ])
                .split(area);
            (chunks[1], Some(chunks[0]))
        } else {
            (area, None)
        };

        // Render status line if available
        if let Some(status_rect) = status_area {
            self.render_status_line(frame, status_rect, dm);
        }

        // Unified terminal rendering (bash-style)
        self.render_unified_terminal(frame, main_area, dm);
    }

    fn handle_key_event(&mut self, event: KeyEvent, dm: &mut DataManager) -> Result<EventResponse> {
        if !self.focused || event.kind != KeyEventKind::Press {
            return Ok(EventResponse::NotHandled);
        }

        debug!("Terminal panel received key event: {:?} in mode {:?}", event, self.mode);

        // Global key handlers (work in both modes)
        match event.code {
            // Esc: Switch from INSERT to VIM mode
            KeyCode::Esc => {
                if self.mode == TerminalMode::Insert {
                    self.vim_goto_bottom();
                    self.vim_number_prefix.clear();
                    self.mode = TerminalMode::Vim;
                    return Ok(EventResponse::Handled);
                }
                // In VIM mode, Esc does nothing (already in VIM mode)
                return Ok(EventResponse::Handled);
            }
            // Ctrl+[ is an alternative to Esc
            KeyCode::Char('[') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.mode == TerminalMode::Insert {
                    self.vim_goto_bottom();
                    self.vim_number_prefix.clear();
                    self.mode = TerminalMode::Vim;
                    return Ok(EventResponse::Handled);
                }
                // In VIM mode, Esc does nothing (already in VIM mode)
                return Ok(EventResponse::Handled);
            }

            // Ctrl+C double-press for exit (works in both modes)
            KeyCode::Char('c') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                let now = Instant::now();
                if let Some(last_time) = self.last_ctrl_c
                    && now.duration_since(last_time).as_secs() < 2 {
                        self.add_system("🚪 Exiting debugger (Ctrl+C double-press)...");
                        return Ok(EventResponse::Exit);
                    }
                self.last_ctrl_c = Some(now);
                if self.mode == TerminalMode::Insert {
                    self.input_buffer.clear();
                    self.cursor_position = 0;
                    self.history_position = None;
                    self.add_system("^C (press again quickly to exit)");
                } else {
                    self.add_system("^C (press again quickly to exit)");
                }
                return Ok(EventResponse::Handled);
            }

            // Ctrl+L to clear terminal (works in both modes)
            KeyCode::Char('l') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.lines.clear();
                self.add_system("Terminal cleared");
                return Ok(EventResponse::Handled);
            }

            _ => {}
        }

        // Mode-specific key handlers
        match self.mode {
            TerminalMode::Insert => self.handle_insert_mode_key(event, dm),
            TerminalMode::Vim => self.handle_vim_mode_key(event, dm),
        }
    }

    fn handle_mouse_event(
        &mut self,
        event: crossterm::event::MouseEvent,
        _data_manager: &mut DataManager,
    ) -> Result<EventResponse> {
        use crossterm::event::MouseEventKind;

        match event.kind {
            MouseEventKind::ScrollUp => {
                self.mode = TerminalMode::Vim;
                self.vim_move_cursor_up(1);
                Ok(EventResponse::Handled)
            }
            MouseEventKind::ScrollDown => {
                self.mode = TerminalMode::Vim;
                self.vim_move_cursor_down(1);
                Ok(EventResponse::Handled)
            }
            _ => Ok(EventResponse::NotHandled),
        }
    }

    fn on_focus(&mut self) {
        self.focused = true;
        debug!("Terminal panel gained focus");
    }

    fn on_blur(&mut self) {
        self.focused = false;
        debug!("Terminal panel lost focus");
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
