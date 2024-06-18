//! Debugger context and event handler implementation.

use alloy_primitives::Address;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use edb_debug_backend::artifact::debug::{DebugArtifact, DebugNodeFlat, DebugStep};
use revm_inspectors::tracing::types::CallKind;
use std::{cell::RefCell, fmt::Debug, ops::ControlFlow, rc::Rc};

use crate::{core::ExitReason, DebugFrontend};

/// This is currently used to remember last scroll position so screen doesn't wiggle as much.
#[derive(Default)]
pub(crate) struct DrawMemory {
    pub(crate) inner_call_index: usize,
    pub(crate) current_buf_startline: usize,
    pub(crate) current_stack_startline: usize,
}

/// Used to keep track of which kind of data is currently active to be drawn by the debugger.
#[derive(Debug, PartialEq)]
pub(crate) enum DataKind {
    Variable,
    Expression,
    Memory,
    Calldata,
    Returndata,
    Stack,
}

impl DataKind {
    /// Helper to cycle through the active buffers.
    pub(crate) fn next(&self) -> Self {
        match self {
            Self::Variable => Self::Expression,
            Self::Expression => Self::Memory,
            Self::Memory => Self::Calldata,
            Self::Calldata => Self::Returndata,
            Self::Returndata => Self::Stack,
            Self::Stack => Self::Variable,
        }
    }

    /// Helper to format the title of the active buffer pane
    pub(crate) fn title(&self, size: usize) -> String {
        match self {
            Self::Variable => format!("Live Variables (number: {size}))"),
            Self::Expression => format!("Watchers (number: {size})"),
            Self::Memory => format!("Memory (max expansion: {size} bytes)"),
            Self::Calldata => format!("Calldata (size: {size} bytes)"),
            Self::Returndata => format!("Returndata (size: {size} bytes)"),
            Self::Stack => format!("Stack (depth: {size})"),
        }
    }
}

/// Used to keep track of which kind of code is currently active to be drawn by the debugger.
#[derive(Debug, PartialEq)]
pub(crate) enum CodeKind {
    Trace,
    Source,
}

impl CodeKind {
    /// Helper to cycle through the active code panes.
    pub(crate) fn next(&self) -> Self {
        match self {
            Self::Trace => Self::Source,
            Self::Source => Self::Trace,
        }
    }
}

pub(crate) struct FrontendContext<'a> {
    pub(crate) artifact: &'a mut DebugArtifact,

    /// Buffer for keys prior to execution, i.e. '10' + 'k' => move up 10 operations.
    pub(crate) key_buffer: String,
    /// Current step in the debug steps.
    pub(crate) current_step: usize,
    pub(crate) draw_memory: DrawMemory,
    pub(crate) opcode_list: Vec<String>,
    pub(crate) last_index: usize,

    pub(crate) stack_labels: bool,
    /// Whether to decode active buffer as utf8 or not.
    pub(crate) buf_utf: bool,
    pub(crate) show_shortcuts: bool,
    /// The currently active data pane to be drawn.
    pub(crate) active_data: DataKind,
    /// The currently active code pane to be drawn.
    pub(crate) active_code: CodeKind,

    /// The current screen size.
    pub(crate) is_small_screen: bool,
}

