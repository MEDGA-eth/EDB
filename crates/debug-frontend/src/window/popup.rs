use crossterm::event::KeyEvent;

use super::Window;

#[derive(Debug, Clone)]
pub enum PopupMode {
    ErrorMessage(String),
    ViewAssignment,
    ViewUnassignment,
}

impl Window<'_> {
    pub fn pop_error_message(&mut self, message: String) {
        self.popup_mode = Some(PopupMode::ErrorMessage(message));
    }

    pub fn pop_view_assignment(&mut self) {
        self.popup_mode = Some(PopupMode::ViewAssignment);
    }

    pub fn has_popup(&self) -> bool {
        self.popup_mode.is_some()
    }

    pub fn exit_popup(&mut self) {
        self.popup_mode = None;
    }

    pub fn handle_key_even_in_popup(&mut self, event: KeyEvent) {
        todo!();
    }
}
