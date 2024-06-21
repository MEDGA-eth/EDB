use crossterm::event::KeyEvent;
use tui_textarea::TextArea;

use super::{TerminalMode, Window};

// Put all editor-realted methods here.
// For other methods, use the Deref and DerefMut traits to refer to ScreenManager.
impl<'a> Window<'a> {
    pub fn set_editor_insert_mode(&mut self) {
        self.editor_mode = TerminalMode::Insert;
    }

    pub fn set_editor_normal_mode(&mut self) {
        self.editor_mode = TerminalMode::Normal;
    }

    pub fn get_editor(&self) -> &TextArea<'a> {
        &self.editor
    }

    pub fn get_editor_mut(&mut self) -> &mut TextArea<'a> {
        &mut self.editor
    }

    pub fn handle_input(&mut self, key: KeyEvent) {
        self.editor.input(key);
    }
}
