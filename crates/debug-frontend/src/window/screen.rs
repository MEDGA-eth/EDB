use std::collections::HashMap;

use eyre::{ensure, Result};
use ratatui::layout::{Direction, Rect};

use crate::{
    context::RecoverableError,
    window::pane::{Pane, PaneFlattened, PaneManager, PaneView, Point},
};

use super::pane::{BorderSide, PaneId};

pub const SMALL_SCREEN_STR: &str = "Defualt (Small Screen)";
pub const LARGE_SCREEN_STR: &str = "Defualt (Large Screen)";

/// Trace the focus, to ensure the pane switching is backed by a state machine.
pub struct ScreenManager {
    pub panes: HashMap<String, PaneManager>,
    pub current_pane: String,
    pub use_default_pane: bool,
    pub full_screen: bool,
    pub pending_mouse_move: Option<Point>,
}

impl ScreenManager {
    pub fn new() -> Result<Self> {
        let mut manager = Self {
            panes: HashMap::new(),
            current_pane: String::new(),
            full_screen: false,
            use_default_pane: true,
            pending_mouse_move: None,
        };

        manager.add_pane_manager(SMALL_SCREEN_STR, PaneManager::default_small_screen()?);
        manager.add_pane_manager(LARGE_SCREEN_STR, PaneManager::default_large_screen()?);
        manager.current_pane = SMALL_SCREEN_STR.to_string();

        Ok(manager)
    }

    pub fn get_focused_pane_mut(&mut self) -> Result<&mut Pane> {
        self.get_current_pane_mut()?.get_focused_pane_mut()
    }
    pub fn get_focused_pane(&self) -> Result<&Pane> {
        self.get_current_pane()?.get_focused_pane()
    }

    pub fn get_focused_view(&self) -> Result<PaneView> {
        self.get_current_pane()?.get_focused_view()
    }

    pub fn get_available_pane_profiles(&self) -> Vec<String> {
        self.panes.keys().cloned().collect()
    }

    pub fn use_default_pane_profile(&mut self, use_default_pane: bool) {
        self.use_default_pane = use_default_pane;
    }

    pub fn add_pane_manager(&mut self, name: &str, manager: PaneManager) {
        self.panes.insert(name.to_string(), manager);
    }

    pub fn toggle_full_screen(&mut self) {
        self.full_screen = !self.full_screen;
    }

    pub fn set_mouse_move(&mut self, x: u16, y: u16) {
        if self.full_screen {
            // Ignore mouse move in full screen mode.
            return;
        }
        self.pending_mouse_move = Some(Point::new(x, y));
    }

    pub fn get_current_pane(&self) -> Result<&PaneManager> {
        self.panes.get(&self.current_pane).ok_or(eyre::eyre!("No current pane"))
    }

    pub fn get_current_pane_mut(&mut self) -> Result<&mut PaneManager> {
        self.panes.get_mut(&self.current_pane).ok_or(eyre::eyre!("No current pane"))
    }

    pub fn enter_terminal(&mut self) -> Result<()> {
        ensure!(!self.full_screen, "Cannot enter terminal in full screen mode");
        self.get_current_pane_mut()?.force_goto_by_view(PaneView::Terminal)?;

        Ok(())
    }

    pub fn set_pane(&mut self, name: &str) -> Result<()> {
        if self.panes.contains_key(name) {
            self.current_pane = name.to_string();
            Ok(())
        } else {
            Err(eyre::eyre!("No such pane"))
        }
    }

    pub fn set_large_screen(&mut self) {
        self.current_pane = LARGE_SCREEN_STR.to_string();
    }

    pub fn set_small_screen(&mut self) {
        self.current_pane = SMALL_SCREEN_STR.to_string();
    }

    pub fn focus_up(&mut self) -> Result<()> {
        self.get_current_pane_mut()?.focus_up()
    }

    pub fn focus_down(&mut self) -> Result<()> {
        self.get_current_pane_mut()?.focus_down()
    }

    pub fn focus_left(&mut self) -> Result<()> {
        self.get_current_pane_mut()?.focus_left()
    }

    pub fn focus_right(&mut self) -> Result<()> {
        self.get_current_pane_mut()?.focus_right()
    }

    pub fn split_focused_pane(&mut self, direction: Direction, ratio: [u32; 2]) -> Result<()> {
        let id = self.get_focused_pane()?.id;
        self.get_current_pane_mut()?.split(id, direction, ratio)?;
        Ok(())
    }

    fn merge_pane_with_neighbor(&mut self, id: PaneId, direction: BorderSide) -> Result<()> {
        match direction {
            BorderSide::Left => {
                self.focus_left()?;
                let new_id = self.get_focused_pane()?.id;
                if self.get_current_pane_mut()?.merge(id, new_id).is_ok() {
                    return Ok(());
                }
                self.focus_right()?;
            }
            BorderSide::Right => {
                self.focus_right()?;
                let new_id = self.get_focused_pane()?.id;
                if self.get_current_pane_mut()?.merge(id, new_id).is_ok() {
                    return Ok(());
                }
                self.focus_left()?;
            }
            BorderSide::Top => {
                self.focus_up()?;
                let new_id = self.get_focused_pane()?.id;
                if self.get_current_pane_mut()?.merge(id, new_id).is_ok() {
                    return Ok(());
                }
                self.focus_down()?;
            }
            BorderSide::Bottom => {
                self.focus_down()?;
                let new_id = self.get_focused_pane()?.id;
                if self.get_current_pane_mut()?.merge(id, new_id).is_ok() {
                    return Ok(());
                }
                self.focus_up()?;
            }
        }

        Err(eyre::eyre!("Cannot merge the pane with its neighbor"))
    }

    pub fn close_focused_pane(&mut self) -> Result<()> {
        let pane = self.get_focused_pane()?;
        if pane.get_current_view() == PaneView::Terminal {
            return Err(RecoverableError::new("You cannot close the script terminal pane.").into());
        }

        let id = pane.id;

        // let's try to merge the pane with its neighbor.
        // we may want to first marge the latest split pane.
        let mut indexed_values: Vec<(usize, i32)> = self
            .get_current_pane()?
            .get_borders(id)?
            .iter()
            .map(|x| x.map(|y| y as i32).unwrap_or(-1))
            .enumerate()
            .collect();
        indexed_values.sort_by(|a, b| b.1.cmp(&a.1));
        for (direction, v) in indexed_values {
            if v < 0 {
                break;
            }
            if self.merge_pane_with_neighbor(id, BorderSide::try_from(direction)?).is_ok() {
                return Ok(());
            }
        }

        ensure!(self.get_focused_pane()?.id == id, "cannot move back to the original pane");
        Err(RecoverableError::new("The current pane cannot be merged with others due to one of the following reasons:\n\n1. The current pane cannot be merged with any adjacent panes.\n\n2. The current pane contains valid debug views and can only be merged with a Terminal Pane.\n\n3. The current pane has an adjacent pane, but these two are not directly split from the same parent pane.\n\nTo close this pane, you may consider unregistering some views or closing other panes first.").into())
    }

    pub fn get_flattened_layout(&self, app: Rect) -> Result<Vec<PaneFlattened>> {
        if self.full_screen {
            let pane = self.get_current_pane()?.get_focused_pane()?;
            Ok(vec![PaneFlattened {
                view: pane.get_current_view(),
                id: pane.id,
                focused: true,
                rect: app,
            }])
        } else {
            Ok(self.get_current_pane()?.get_flattened_layout(app)?)
        }
    }
}
