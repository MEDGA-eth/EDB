use std::collections::HashMap;

use eyre::{ensure, OptionExt, Result};
use ratatui::layout::{Direction, Rect};

use crate::{
    context::RecoverableError,
    window::pane::{Pane, PaneFlattened, PaneManager, PaneView},
};

use super::pane::{BorderSide, PaneId};

pub const SMALL_SCREEN_STR: &str = "Defualt (Small Screen)";
pub const LARGE_SCREEN_STR: &str = "Defualt (Large Screen)";

/// A fully abstracted screen manager that manages the layout of the screen.
/// The screen manager is not aware of the actual terminal size, but it is aware of the layout of
/// the screen.
pub struct ScreenManager {
    pub panes: HashMap<String, PaneManager>,
    pub current_pane: String,
    pub use_default_pane: bool,
    pub full_screen: bool,
}

impl ScreenManager {
    pub fn new() -> Result<Self> {
        let mut manager = Self {
            panes: HashMap::new(),
            current_pane: String::new(),
            full_screen: false,
            use_default_pane: true,
        };

        manager.add_pane_manager(SMALL_SCREEN_STR, PaneManager::default_small_screen()?);
        manager.add_pane_manager(LARGE_SCREEN_STR, PaneManager::default_large_screen()?);
        manager.current_pane = SMALL_SCREEN_STR.to_string();

        Ok(manager)
    }

    pub fn get_focused_pane_mut(&mut self) -> Result<&mut Pane> {
        self.get_pane_manager_mut()?.get_focused_pane_mut()
    }
    pub fn get_focused_pane(&self) -> Result<&Pane> {
        self.get_pane_manager()?.get_focused_pane()
    }

    pub fn get_focused_view(&self) -> Result<PaneView> {
        self.get_pane_manager()?.get_focused_view()
    }

    #[allow(dead_code)] // XXX (ZZ): Remove this later after the implementation is done.
    pub fn get_available_pane_profiles(&self) -> Vec<String> {
        self.panes.keys().cloned().collect()
    }

    #[allow(dead_code)] // XXX (ZZ): Remove this later after the implementation is done.
    pub fn use_default_pane_profile(&mut self, use_default_pane: bool) {
        self.use_default_pane = use_default_pane;
    }

    pub fn add_pane_manager(&mut self, name: &str, manager: PaneManager) {
        self.panes.insert(name.to_string(), manager);
    }

    pub fn toggle_full_screen(&mut self) {
        self.full_screen = !self.full_screen;
    }

    pub fn get_pane_manager(&self) -> Result<&PaneManager> {
        self.panes.get(&self.current_pane).ok_or_eyre("No current pane")
    }

    pub fn get_pane_manager_mut(&mut self) -> Result<&mut PaneManager> {
        self.panes.get_mut(&self.current_pane).ok_or_eyre("No current pane")
    }

    pub fn enter_terminal(&mut self) -> Result<()> {
        self.get_pane_manager_mut()?.force_goto_by_view(PaneView::Terminal)?;

        Ok(())
    }

    #[allow(dead_code)] // XXX (ZZ): Remove this later after the implementation is done.
    pub fn change_pane_manager(&mut self, name: &str) -> Result<()> {
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
        self.get_pane_manager_mut()?.focus_up()
    }

    pub fn focus_down(&mut self) -> Result<()> {
        self.get_pane_manager_mut()?.focus_down()
    }

    pub fn focus_left(&mut self) -> Result<()> {
        self.get_pane_manager_mut()?.focus_left()
    }

    pub fn focus_right(&mut self) -> Result<()> {
        self.get_pane_manager_mut()?.focus_right()
    }

    pub fn split_focused_pane(&mut self, direction: Direction, ratio: [u32; 2]) -> Result<()> {
        let id = self.get_focused_pane()?.id;
        self.get_pane_manager_mut()?.split(id, direction, ratio)?;
        Ok(())
    }

