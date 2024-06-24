//! Debugger context and event handler implementation.

use alloy_primitives::Address;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use edb_debug_backend::artifact::debug::{DebugArtifact, DebugNodeFlat, DebugStep};
use eyre::Result;
use ratatui::layout::{Direction, Rect};
use revm_inspectors::tracing::types::CallKind;
use serde::de;
use std::ops::ControlFlow;

use crate::{
    core::ExitReason,
    window::{PaneView, TerminalMode, VirtCoord, Window},
};

/// This is currently used to remember last scroll position so screen doesn't wiggle as much.
#[derive(Default)]
pub struct DrawMemory {
    pub inner_call_index: usize,
    pub current_buf_startline: usize,
    pub current_stack_startline: usize,
}

#[derive(Debug)]
pub struct RecoverableError {
    pub message: String,
}

impl RecoverableError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl std::fmt::Display for RecoverableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}\n\nPress the ESC key to close the pop-up window.", self.message)
    }
}

impl std::error::Error for RecoverableError {}

pub struct FrontendContext<'a> {
    pub artifact: &'a mut DebugArtifact,

    /// Buffer for keys prior to execution, i.e. '10' + 'k' => move up 10 operations.
    pub key_buffer: String,
    /// Current step in the debug steps.
    pub current_step: usize,
    pub draw_memory: DrawMemory,
    pub opcode_list: Vec<String>,
    pub last_index: usize,

    pub stack_labels: bool,
    /// Whether to decode active buffer as utf8 or not.
    pub buf_utf: bool,
    pub show_shortcuts: bool,

    /// The display window (which is only aware of the layout,
    /// without any actual data)
    pub window: Window<'a>,
}

impl<'a> FrontendContext<'a> {
    pub(crate) fn new(artifact: &'a mut DebugArtifact) -> Result<Self> {
        Ok(FrontendContext {
            artifact,

            key_buffer: String::with_capacity(64),
            current_step: 0,
            draw_memory: DrawMemory::default(),
            opcode_list: Vec::new(),
            last_index: 0,

            stack_labels: false,
            buf_utf: false,
            show_shortcuts: true,

            window: Window::new()?,
        })
    }

    pub(crate) fn init(&mut self) {
        self.gen_opcode_list();
    }

    pub(crate) fn debug_arena(&self) -> &[DebugNodeFlat] {
        &self.artifact.debug_arena
    }

    pub(crate) fn debug_call(&self) -> &DebugNodeFlat {
        &self.debug_arena()[self.draw_memory.inner_call_index]
    }

    /// Returns the current call address.
    pub(crate) fn address(&self) -> &Address {
        &self.debug_call().address
    }

    /// Returns the current call kind.
    pub(crate) fn call_kind(&self) -> CallKind {
        self.debug_call().kind
    }

    /// Returns the current debug steps.
    pub(crate) fn debug_steps(&self) -> &[DebugStep] {
        &self.debug_call().steps
    }

    /// Returns the current debug step.
    pub(crate) fn current_step(&self) -> &DebugStep {
        &self.debug_steps()[self.current_step]
    }

    fn gen_opcode_list(&mut self) {
        self.opcode_list.clear();
        let debug_steps = &self.artifact.debug_arena[self.draw_memory.inner_call_index].steps;
        self.opcode_list.extend(debug_steps.iter().map(DebugStep::pretty_opcode));
    }

    fn gen_opcode_list_if_necessary(&mut self) {
        if self.last_index != self.draw_memory.inner_call_index {
            self.gen_opcode_list();
            self.last_index = self.draw_memory.inner_call_index;
        }
    }

    fn data_pane_height(&self) -> usize {
        // PaneView::Memory => self.current_step().memory.len() / 32,
        // PaneView::Calldata => self.current_step().calldata.len() / 32,
        // PaneView::Returndata => self.current_step().returndata.len() / 32,
        // PaneView::Stack => self.current_step().stack.len(),
        // PaneView::Expression | PaneView::Variable => todo!(),
        unimplemented!()
    }
}

