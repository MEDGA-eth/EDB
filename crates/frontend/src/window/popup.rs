use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyEvent};
use eyre::{eyre, Result};

use crate::context::RecoverableError;

use super::{pane::Pane, PaneView, Window};

#[derive(Debug, Clone)]
pub enum PopupMode {
    ErrorMessage(String),
    ViewAssignment(u8),
}

#[derive(Debug, Clone)]
pub struct PopupMessage {
    pub title: String,
    pub message: String,
    pub highlights: HashSet<String>,
}

impl PopupMode {
    pub fn title(&self) -> &str {
        match self {
            Self::ErrorMessage(_) => " Error ",
            Self::ViewAssignment(_) => " View Assignment ",
        }
    }

    pub fn message(&self, pane: &Pane) -> (String, HashSet<String>) {
        let mut highlights = HashSet::new();
        match self {
            Self::ErrorMessage(message) => (message.clone(), highlights),
            Self::ViewAssignment(k) => {
                let mut message = "Select the following view to register\n-------------------------------------------\n".to_string();
                let mut assign_count = 0u8;
                for i in 0..PaneView::num_of_valid_views() {
                    let view = PaneView::from(i);
                    if !pane.has_view(&view) {
                        let new_line = format!("({assign_count}) {view}\n");
                        message.push_str(&new_line);
                        if *k == assign_count {
                            highlights.insert(new_line.trim().to_string());
                        }
                        assign_count += 1;
                    }
                }

                if pane.len() == 0 {
                    return (message, highlights);
                }

                message.push_str("\nSelect the following view to unregister\n-------------------------------------------\n");
                let mut unassign_count = 0u8;
                for i in 0..PaneView::num_of_valid_views() {
                    let view = PaneView::from(i);
                    if pane.has_view(&view) {
                        let new_line = format!("({}) {view}\n", (unassign_count + b'a') as char);
                        message.push_str(&new_line);
                        if *k == assign_count + unassign_count {
                            highlights.insert(new_line.trim().to_string());
                        }
                        unassign_count += 1;
                    }
                }
                (message, highlights)
            }
        }
    }
}

impl Window<'_> {
    pub fn pop_error_message(&mut self, message: String) {
        self.popup_mode = Some(PopupMode::ErrorMessage(message));
    }

    pub fn pop_assignment(&mut self) {
        self.popup_mode = Some(PopupMode::ViewAssignment(0));
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
        let (message, highlights) = mode.message(pane);

        Ok(PopupMessage { title: mode.title().to_string(), message, highlights })
    }

    pub fn handle_key_event_in_popup(&mut self, event: KeyEvent) -> Result<()> {
        match self.popup_mode.clone() {
            Some(PopupMode::ViewAssignment(k)) => self.handle_key_event_for_assignment(event, k),
            _ => Ok(()),
        }
    }

    fn handle_key_event_for_assignment(&mut self, event: KeyEvent, k: u8) -> Result<()> {
        match event.code {
            KeyCode::Char(c) => match c {
                '0'..='9' => self.handle_assignment(c as u8 - b'0')?,
                'a'..='j' => self.handle_assignment(
                    c as u8 - b'a' +
                        (PaneView::num_of_valid_views() - self.get_focused_pane()?.len() as u8),
                )?,
                _ => {}
            },
            KeyCode::Up => {
                self.popup_mode = Some(PopupMode::ViewAssignment(
                    (k + PaneView::num_of_valid_views() - 1) % PaneView::num_of_valid_views(),
                ))
            }
            KeyCode::Down => {
                self.popup_mode =
                    Some(PopupMode::ViewAssignment((k + 1) % PaneView::num_of_valid_views()))
            }
            KeyCode::Enter => self.handle_assignment(k)?,
            _ => {}
        }

        Ok(())
    }

    fn handle_assignment(&mut self, mut k: u8) -> Result<()> {
        let pane = self.get_focused_pane()?;
        if k < PaneView::num_of_valid_views() - pane.len() as u8 {
            // this will be registeration
            let mut count = 0u8;
            for i in 0..PaneView::num_of_valid_views() {
                let view = PaneView::from(i);
                if !pane.has_view(&view) {
                    if count == k {
                        let target = pane.id;
                        self.get_pane_manager_mut()?.assign(view, target).map_err(|e| {
                            RecoverableError::new(format!(
                                "Failed to register the selectced view ({view})\n\nReason: {e}",
                            ))
                        })?;
                        self.exit_popup();
                        return Ok(());
                    }
                    count += 1;
                }
            }
        } else {
            // this will be unregistration
            k -= PaneView::num_of_valid_views() - pane.len() as u8;
            let mut count = 0u8;
            for i in 0..PaneView::num_of_valid_views() {
                let view = PaneView::from(i);
                if pane.has_view(&view) {
                    if count == k {
                        self.get_pane_manager_mut()?.unassign(view).map_err(|e| {
                            RecoverableError::new(format!(
                                "Failed to unregister the selectced view ({view})\n\nReason: {e}"
                            ))
                        })?;
                        self.exit_popup();
                        return Ok(());
                    }
                    count += 1;
                }
            }
        }

        Err(RecoverableError::new("Invalid selection").into())
    }
}
