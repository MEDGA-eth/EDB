use std::{collections::BTreeMap, sync::Arc};

use eyre::{ensure, eyre, Result};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::palette::tailwind::VIOLET,
};
use tracing::instrument::WithSubscriber;

use crate::screen::PaneView;

pub type PaneId = usize;

#[derive(Debug)]
pub struct PaneLayout(BTreeMap<PaneId, Rect>);

#[derive(Debug, Clone, Copy, Default)]
struct Point(u16, u16);

/// A virtual rect to help find the focused pane.
const VIRTUAL_RECT: Rect = Rect { x: 0, y: 0, width: 2048, height: 2048 };

#[derive(Debug, Clone, PartialEq)]
pub struct Pane {
    id: PaneId,
    views: Vec<PaneView>,
    current_view: usize,
}

impl Pane {
    pub fn new(id: PaneId) -> Self {
        Pane { id, views: vec![], current_view: 0 }
    }

    pub fn add_view(&mut self, view: PaneView) {
        self.views.push(view);
    }

    pub fn remove_view(&mut self, view: PaneView) {
        self.views.retain(|v| v != &view);
        if self.views.is_empty() {
            self.current_view = 0;
        } else {
            self.current_view %= self.views.len();
        }
    }

    pub fn current_view(&self) -> Option<PaneView> {
        self.views.get(self.current_view).copied()
    }

    pub fn next_view(&mut self) {
        if self.views.is_empty() {
            return;
        }

        self.current_view = (self.current_view + 1) % self.views.len();
    }

    pub fn prev_view(&mut self) {
        if self.views.is_empty() {
            return;
        }

        self.current_view = (self.current_view + self.views.len() - 1) % self.views.len();
    }
}

#[derive(Debug, Clone, PartialEq)]
struct SplitPane {
    input_id: PaneId,
    output_id: [PaneId; 2],
    ratio: [u32; 2],
    direction: Direction,
}

#[derive(Debug, Clone, PartialEq)]
struct MergePane {
    input_id: [PaneId; 2],
    output_id: PaneId,
}

#[derive(Debug, Clone, PartialEq)]
enum PaneOperation {
    Split(SplitPane),
    Merge(MergePane),
}

#[derive(Debug, Clone)]
struct FocusInfo {
    pane_id: PaneId,
    rect: Rect,
}

#[derive(Debug, Clone)]
pub struct PaneManager {
    next_id: PaneId,
    panes: BTreeMap<usize, Pane>,
    operations: Vec<PaneOperation>,

    // This is used to quickly find the pane that contains a view
    view_assignment: BTreeMap<PaneView, PaneId>,

    // A virtual focus point to help find the focused pane
    focus: Point,
    focus_cache: Option<FocusInfo>,
}

impl PaneManager {
    pub fn new(view: PaneView) -> Self {
        let mut pane = Pane::new(1); // we start with 1;
        pane.add_view(view);

        let mut panes = BTreeMap::new();
        panes.insert(pane.id, pane);

        PaneManager {
            next_id: 2,
            panes,
            operations: Vec::new(),
            view_assignment: BTreeMap::new(),
            focus: Point::new(0, 0),
            focus_cache: None,
        }
    }

    /// The large screen layout is:
    /// ```text
    /// +----------+-----------+-----------+
    /// | 1. Trace | 3. Source | 4. opcode |
    /// +----------+-------+---+-----------+
    /// | 2. Terminal Pane | 5. Data Pane  |
    /// +------------------+---------------+
    /// ```
    pub fn default_large_screen() -> Result<Self> {
        let mut manager = PaneManager::new(PaneView::Source);

        manager.split(1, Direction::Horizontal, [3, 2])?;
        manager.split(1, Direction::Vertical, [4, 1])?;
        manager.split(3, Direction::Vertical, [2, 1])?;
        manager.split(2, Direction::Vertical, [1, 1])?;

        manager.assign(PaneView::Trace, 1)?;
        manager.assign(PaneView::Source, 3)?;
        manager.assign(PaneView::Opcode, 4)?;

        manager.assign(PaneView::Variable, 5)?;
        manager.assign(PaneView::Expression, 5)?;
        manager.assign(PaneView::Stack, 5)?;
        manager.assign(PaneView::Memory, 5)?;
        manager.assign(PaneView::Calldata, 5)?;
        manager.assign(PaneView::Returndata, 5)?;

        manager.assign(PaneView::Terminal, 2)?;

        Ok(manager)
    }

