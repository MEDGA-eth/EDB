use std::ops::{Deref, DerefMut};

use crossterm::event::KeyEvent;
use eyre::Result;
use tui_textarea::TextArea;

use crate::screen::ScreenManager;

/// The focus mode of the frontend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerminalMode {
    Normal,
    Insert,
}

pub struct TerminalManager<'a> {
    editor: TextArea<'a>,
    pub mode: TerminalMode,
}

impl<'a> Deref for TerminalManager<'a> {
    type Target = TextArea<'a>;

    fn deref(&self) -> &Self::Target {
        &self.editor
    }
}

impl<'a> DerefMut for TerminalManager<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.editor
    }
}

impl<'a> TerminalManager<'a> {
    pub fn new() -> Self {
        Self { editor: TextArea::default(), mode: TerminalMode::Insert }
    }

    pub fn enter_insert_mode(&mut self) {
        self.mode = TerminalMode::Insert;
    }

    pub fn enter_normal_mode(&mut self) {
        self.mode = TerminalMode::Normal;
    }
}
