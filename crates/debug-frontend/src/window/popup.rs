use crossterm::event::KeyEvent;
use eyre::{eyre, Result};

use super::{pane::Pane, PaneView, Window};

#[derive(Debug, Clone)]
pub enum PopupMode {
    ErrorMessage(String),
    ViewAssignment,
}

#[derive(Debug, Clone)]
pub struct PopupMessage {
    pub title: String,
    pub message: String,
}

impl PopupMode {
    pub fn title(&self) -> &str {
        match self {
            Self::ErrorMessage(_) => " Error ",
            Self::ViewAssignment => " View Assignment ",
        }
    }

    pub fn message(&self, pane: &Pane) -> String {
        match self {
            Self::ErrorMessage(message) => message.clone(),
            Self::ViewAssignment => {
                let mut message = "Select the following view to register\n-------------------------------------------\n".to_string();
                let mut assign_count = 0u8;
                for i in 0..10usize {
                    let view = PaneView::from(i);
                    if !pane.has_view(&view) {
                        message.push_str(&format!("[{}] {}\n", assign_count, view.to_string()));
                        assign_count += 1;
                    }
                }

                if pane.len() == 0 {
                    return message;
                }

                message.push_str("\nSelect the following view to unregister\n-------------------------------------------\n");
                let mut unassign_count = 0u8;
                for i in 0..10usize {
                    let view = PaneView::from(i);
                    if pane.has_view(&view) {
                        message.push_str(&format!(
                            "[{}] {}\n",
                            (unassign_count + b'a') as char,
                            view.to_string()
                        ));
                        unassign_count += 1;
                    }
                }
                message
            }
        }
    }
}

impl Window<'_> {
    pub fn pop_error_message(&mut self, message: String) {
        self.popup_mode = Some(PopupMode::ErrorMessage(message));
    }

    pub fn pop_assignment(&mut self) {
        self.popup_mode = Some(PopupMode::ViewAssignment);
    }

    pub fn exit_popup(&mut self) {
        self.popup_mode = None;
    }

    /// Returns the popup message, the width, and the height.
    pub fn get_popup_message(&self) -> Result<PopupMessage> {
        let mode = self.popup_mode.as_ref().ok_or_else(|| eyre!("no popup mode is active"))?;
        let pane = self.get_focused_pane()?;
        let message = mode.message(pane);

        Ok(PopupMessage { title: mode.title().to_string(), message })
    }

    pub fn handle_key_even_in_popup(&mut self, event: KeyEvent) {
        todo!();
    }
}