    /// The small screen layout is:
    /// ```text
    /// +-----------------+-------+
    /// | 1: CodePane     | 2:    |
    /// +-----------------+   op  |
    /// | 3: DataPane     |  code |
    /// +-----------------+  list |
    /// | 4: TerminalPane |       |
    /// +-----------------+-------+
    /// ```
    pub fn default_small_screen() -> Result<Self> {
        let mut manager = PaneManager::new(PaneView::Source);

        manager.split(1, Direction::Vertical, [3, 1])?;
        manager.split(1, Direction::Horizontal, [3, 4])?;
        manager.split(3, Direction::Horizontal, [3, 1])?;

        manager.assign(PaneView::Trace, 1)?;
        manager.assign(PaneView::Source, 1)?;

        manager.assign(PaneView::Opcode, 2)?;

        manager.assign(PaneView::Variable, 3)?;
        manager.assign(PaneView::Expression, 3)?;
        manager.assign(PaneView::Stack, 3)?;
        manager.assign(PaneView::Memory, 3)?;
        manager.assign(PaneView::Calldata, 3)?;
        manager.assign(PaneView::Returndata, 3)?;

        manager.assign(PaneView::Terminal, 4)?;

        Ok(manager)
    }

    pub fn assign(&mut self, view: PaneView, target: PaneId) -> Result<()> {
        if let Some(old_pane_id) = self.view_assignment.insert(view, target) {
            let old_pane = self.panes.get_mut(&old_pane_id).ok_or(eyre::eyre!("Pane not found"))?;
            old_pane.remove_view(view);
        }

        let pane = self.panes.get_mut(&target).ok_or(eyre::eyre!("Pane not found"))?;
        pane.add_view(view);

        Ok(())
    }

    pub fn split_by_view(
        &mut self,
        view: PaneView,
        direction: Direction,
        ratio: [u32; 2],
    ) -> Result<usize> {
        let target_id =
            self.view_assignment.get(&view).copied().ok_or(eyre::eyre!("Pane not found"))?;
        self.split(target_id, direction, ratio)
    }

    pub fn merge_by_view(&mut self, input1: PaneView, input2: PaneView) -> Result<usize> {
        let input1_id =
            self.view_assignment.get(&input1).copied().ok_or(eyre::eyre!("Pane not found"))?;
        let input2_id =
            self.view_assignment.get(&input2).copied().ok_or(eyre::eyre!("Pane not found"))?;
        self.merge(input1_id, input2_id)
    }

    pub fn merge(&mut self, id1: PaneId, id2: PaneId) -> Result<usize> {
        // remove the focus cache
        self.focus_cache = None;

        let layout = self.get_screen_layout(VIRTUAL_RECT)?;
        let rect1 = layout.0.get(&id1).ok_or(eyre::eyre!("Pane not found"))?;
        let rect2 = layout.0.get(&id2).ok_or(eyre::eyre!("Pane not found"))?;

        // we should first check if the two panes are adjacent
        if rect1.width == rect2.width {
            if rect1.bottom() != rect2.top() && rect1.top() != rect2.bottom() {
                return Err(eyre::eyre!("Panes are not adjacent"));
            }
        } else if rect1.height == rect2.height {
            if rect1.right() != rect2.left() && rect1.left() != rect2.right() {
                return Err(eyre::eyre!("Panes are not adjacent"));
            }
        } else {
            return Err(eyre::eyre!("Panes are not adjacent"));
        }

        let target1 = self.panes.get(&id1).ok_or(eyre::eyre!("Pane not found"))?;
        let target2 = self.panes.get(&id2).ok_or(eyre::eyre!("Pane not found"))?;

        let new_id = self.next_id;
        self.next_id += 1;

        let merge = MergePane { input_id: [id1, id2], output_id: new_id };

        self.operations.push(PaneOperation::Merge(merge));

        let new_pane = Pane::new(new_id);
        let new_pane_views: Vec<_> =
            target1.views.iter().chain(target2.views.iter()).cloned().collect();
        self.panes.insert(new_id, new_pane);

        for view in new_pane_views {
            self.assign(view, new_id)?;
        }

        Ok(new_id)
    }

    pub fn split(&mut self, id: PaneId, direction: Direction, ratio: [u32; 2]) -> Result<usize> {
        // remove the focus cache
        self.focus_cache = None;

        let _ = self.panes.get(&id).ok_or(eyre::eyre!("Pane not found"))?;

        let new_id = self.next_id;
        self.next_id += 1;

        let split = SplitPane { input_id: id, output_id: [new_id, new_id], ratio, direction };

        self.operations.push(PaneOperation::Split(split));

        let new_pane = Pane::new(new_id);
        self.panes.insert(new_id, new_pane);

        Ok(new_id)
    }