impl FrontendContext<'_> {
    pub(crate) fn handle_event(&mut self, event: Event) -> ControlFlow<ExitReason> {
        let ret = match event {
            Event::Key(event) => self.handle_key_event(event),
            Event::Mouse(event) => self.handle_mouse_event(event),
            _ => ControlFlow::Continue(()),
        };
        // Generate the list after the event has been handled.
        self.gen_opcode_list_if_necessary();
        ret
    }

    fn handle_key_event(&mut self, event: KeyEvent) -> ControlFlow<ExitReason> {
        match self.try_handle_key_event(event) {
            Ok(control_flow) => control_flow,
            Err(report) => {
                if let Some(e) = report.downcast_ref::<RecoverableError>() {
                    self.window.pop_error_message(e.to_string());
                    ControlFlow::Continue(())
                } else {
                    panic!("{:?}", report);
                }
            }
        }
    }

    fn try_handle_key_event(&mut self, event: KeyEvent) -> Result<ControlFlow<ExitReason>> {
        // // Breakpoints
        // if let KeyCode::Char(c) = event.code {
        //     if c.is_alphabetic() && self.key_buffer.starts_with('\'') {
        //         self.handle_breakpoint(c);
        //         return ControlFlow::Continue(());
        //     }
        // }

        let shift = event.modifiers.contains(KeyModifiers::SHIFT);
        let control = event.modifiers.contains(KeyModifiers::CONTROL);
        let screen_size = self.window.screen_size;

        let focused_pane = self.window.get_focused_view()?;
        if self.window.has_popup() {
            if event.code == KeyCode::Esc {
                self.window.exit_popup();
            } else {
                self.window.handle_key_event_in_popup(event)?;
            }
        } else if focused_pane == PaneView::Terminal &&
            self.window.editor_mode == TerminalMode::Insert
        {
            // Insert mode is a special case
            if event.code == KeyCode::Esc {
                self.window.set_editor_normal_mode();
            } else {
                self.window.handle_input(event);
            }
        } else {
            // Handle common key events
            match event.code {
                // Scale the focused pane on the left side
                KeyCode::Left if shift && control => self.window.scale_left(1, screen_size)?,
                // Scale the focused pane on the right side
                KeyCode::Right if shift && control => self.window.scale_right(1, screen_size)?,
                // Scale the focused pane on the bottom side
                KeyCode::Down if shift && control => self.window.scale_down(1, screen_size)?,
                // Scale the focused pane on the top side
                KeyCode::Up if shift && control => self.window.scale_up(1, screen_size)?,

                // Move focus to the left pane
                KeyCode::Left if shift => self.window.focus_left()?,
                // Move focus to the right pane
                KeyCode::Right if shift => self.window.focus_right()?,
                // Move focus to the down pane
                KeyCode::Down if shift => self.window.focus_down()?,
                // Move focus to the up pane
                KeyCode::Up if shift => self.window.focus_up()?,

                // Pop up the assignment window
                KeyCode::Char('C') if shift => self.window.pop_assignment(),

                // Shortcut to enter the terminal
                KeyCode::Char('i') => {
                    // We do not want to exit the full screen mode when we are in
                    // the terminal, so we do not change screen
                    if focused_pane != PaneView::Terminal {
                        self.window.enter_terminal()?;
                    }
                    self.window.set_editor_insert_mode();
                }

                // Esc
                KeyCode::Esc if self.window.full_screen => self.window.toggle_full_screen(),

                // Enter
                KeyCode::Enter if !self.window.full_screen => self.window.toggle_full_screen(),

                // Cycle left the current focused pane
                KeyCode::Left if focused_pane != PaneView::Terminal => self.repeat(|this| {
                    this.window.get_focused_pane_mut()?.prev_view();
                    Ok(())
                })?,

                // Cycle right the current focused pane
                KeyCode::Right if focused_pane != PaneView::Terminal => self.repeat(|this| {
                    this.window.get_focused_pane_mut()?.next_view();
                    Ok(())
                })?,

                // Quit
                KeyCode::Char('Q') if shift => return Ok(ControlFlow::Break(ExitReason::CharExit)),

                // Shortcut to split the screen: (s)plit and (d)ivide
                KeyCode::Char('D') if shift => {
                    self.window.split_focused_pane(Direction::Vertical, [1, 1])?
                }
                KeyCode::Char('S') if shift => {
                    self.window.split_focused_pane(Direction::Horizontal, [1, 1])?
                }

                // Shortcut to unregister the current view
                KeyCode::Char('X') if shift => {
                    let view = self.window.get_focused_view()?;

                    if view.is_valid() {
                        self.window.get_pane_manager_mut()?.unassign(view)?;
                    }

                    // try to merge the pane if it is empty
                    if !self.window.get_focused_view()?.is_valid() && !self.window.full_screen {
                        if self.window.get_pane_manager()?.pane_num() == 1 {
                            return Err(RecoverableError::new("Cannot close the last pane.").into());
                        }
                        self.window.close_focused_pane()?;
                    }
                }

                // Other view-specific key events
                _ => match focused_pane {
                    PaneView::Terminal => self.window.handle_input(event),
                    PaneView::Source => self.handle_key_event_in_source(event),
                    PaneView::Trace => self.handle_key_event_in_trace(event),
                    PaneView::Opcode => self.handle_key_event_in_opcode(event),
                    _ => self.handle_key_even_in_data(event),
                },
                // // Scroll up the memory buffer
                // KeyCode::Char('k') | KeyCode::Up if control => self.repeat(|this| {
                //     this.draw_memory.current_buf_startline =
                //         this.draw_memory.current_buf_startline.saturating_sub(1);
                // }),
                // // Scroll down the memory buffer
                // // KeyCode::Char('j') | KeyCode::Down if control =>
                // self.repeat(|this| { //     let max_buf =
                // this.data_pane_height().saturating_sub(1);
                // //     if this.draw_memory.current_buf_startline < max_buf {
                // //         this.draw_memory.current_buf_startline += 1;
                // //     }
                // // }),

                // // Move up
                // KeyCode::Char('k') | KeyCode::Up => self.repeat(Self::step_back),
                // // Move down
                // KeyCode::Char('j') | KeyCode::Down => self.repeat(Self::step),

                // // Go to top of file
                // KeyCode::Char('g') => {
                //     self.draw_memory.inner_call_index = 0;
                //     self.current_step = 0;
                // }

                // // Go to bottom of file
                // KeyCode::Char('G') => {
                //     self.draw_memory.inner_call_index = self.debug_arena().len() - 1;
                //     self.current_step = self.n_steps() - 1;
                // }

                // // Go to previous call
                // KeyCode::Char('c') => {
                //     self.draw_memory.inner_call_index =
                //         self.draw_memory.inner_call_index.saturating_sub(1);
                //     self.current_step = self.n_steps() - 1;
                // }

                // // Go to next call
                // KeyCode::Char('C') => {
                //     if self.debug_arena().len() > self.draw_memory.inner_call_index +
                // 1     {
                //         self.draw_memory.inner_call_index += 1;
                //         self.current_step = 0;
                //     }
                // }

                // // Step forward
                // KeyCode::Char('s') => self.repeat(|this| {
                //     let remaining_ops = &this.opcode_list[this.current_step..];
                //     if let Some((i, _)) =
                //         remaining_ops.iter().enumerate().skip(1).find(|&(i, op)| {
                //             let prev = &remaining_ops[i - 1];
                //             let prev_is_jump =
                //                 prev.contains("JUMP") && prev != "JUMPDEST";
                //             let is_jumpdest = op == "JUMPDEST";
                //             prev_is_jump && is_jumpdest
                //         })
                //     {
                //         this.current_step += i;
                //     }
                // }),

                // // Step backwards
                // KeyCode::Char('a') => self.repeat(|this| {
                //     let ops = &this.opcode_list[..this.current_step];
                //     this.current_step = ops
                //         .iter()
                //         .enumerate()
                //         .skip(1)
                //         .rev()
                //         .find(|&(i, op)| {
                //             let prev = &ops[i - 1];
                //             let prev_is_jump =
                //                 prev.contains("JUMP") && prev != "JUMPDEST";
                //             let is_jumpdest = op == "JUMPDEST";
                //             prev_is_jump && is_jumpdest
                //         })
                //         .map(|(i, _)| i)
                //         .unwrap_or_default();
                // }),

                // // Toggle stack labels
                // KeyCode::Char('t') => self.stack_labels = !self.stack_labels,

                // // Toggle memory UTF-8 decoding
                // KeyCode::Char('m') => self.buf_utf = !self.buf_utf,

                // // Toggle help notice
                // KeyCode::Char('H') => self.show_shortcuts = !self.show_shortcuts,

                // // Numbers for repeating commands or breakpoints
                // KeyCode::Char(
                //     other @ ('0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' |
                //     '9' | '\''),
                // ) => {
                //     // Early return to not clear the buffer.
                //     self.key_buffer.push(other);
                //     return ControlFlow::Continue(());
                // }

                // // Unknown/unhandled key code
                // _ => {}
            }
        };

        self.key_buffer.clear();
        Ok(ControlFlow::Continue(()))
    }

    fn handle_breakpoint(&mut self, _c: char) {
        // // Find the location of the called breakpoint in the whole debug arena (at this address
        // with // this pc)
        // if let Some((caller, pc)) = self.debugger.breakpoints.get(&c) {
        //     for (i, node) in self.debug_arena().iter().enumerate() {
        //         if node.address == *caller {
        //             if let Some(step) = node.steps.iter().position(|step| step.pc == *pc) {
        //                 self.draw_memory.inner_call_index = i;
        //                 self.current_step = step;
        //                 break;
        //             }
        //         }
        //     }
        // }
        // self.key_buffer.clear();
    }

    fn handle_mouse_event(&mut self, event: MouseEvent) -> ControlFlow<ExitReason> {
        self.try_handle_mouse_event(event).unwrap()
    }

    fn try_handle_mouse_event(&mut self, event: MouseEvent) -> Result<ControlFlow<ExitReason>> {
        if self.window.has_popup() {
            return Ok(ControlFlow::Continue(()));
        }

        match event.kind {
            MouseEventKind::ScrollUp => self.window.get_focused_pane_mut()?.prev_view(),
            MouseEventKind::ScrollDown => self.window.get_focused_pane_mut()?.next_view(),
            MouseEventKind::Down(MouseButton::Left) => {
                if !self.window.full_screen {
                    let v_point =
                        VirtCoord::project(event.column, event.row, self.window.screen_size);
                    self.window.get_pane_manager_mut().unwrap().force_goto(v_point);
                }
            }
            _ => {}
        }

        Ok(ControlFlow::Continue(()))
    }

    fn step_back(&mut self) {
        if self.current_step > 0 {
            self.current_step -= 1;
        } else if self.draw_memory.inner_call_index > 0 {
            self.draw_memory.inner_call_index -= 1;
            self.current_step = self.n_steps() - 1;
        }
    }

    fn step(&mut self) {
        if self.current_step < self.n_steps() - 1 {
            self.current_step += 1;
        } else if self.draw_memory.inner_call_index < self.debug_arena().len() - 1 {
            self.draw_memory.inner_call_index += 1;
            self.current_step = 0;
        }
    }

    /// Calls a closure `f` the number of times specified in the key buffer, and at least once.
    fn repeat(&mut self, mut f: impl FnMut(&mut Self) -> Result<()>) -> Result<()> {
        for _ in 0..buffer_as_number(&self.key_buffer) {
            f(self)?;
        }

        Ok(())
    }

    fn n_steps(&self) -> usize {
        self.debug_steps().len()
    }
}

/// Grab number from buffer. Used for something like '10k' to move up 10 operations
fn buffer_as_number(s: &str) -> usize {
    const MIN: usize = 1;
    const MAX: usize = 100_000;
    s.parse().unwrap_or(MIN).clamp(MIN, MAX)
}
