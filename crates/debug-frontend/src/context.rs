//! Debugger context and event handler implementation.

use alloy_primitives::Address;
use crossterm::{
    event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind},
    terminal,
};
use edb_debug_backend::artifact::debug::{DebugArtifact, DebugNodeFlat, DebugStep};
use ratatui::{
    style::{Modifier, Style},
    text::Text,
    widgets::{Block, Borders},
};
use revm_inspectors::tracing::types::CallKind;
use std::{cell::RefCell, fmt::Debug, ops::ControlFlow, rc::Rc};
use tui_textarea::TextArea;

use crate::{core::ExitReason, DebugFrontend};

/// The focus mode of the frontend.
#[derive(Debug)]
pub(crate) enum FocusMode {
    Normal,
    Insert,
}

/// This is currently used to remember last scroll position so screen doesn't wiggle as much.
#[derive(Default)]
pub(crate) struct DrawMemory {
    pub(crate) inner_call_index: usize,
    pub(crate) current_buf_startline: usize,
    pub(crate) current_stack_startline: usize,
}

/// Used to keep track of which kind of pane is currently active
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PaneKind {
    // we need to have a specific code pane for each code kind,
    // since the code pane is flattened in the large screen layout
    CodePane(CodeKind),
    DataPane,
    TerminalPane,
    OpcodePane,
}

/// The size of the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Screen {
    SmallScreen,
    LargeScreen,
}

/// Trace the focus, to ensure the pane switching is backed by a state machine.
pub(crate) struct ScreenMode {
    pub(crate) screen: Screen,
    pub(crate) focus: PaneKind,
    pub(crate) prev_focus: Option<PaneKind>,
    pub(crate) mode: FocusMode,

    pub(crate) active_data: DataKind,
    pub(crate) active_code: CodeKind,
}

/// The layout of two different screens.
///
/// The small screen layout is:
/// ```text
/// +-----------------+-------+
/// | CodePane        |       |
/// +-----------------+   op  |
/// | DataPane        |  code |
/// +-----------------+  list |
/// | TerminalPane    |       |
/// +-----------------+-------+
///                    // prev_focus is only meaningful in op code list
/// ```
///
/// The large screen layout is:
/// ```text
/// +---------+--------+--------+
/// |  Trace  | Source | opcode |
/// +---------+-----+--+--------+
/// | Terminal Pane | Data Pane |
/// +---------+--------+--------+
/// ```
impl ScreenMode {
    pub(crate) fn new() -> Self {
        Self {
            screen: Screen::SmallScreen,
            focus: PaneKind::CodePane(CodeKind::Trace),
            prev_focus: None,
            mode: FocusMode::Normal,
            active_code: CodeKind::Trace,
            active_data: DataKind::Variable,
        }
    }

    pub(crate) fn switch_right(&mut self) {
        if self.is_small() {
            // todo
        } else {
            match self.focus {
                PaneKind::CodePane(CodeKind::Trace) => {
                    self.focus = PaneKind::CodePane(CodeKind::Source)
                }
                PaneKind::CodePane(CodeKind::Source) => self.focus = PaneKind::OpcodePane,
                PaneKind::CodePane(CodeKind::General) => {
                    unreachable!("general code pane should not appear in large screen layout")
                }
                PaneKind::OpcodePane => self.focus = PaneKind::CodePane(CodeKind::Trace),
                PaneKind::DataPane => self.focus = PaneKind::TerminalPane,
                PaneKind::TerminalPane => self.focus = PaneKind::DataPane,
            }
            self.prev_focus = None;
        }
    }

    pub(crate) fn switch_left(&mut self) {
        if self.is_small() {
            // todo
        } else {
            match self.focus {
                PaneKind::CodePane(CodeKind::Trace) => {
                    self.focus = PaneKind::CodePane(CodeKind::Source)
                }
                PaneKind::CodePane(CodeKind::Source) => self.focus = PaneKind::OpcodePane,
                PaneKind::CodePane(CodeKind::General) => {
                    unreachable!("general code pane should not appear in large screen layout")
                }
                PaneKind::OpcodePane => self.focus = PaneKind::CodePane(CodeKind::Trace),
                PaneKind::DataPane => self.focus = PaneKind::TerminalPane,
                PaneKind::TerminalPane => self.focus = PaneKind::DataPane,
            }
            self.prev_focus = None; // reset the prev_focus
        }
    }