    pub fn focus_up(&mut self) -> Result<()> {
        let rect = self.get_focused_info()?.rect;
        ensure!(self.focus.in_rect(rect), "focus is not in the focused pane");

        self.focus.1 = (rect.top() + VIRTUAL_RECT.height - 1) % VIRTUAL_RECT.height;
        Ok(())
    }

    pub fn focus_down(&mut self) -> Result<()> {
        let rect = self.get_focused_info()?.rect;
        ensure!(self.focus.in_rect(rect), "focus is not in the focused pane");

        self.focus.1 = rect.bottom() % VIRTUAL_RECT.height;
        Ok(())
    }

    pub fn focus_left(&mut self) -> Result<()> {
        let rect = self.get_focused_info()?.rect;
        ensure!(self.focus.in_rect(rect), "focus is not in the focused pane");

        self.focus.0 = (rect.left() + VIRTUAL_RECT.width - 1) % VIRTUAL_RECT.width;
        Ok(())
    }

    pub fn focus_right(&mut self) -> Result<()> {
        let rect = self.get_focused_info()?.rect;
        ensure!(self.focus.in_rect(rect), "focus is not in the focused pane");

        self.focus.0 = rect.right() % VIRTUAL_RECT.width;
        Ok(())
    }

    pub fn get_focused_view(&mut self) -> Result<Option<PaneView>> {
        let info = self.get_focused_info()?;
        let pane = self.panes.get(&info.pane_id).ok_or(eyre!("invalid pane id"))?;
        Ok(pane.current_view())
    }

    fn get_focused_info(&mut self) -> Result<FocusInfo> {
        if let Some(info) = &self.focus_cache {
            return Ok(info.clone());
        }

        let layout = self.get_screen_layout(VIRTUAL_RECT)?;
        for (id, rect) in layout.0.iter() {
            if self.focus.in_rect(*rect) {
                ensure!(self.panes.contains_key(id), "Pane not found");
                let info = FocusInfo { pane_id: *id, rect: *rect };
                self.focus_cache = Some(info.clone());
                return Ok(info);
            }
        }

        Err(eyre::eyre!("No pane found at {:?}", self.focus))
    }

    pub fn get_pane_id(&self, view: PaneView) -> Option<PaneId> {
        self.view_assignment.get(&view).copied()
    }

    pub fn get_screen_layout(&self, app: Rect) -> Result<PaneLayout> {
        let mut layout = BTreeMap::new();
        layout.insert(1usize, app);

        for op in &self.operations {
            match op {
                PaneOperation::Split(split) => {
                    let input = layout.get(&split.input_id).ok_or(eyre::eyre!("Pane not found"))?;
                    let t_r = split.ratio[0] + split.ratio[1];
                    let [output1, output2] = Layout::new(
                        split.direction,
                        [
                            Constraint::Ratio(split.ratio[0], t_r),
                            Constraint::Ratio(split.ratio[1], t_r),
                        ],
                    )
                    .split(*input)[..] else {
                        return Err(eyre::eyre!("unreachable code"));
                    };

                    layout.remove(&split.input_id);
                    layout.insert(split.output_id[0], output1);
                    layout.insert(split.output_id[1], output2);
                }
                PaneOperation::Merge(merge) => {
                    let input1 =
                        layout.get(&merge.input_id[0]).ok_or(eyre::eyre!("Pane not found"))?;
                    let input2 =
                        layout.get(&merge.input_id[1]).ok_or(eyre::eyre!("Pane not found"))?;
                    let output = input1.union(*input2);

                    layout.remove(&merge.input_id[0]);
                    layout.remove(&merge.input_id[1]);
                    layout.insert(merge.output_id, output);
                }
            }
        }

        Ok(PaneLayout(layout))
    }
}

impl PaneLayout {
    pub fn flatten<'a>(&self, manager: &PaneManager) -> Vec<(Rect, Option<PaneView>)> {
        self.0
            .iter()
            .map(|(id, rect)| {
                let view = manager.panes.get(&id).and_then(|pane| pane.current_view());
                (*rect, view)
            })
            .collect()
    }
}

impl Point {
    pub fn new(x: u16, y: u16) -> Self {
        Point(x, y)
    }

    pub fn in_rect(&self, rect: Rect) -> bool {
        self.0 >= rect.left() &&
            self.0 < rect.right() &&
            self.1 >= rect.top() &&
            self.1 < rect.bottom()
    }
}