    fn merge_pane_with_neighbor(&mut self, id: PaneId, direction: BorderSide) -> Result<()> {
        match direction {
            BorderSide::Left => {
                self.focus_left()?;
                let new_id = self.get_focused_pane()?.id;
                if self.get_pane_manager_mut()?.merge(id, new_id).is_ok() {
                    return Ok(());
                }
                self.focus_right()?;
            }
            BorderSide::Right => {
                self.focus_right()?;
                let new_id = self.get_focused_pane()?.id;
                if self.get_pane_manager_mut()?.merge(id, new_id).is_ok() {
                    return Ok(());
                }
                self.focus_left()?;
            }
            BorderSide::Top => {
                self.focus_up()?;
                let new_id = self.get_focused_pane()?.id;
                if self.get_pane_manager_mut()?.merge(id, new_id).is_ok() {
                    return Ok(());
                }
                self.focus_down()?;
            }
            BorderSide::Bottom => {
                self.focus_down()?;
                let new_id = self.get_focused_pane()?.id;
                if self.get_pane_manager_mut()?.merge(id, new_id).is_ok() {
                    return Ok(());
                }
                self.focus_up()?;
            }
        }

        Err(eyre::eyre!("Cannot merge the pane with its neighbor"))
    }

    pub fn close_focused_pane(&mut self) -> Result<()> {
        let pane = self.get_focused_pane()?;
        let id = pane.id;

        // let's try to merge the pane with its neighbor.
        // we may want to first marge the latest split pane.
        let mut indexed_values: Vec<(usize, i32)> = self
            .get_pane_manager()?
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
        Err(RecoverableError::new("The current pane cannot be merged with others due to one of the following reasons:\n\n1. The current pane cannot be merged with any adjacent panes.\n\n2. The current pane contains valid debug views but can only be merged with a Terminal Pane.\n\n3. The current pane has an adjacent pane, but these two are not directly split from the same parent pane.\n\nTo close this pane, you may consider unregistering some views or closing other panes first.").into())
    }

    pub fn get_flattened_layout(&self, app: Rect) -> Result<Vec<PaneFlattened<'_>>> {
        if self.full_screen {
            let pane = self.get_pane_manager()?.get_focused_pane()?;
            Ok(vec![PaneFlattened {
                view: pane.get_current_view(),
                views: pane.get_views(),
                id: pane.id,
                focused: true,
                rect: app,
            }])
        } else {
            Ok(self.get_pane_manager()?.get_flattened_layout(app)?)
        }
    }

    pub fn scale_right(&mut self, amount: u32, screen: Rect) -> Result<()> {
        let pane_manager = self.get_pane_manager_mut()?;
        let id = pane_manager.get_focused_pane()?.id;

        // let's try to scale the pane with its neighbor.
        if pane_manager.scale_pane(id, BorderSide::Right, amount as i32, screen).is_ok() {
            return Ok(());
        }
        if pane_manager.scale_pane(id, BorderSide::Left, amount as i32, screen).is_ok() {
            return Ok(());
        }

        Err(RecoverableError::new("The current pane cannot be scaled to its right side.").into())
    }

    pub fn scale_left(&mut self, amount: u32, screen: Rect) -> Result<()> {
        let pane_manager = self.get_pane_manager_mut()?;
        let id = pane_manager.get_focused_pane()?.id;

        // let's try to scale the pane with its neighbor.
        if pane_manager.scale_pane(id, BorderSide::Left, -(amount as i32), screen).is_ok() {
            return Ok(());
        }
        if pane_manager.scale_pane(id, BorderSide::Right, -(amount as i32), screen).is_ok() {
            return Ok(());
        }

        Err(RecoverableError::new("The current pane cannot be scaled to its left side.").into())
    }

    pub fn scale_down(&mut self, amount: u32, screen: Rect) -> Result<()> {
        let pane_manager = self.get_pane_manager_mut()?;
        let id = pane_manager.get_focused_pane()?.id;

        // let's try to scale the pane with its neighbor.
        if pane_manager.scale_pane(id, BorderSide::Bottom, amount as i32, screen).is_ok() {
            return Ok(());
        }
        if pane_manager.scale_pane(id, BorderSide::Top, amount as i32, screen).is_ok() {
            return Ok(());
        }

        Err(RecoverableError::new("The current pane cannot be scaled to its bottom side.").into())
    }

    pub fn scale_up(&mut self, amount: u32, screen: Rect) -> Result<()> {
        let pane_manager = self.get_pane_manager_mut()?;
        let id = pane_manager.get_focused_pane()?.id;

        // let's try to scale the pane with its neighbor.
        if pane_manager.scale_pane(id, BorderSide::Top, -(amount as i32), screen).is_ok() {
            return Ok(());
        }
        if pane_manager.scale_pane(id, BorderSide::Bottom, -(amount as i32), screen).is_ok() {
            return Ok(());
        }

        Err(RecoverableError::new("The current pane cannot be scale to its top side.").into())
    }
}