impl<'a> FrontendContext<'a> {
    pub(crate) fn new(artifact: &'a mut DebugArtifact) -> Self {
        FrontendContext {
            artifact,

            key_buffer: String::with_capacity(64),
            current_step: 0,
            draw_memory: DrawMemory::default(),
            opcode_list: Vec::new(),
            last_index: 0,

            stack_labels: false,
            buf_utf: false,
            show_shortcuts: true,
            active_data: DataKind::Variable,
            active_code: CodeKind::Trace,

            is_small_screen: true,
        }
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

    fn active_data_depth(&self) -> usize {
        match self.active_data {
            DataKind::Memory => self.current_step().memory.len() / 32,
            DataKind::Calldata => self.current_step().calldata.len() / 32,
            DataKind::Returndata => self.current_step().returndata.len() / 32,
            DataKind::Stack => self.current_step().stack.len(),
            DataKind::Expression | DataKind::Variable => todo!(),
        }
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
        // Breakpoints
        if let KeyCode::Char(c) = event.code {
            if c.is_alphabetic() && self.key_buffer.starts_with('\'') {
                self.handle_breakpoint(c);
                return ControlFlow::Continue(());
            }
        }

        let control = event.modifiers.contains(KeyModifiers::CONTROL);

        match event.code {
            // Exit
            KeyCode::Char('q') => return ControlFlow::Break(ExitReason::CharExit),

            // Scroll up the memory buffer
            KeyCode::Char('k') | KeyCode::Up if control => self.repeat(|this| {
                this.draw_memory.current_buf_startline =
                    this.draw_memory.current_buf_startline.saturating_sub(1);
            }),
            // Scroll down the memory buffer
            KeyCode::Char('j') | KeyCode::Down if control => self.repeat(|this| {
                let max_buf = this.active_data_depth().saturating_sub(1);
                if this.draw_memory.current_buf_startline < max_buf {
                    this.draw_memory.current_buf_startline += 1;
                }
            }),

            // Move up
            KeyCode::Char('k') | KeyCode::Up => self.repeat(Self::step_back),
            // Move down
            KeyCode::Char('j') | KeyCode::Down => self.repeat(Self::step),

            // Cycle code
            KeyCode::Char('K') if self.is_small_screen => {
                self.active_code = self.active_code.next();
            }

            // Cycle data
            KeyCode::Char('b') => {
                self.active_data = self.active_data.next();
                self.draw_memory.current_buf_startline = 0;
            }

            // Go to top of file
            KeyCode::Char('g') => {
                self.draw_memory.inner_call_index = 0;
                self.current_step = 0;
            }

            // Go to bottom of file
            KeyCode::Char('G') => {
                self.draw_memory.inner_call_index = self.debug_arena().len() - 1;
                self.current_step = self.n_steps() - 1;
            }

            // Go to previous call
            KeyCode::Char('c') => {
                self.draw_memory.inner_call_index =
                    self.draw_memory.inner_call_index.saturating_sub(1);
                self.current_step = self.n_steps() - 1;
            }

            // Go to next call
            KeyCode::Char('C') => {
                if self.debug_arena().len() > self.draw_memory.inner_call_index + 1 {
                    self.draw_memory.inner_call_index += 1;
                    self.current_step = 0;
                }
            }

            // Step forward
            KeyCode::Char('s') => self.repeat(|this| {
                let remaining_ops = &this.opcode_list[this.current_step..];
                if let Some((i, _)) = remaining_ops.iter().enumerate().skip(1).find(|&(i, op)| {
                    let prev = &remaining_ops[i - 1];
                    let prev_is_jump = prev.contains("JUMP") && prev != "JUMPDEST";
                    let is_jumpdest = op == "JUMPDEST";
                    prev_is_jump && is_jumpdest
                }) {
                    this.current_step += i;
                }
            }),

            // Step backwards
            KeyCode::Char('a') => self.repeat(|this| {
                let ops = &this.opcode_list[..this.current_step];
                this.current_step = ops
                    .iter()
                    .enumerate()
                    .skip(1)
                    .rev()
                    .find(|&(i, op)| {
                        let prev = &ops[i - 1];
                        let prev_is_jump = prev.contains("JUMP") && prev != "JUMPDEST";
                        let is_jumpdest = op == "JUMPDEST";
                        prev_is_jump && is_jumpdest
                    })
                    .map(|(i, _)| i)
                    .unwrap_or_default();
            }),

            // Toggle stack labels
            KeyCode::Char('t') => self.stack_labels = !self.stack_labels,

            // Toggle memory UTF-8 decoding
            KeyCode::Char('m') => self.buf_utf = !self.buf_utf,

            // Toggle help notice
            KeyCode::Char('h') => self.show_shortcuts = !self.show_shortcuts,

            // Numbers for repeating commands or breakpoints
            KeyCode::Char(
                other @ ('0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9' | '\''),
            ) => {
                // Early return to not clear the buffer.
                self.key_buffer.push(other);
                return ControlFlow::Continue(());
            }

            // Unknown/unhandled key code
            _ => {}
        };

        self.key_buffer.clear();
        ControlFlow::Continue(())
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
        match event.kind {
            MouseEventKind::ScrollUp => self.step_back(),
            MouseEventKind::ScrollDown => self.step(),
            _ => {}
        }

        ControlFlow::Continue(())
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
    fn repeat(&mut self, mut f: impl FnMut(&mut Self)) {
        for _ in 0..buffer_as_number(&self.key_buffer) {
            f(self);
        }
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
