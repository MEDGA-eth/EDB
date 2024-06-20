use ratatui::layout::{Constraint, Direction, Layout, Rect};
use revm::primitives::bitvec::vec;

/// The focus mode of the frontend.
#[derive(Debug)]
pub(crate) enum FocusMode {
    NormalEnter,
    NormalBrowse,
    Insert,
}

/// Used to keep track of which kind of pane is currently active
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Pane {
    // we need to have a specific code pane for each code kind,
    // since the code pane is flattened in the large screen layout
    CodePane(CodeView),
    DataPane,
    TerminalPane,
    OpcodePane,
}

/// The size of the screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ScreenSize {
    Small,
    Large,
}

/// Trace the focus, to ensure the pane switching is backed by a state machine.
pub(crate) struct Screen {
    pub(crate) screen: ScreenSize,
    pub(crate) focus: Pane,
    pub(crate) prev_focus: Option<Pane>,
    pub(crate) mode: FocusMode,

    pub(crate) active_data: DataView,
    pub(crate) active_code: CodeView,
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
impl Screen {
    pub(crate) fn new() -> Self {
        Self {
            screen: ScreenSize::Small,
            focus: Pane::CodePane(CodeView::Trace),
            prev_focus: None,
            mode: FocusMode::NormalBrowse,
            active_code: CodeView::Trace,
            active_data: DataView::Variable,
        }
    }

    pub(crate) fn split_screen(&self, app: Rect) -> Vec<Rect> {
        match self.screen {
            ScreenSize::Small => {
                // Split app in 2 horizontally.
                let [app_left, op_pane] = Layout::new(
                    Direction::Horizontal,
                    [Constraint::Ratio(3, 4), Constraint::Ratio(1, 4)],
                )
                .split(app)[..] else {
                    unreachable!()
                };

                // Split the right pane vertically to construct data and text panes.
                let [code_pane, data_pane, text_pane] = Layout::new(
                    Direction::Vertical,
                    [Constraint::Ratio(3, 8), Constraint::Ratio(3, 8), Constraint::Ratio(1, 4)],
                )
                .split(app_left)[..] else {
                    unreachable!()
                }; // Split app in 2 horizontally.

                vec![code_pane, data_pane, text_pane, op_pane]
            }
            ScreenSize::Large => {
                // Split app in 2 vertically.
                let [app_top, app_bottom] = Layout::new(
                    Direction::Vertical,
                    [Constraint::Ratio(3, 5), Constraint::Ratio(2, 5)],
                )
                .split(app)[..] else {
                    unreachable!()
                };

                // Split the upper pane in 3 vertically to trace, source, and opcode list .
                let [trace_pane, src_pane, op_pane] = Layout::new(
                    Direction::Horizontal,
                    [Constraint::Ratio(2, 5), Constraint::Ratio(2, 5), Constraint::Ratio(1, 5)],
                )
                .split(app_top)[..] else {
                    unreachable!()
                };

                // Split the lower pane horizontally to construct text aren and data panes.
                let [text_pane, data_pane] = Layout::new(
                    Direction::Horizontal,
                    [Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)],
                )
                .split(app_bottom)[..] else {
                    unreachable!()
                };

                vec![trace_pane, src_pane, op_pane, text_pane, data_pane]
            }
        }
    }

    pub(crate) fn is_focused_pane(&self, pane: Pane) -> bool {
        match self.screen {
            ScreenSize::Small => match self.focus {
                Pane::CodePane(_) => matches!(pane, Pane::CodePane(_)),
                _ => self.focus == pane,
            },
            ScreenSize::Large => self.focus == pane,
        }
    }

    pub(crate) fn set_normal_enter_mode(&mut self) {
        if self.focus == Pane::TerminalPane {
            // enter to the terminal pane should always be in insert mode
            self.set_insert_mode();
        } else {
            self.mode = FocusMode::NormalEnter;
        }
    }

    pub(crate) fn set_normal_browse_mode(&mut self) {
        self.mode = FocusMode::NormalBrowse;
    }

    pub(crate) fn set_insert_mode(&mut self) {
        self.mode = FocusMode::Insert;
        self.focus = Pane::TerminalPane;
    }

