use crossterm::event::{KeyCode, KeyEvent};
use eyre::{eyre, Result};

use crate::context::RecoverableError;

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
                        message.push_str(&format!("({}) {}\n", assign_count, view.to_string()));
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
                            "({}) {}\n",
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

    pub fn has_popup(&self) -> bool {
        self.popup_mode.is_some()
    }

    /// Returns the popup message, the width, and the height.
    pub fn get_popup_message(&self) -> Result<PopupMessage> {
        let mode = self.popup_mode.as_ref().ok_or_else(|| eyre!("no popup mode is active"))?;
        let pane = self.get_focused_pane()?;
        let message = mode.message(pane);

        Ok(PopupMessage { title: mode.title().to_string(), message })
    }

    pub fn handle_key_even_in_popup(&mut self, event: KeyEvent) -> Result<()> {
        match self.popup_mode.clone() {
            Some(PopupMode::ViewAssignment) => self.handle_assignment(event),
            _ => Ok(()),
        }
    }

    fn handle_assignment(&mut self, event: KeyEvent) -> Result<()> {
        match event.code {
            KeyCode::Char(c) => {
                match c {
                    '0'..='9' => {
                        let pane = self.get_focused_pane()?;
                        let mut count = 0u8;
                        for i in 0..10usize {
                            let view = PaneView::from(i);
                            if !pane.has_view(&view) {
                                if count == c as u8 - b'0' {
                                    let target = pane.id;
                                    self.get_current_pane_mut()?.assign(view, target).map_err(|e| RecoverableError::new(format!("Failed to register the selectced view ({})\n\nReason: {}", view.to_string(), e.to_string())))?;
                                    self.exit_popup();
                                    return Ok(());
                                }
                                count += 1;
                            }
                        }
                    }
                    'a'..='j' => {
                        let pane = self.get_focused_pane()?;
                        let mut count = 0u8;
                        for i in 0..10usize {
                            let view = PaneView::from(i);
                            if pane.has_view(&view) {
                                if count == c as u8 - b'a' {
                                    self.get_current_pane_mut()?.unassign(view).map_err(|e| RecoverableError::new(format!("Failed to unregister the selectced view ({})\n\nReason: {}", view.to_string(), e.to_string())))?;
                                    self.exit_popup();
                                    return Ok(());
                                }
                                count += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        Ok(())
    }
}
