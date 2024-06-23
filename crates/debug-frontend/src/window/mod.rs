mod editor;
mod pane;
mod popup;
mod screen;

use std::ops::{Deref, DerefMut};

use eyre::Result;
use tui_textarea::TextArea;

pub use pane::{PaneFlattened, PaneView};
pub use popup::{PopupMessage, PopupMode};
use screen::ScreenManager;

/// The focus mode of the frontend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerminalMode {
    Normal,
    Insert,
}

pub struct Window<'a> {
    editor: TextArea<'a>,
    screen: ScreenManager,

    pub editor_mode: TerminalMode,
    pub popup_mode: Option<PopupMode>,
}

impl<'a> Deref for Window<'a> {
    type Target = ScreenManager;

    fn deref(&self) -> &Self::Target {
        &self.screen
    }
}

impl<'a> DerefMut for Window<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.screen
    }
}

impl<'a> Window<'a> {
    pub fn new() -> Result<Self> {
        Ok(Self {
            editor: TextArea::default(),
            editor_mode: TerminalMode::Normal,
            screen: ScreenManager::new()?,
            popup_mode: None,
        })
    }
}
