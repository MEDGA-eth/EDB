use crossterm::event::{KeyCode, KeyEvent};

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

    pub fn handle_input(&mut self, key: KeyEvent) {
        match self.editor_mode {
            TerminalMode::Insert => self.handle_insert_mode(key),
            TerminalMode::Normal => self.handle_normal_mode(key),
        }
    }

    pub fn handle_normal_mode(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('i') => self.set_editor_insert_mode(),
            _ => {
                self.editor.borrow_mut().input(key);
            }
        }
    }

    pub fn handle_insert_mode(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.set_editor_normal_mode(),
            _ => {
                self.editor.borrow_mut().input(key);
            }
        }
    }
}
