mod editor;
mod pane;
mod popup;
mod screen;

use std::{
    cell::{RefCell, RefMut},
    ops::{Deref, DerefMut},
    rc::Rc,
};

use eyre::Result;
use ratatui::layout::Rect;
use tui_textarea::TextArea;

pub use pane::{PaneFlattened, PaneView, VirtCoord};
pub use popup::{PopupMessage, PopupMode};
use screen::ScreenManager;

/// The focus mode of the frontend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerminalMode {
    Normal,
    Insert,
}

pub struct Window<'a> {
    screen: ScreenManager,

    pub screen_size: Rect,
    pub editor: Rc<RefCell<TextArea<'a>>>,
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
            editor: Rc::new(RefCell::new(TextArea::default())),
            editor_mode: TerminalMode::Normal,
            screen: ScreenManager::new()?,
            screen_size: Rect::default(),
            popup_mode: None,
        })
    }
}