    pub(crate) fn swith_up(&mut self) {
        if self.is_small() {
            match self.focus {
                PaneKind::CodePane(CodeKind::General) => self.focus = PaneKind::TerminalPane,
                PaneKind::CodePane(_) => {
                    unreachable!("flatten code pane should not appear in small screen layout")
                }
                PaneKind::DataPane => self.focus = PaneKind::CodePane(CodeKind::Trace),
                PaneKind::TerminalPane => self.focus = PaneKind::DataPane,
                PaneKind::OpcodePane => { /* do nothing */ }
            }
        } else {
            match self.focus {
                PaneKind::CodePane(CodeKind::Trace) => self.focus = PaneKind::TerminalPane,
                PaneKind::CodePane(CodeKind::Source) => {
                    self.focus = self.prev_focus.unwrap_or(PaneKind::TerminalPane)
                }
                PaneKind::CodePane(CodeKind::General) => {
                    unreachable!("general code pane should not appear in large screen layout")
                }
                PaneKind::OpcodePane => self.focus = PaneKind::DataPane,
                PaneKind::DataPane => self.focus = self.prev_focus.unwrap_or(PaneKind::OpcodePane),

                PaneKind::TerminalPane => {
                    self.focus = self.prev_focus.unwrap_or(PaneKind::CodePane((CodeKind::Trace)))
                }
            }
            self.prev_focus = Some(self.focus);
        }
    }

    pub(crate) fn switch_down(&mut self) {
        if self.is_small() {
            match self.focus {
                PaneKind::CodePane(CodeKind::General) => self.focus = PaneKind::DataPane,
                PaneKind::CodePane(_) => {
                    unreachable!("flatten code pane should not appear in small screen layout")
                }
                PaneKind::DataPane => self.focus = PaneKind::TerminalPane,
                PaneKind::TerminalPane => self.focus = PaneKind::CodePane(CodeKind::Trace),
                PaneKind::OpcodePane => { /* do nothing */ }
            }
        } else {
            // large screen layout only has two rows, so swith_down is the same as switch_up
            self.swith_up();
        }
    }

    pub(crate) fn is_small(&self) -> bool {
        matches!(self.screen, Screen::SmallScreen)
    }

    pub(crate) fn is_large(&self) -> bool {
        matches!(self.screen, Screen::LargeScreen)
    }

    pub(crate) fn set_large_screen(&mut self) {
        if self.is_small() {
            // when we switch from small screen to large screen, we reset the focus to the default
            self.prev_focus = None;
            self.focus = PaneKind::CodePane(CodeKind::Trace);
            self.active_code = CodeKind::General;
            self.screen = Screen::LargeScreen;
        }
    }

    pub(crate) fn set_small_screen(&mut self) {
        if self.is_large() {
            self.prev_focus = None;
            self.focus = PaneKind::CodePane(CodeKind::General);
            self.active_code = CodeKind::Trace;
            self.screen = Screen::SmallScreen;
        }
    }
}

/// Used to keep track of which kind of data is currently active to be drawn by the debugger.
#[derive(Debug, Clone, Copy, PartialEq)]
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
}

/// Used to keep track of which kind of code is currently active to be drawn by the debugger.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CodeKind {
    Trace,
    Source,
    General,
}

impl CodeKind {
    /// Helper to cycle through the active code panes.
    pub(crate) fn next(&self) -> Self {
        match self {
            Self::Trace => Self::Source,
            Self::Source => Self::Trace,
            Self::General => Self::General,
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
    /// The terminal pane
    pub(crate) terminal: TextArea<'a>,

    /// The current screen.
    pub(crate) screen: ScreenMode,
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
            terminal: TextArea::default(),

            screen: ScreenMode::new(),
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
        match self.screen.active_data {
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
            KeyCode::Char('K') if self.screen.is_small() => {
                self.screen.active_code = self.screen.active_code.next();
            }

            // Cycle data
            KeyCode::Char('b') => {
                self.screen.active_data = self.screen.active_data.next();
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

            // XXX: it is a test for text terminal
            // Unknown/unhandled key code
            _ => {
                self.terminal.input(event);
            }
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