    pub(crate) fn switch_right(&mut self) {
        if self.is_small() {
            let focus = self.focus;
            match self.focus {
                Pane::CodePane(_) | Pane::DataPane | Pane::TerminalPane => {
                    self.focus = Pane::OpcodePane
                }
                Pane::OpcodePane => {
                    self.focus = self.prev_focus.unwrap_or(Pane::CodePane(self.active_code))
                }
            }
            self.prev_focus = Some(focus);
        } else {
            match self.focus {
                Pane::CodePane(CodeView::Trace) => self.focus = Pane::CodePane(CodeView::Source),
                Pane::CodePane(CodeView::Source) => self.focus = Pane::OpcodePane,
                Pane::OpcodePane => self.focus = Pane::CodePane(CodeView::Trace),
                Pane::DataPane => self.focus = Pane::TerminalPane,
                Pane::TerminalPane => self.focus = Pane::DataPane,
            }
            self.prev_focus = None;
        }
    }

    pub(crate) fn switch_left(&mut self) {
        if self.is_small() {
            // small screen layout only has two columns, so switch_left is the same as switch_right
            self.switch_right();
        } else {
            match self.focus {
                Pane::CodePane(CodeView::Trace) => self.focus = Pane::OpcodePane,
                Pane::CodePane(CodeView::Source) => self.focus = Pane::CodePane(CodeView::Trace),
                Pane::OpcodePane => self.focus = Pane::CodePane(CodeView::Source),
                Pane::DataPane => self.focus = Pane::TerminalPane,
                Pane::TerminalPane => self.focus = Pane::DataPane,
            }
            self.prev_focus = None; // reset the prev_focus
        }
    }

    pub(crate) fn switch_up(&mut self) {
        if self.is_small() {
            match self.focus {
                Pane::CodePane(_) => self.focus = Pane::TerminalPane,
                Pane::DataPane => self.focus = Pane::CodePane(CodeView::Trace),
                Pane::TerminalPane => self.focus = Pane::DataPane,
                Pane::OpcodePane => { /* do nothing */ }
            }
        } else {
            let focus = self.focus;
            match self.focus {
                Pane::CodePane(CodeView::Trace) => self.focus = Pane::TerminalPane,
                Pane::CodePane(CodeView::Source) => {
                    self.focus = self.prev_focus.unwrap_or(Pane::TerminalPane)
                }
                Pane::OpcodePane => self.focus = Pane::DataPane,
                Pane::DataPane => self.focus = self.prev_focus.unwrap_or(Pane::OpcodePane),

                Pane::TerminalPane => {
                    self.focus = self.prev_focus.unwrap_or(Pane::CodePane(CodeView::Trace))
                }
            }
            self.prev_focus = Some(focus);
        }
    }

    pub(crate) fn switch_down(&mut self) {
        if self.is_small() {
            match self.focus {
                Pane::CodePane(_) => self.focus = Pane::DataPane,
                Pane::DataPane => self.focus = Pane::TerminalPane,
                Pane::TerminalPane => self.focus = Pane::CodePane(CodeView::Trace),
                Pane::OpcodePane => { /* do nothing */ }
            }
        } else {
            // large screen layout only has two rows, so swith_down is the same as switch_up
            self.switch_up();
        }
    }

    pub(crate) fn is_small(&self) -> bool {
        matches!(self.screen, ScreenSize::Small)
    }

    pub(crate) fn is_large(&self) -> bool {
        matches!(self.screen, ScreenSize::Large)
    }

    pub(crate) fn set_large_screen(&mut self) {
        if self.is_small() {
            // when we switch from small screen to large screen, we reset the focus to the default
            self.prev_focus = None;
            self.focus = Pane::CodePane(CodeView::Trace);
            self.active_code = CodeView::Trace; // use trace as a placeholder
            self.screen = ScreenSize::Large;
        }
    }

    pub(crate) fn set_small_screen(&mut self) {
        if self.is_large() {
            self.prev_focus = None;
            self.focus = Pane::CodePane(self.active_code);
            self.active_code = CodeView::Trace;
            self.screen = ScreenSize::Small;
        }
    }
}

/// Used to keep track of which kind of data is currently active to be drawn by the debugger.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DataView {
    Variable,
    Expression,
    Memory,
    Calldata,
    Returndata,
    Stack,
}

impl DataView {
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
pub(crate) enum CodeView {
    Trace,
    Source,
}

impl CodeView {
    /// Helper to cycle through the active code panes.
    /// Note that opcode is not included here, since it is in a separate pane.
    pub(crate) fn next(&self) -> Self {
        match self {
            Self::Trace => Self::Source,
            Self::Source => Self::Trace,
        }
    }
}
