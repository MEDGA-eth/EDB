use std::collections::BTreeMap;

use eyre::{ensure, eyre, Result};
use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub type PaneId = usize;

/// Used to keep track of which kind of pane is currently active
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq)]
pub enum PaneView {
    // terminal
    Terminal,

    // code
    Trace,
    Source,
    Opcode,

    // data
    Variable,
    Expression,
    Memory,
    Calldata,
    Returndata,
    Stack,

    // null
    Null,
}

impl ToString for PaneView {
    fn to_string(&self) -> String {
        match self {
            PaneView::Terminal => "Script Terminal".to_string(),
            PaneView::Trace => "Call Trace".to_string(),
            PaneView::Source => "Source Code".to_string(),
            PaneView::Opcode => "Opcode".to_string(),
            PaneView::Variable => "Variables".to_string(),
            PaneView::Expression => "Customized Watchers".to_string(),
            PaneView::Memory => "Memory".to_string(),
            PaneView::Calldata => "Calldata".to_string(),
            PaneView::Returndata => "Returndata".to_string(),
            PaneView::Stack => "Stack".to_string(),
            PaneView::Null => "Null".to_string(),
        }
    }
}

impl From<u8> for PaneView {
    fn from(value: u8) -> Self {
        match value {
            0 => PaneView::Terminal,
            1 => PaneView::Trace,
            2 => PaneView::Source,
            3 => PaneView::Opcode,
            4 => PaneView::Variable,
            5 => PaneView::Expression,
            6 => PaneView::Memory,
            7 => PaneView::Calldata,
            8 => PaneView::Returndata,
            9 => PaneView::Stack,
            _ => PaneView::Null,
        }
    }
}

impl PaneView {
    pub fn is_valid(&self) -> bool {
        !matches!(self, PaneView::Null)
    }

    pub fn num_of_valid_views() -> u8 {
        10
    }
}

#[derive(Debug, Clone)]
pub struct PaneLayout(BTreeMap<PaneId, Rect>);

#[derive(Debug, Clone, Copy, Default)]
pub struct Point(u16, u16);

/// A virtual rect to help find the focused pane.
const VIRTUAL_RECT: Rect = Rect { x: 0, y: 0, width: 2048, height: 2048 };

#[derive(Debug, Clone)]
pub struct Pane {
    pub id: PaneId,
    views: Vec<PaneView>,
    current_view: usize,
}

#[derive(Debug, Clone)]
pub struct PaneFlattened {
    pub rect: Rect,
    pub view: PaneView,
    pub focused: bool,
    pub id: PaneId,
}

impl Pane {
    pub fn new(id: PaneId) -> Self {
        Pane { id, views: vec![], current_view: 0 }
    }

    pub fn add_view(&mut self, view: PaneView) -> Result<()> {
        // We will ensure that Terminal is the only view in a pane
        if view == PaneView::Terminal {
            // let's first check whether Terminal is here
            if self.views.iter().any(|v| *v != PaneView::Terminal) {
                return Err(eyre::eyre!("terminal has to be the only view in a pane"));
            }

            // we then push update the view if Terminal is not here
            if self.views.is_empty() {
                self.views.push(view);
            }
        } else if !self.views.contains(&view) {
            if self.views.contains(&PaneView::Terminal) {
                return Err(eyre::eyre!("terminal has to be the only view in a pane"));
            }
            self.views.push(view);
        }

        Ok(())
    }

    pub fn remove_view(&mut self, view: PaneView) {
        self.views.retain(|v| v != &view);
        if self.views.is_empty() {
            self.current_view = 0;
        } else {
            self.current_view %= self.views.len();
        }
    }

    pub fn len(&self) -> usize {
        self.views.len()
    }

    pub fn get_current_view(&self) -> PaneView {
        *self.views.get(self.current_view).unwrap_or(&PaneView::Null)
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

    pub fn select_view(&mut self, view: PaneView) {
        if let Some(index) = self.views.iter().position(|v| v == &view) {
            self.current_view = index;
        }
    }

    pub fn has_view(&self, view: &PaneView) -> bool {
        self.views.contains(view)
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
struct SwitchPaneId {
    input_id: [PaneId; 2],
}

#[derive(Debug, Clone, PartialEq)]
enum PaneOperation {
    Split(SplitPane),
    Merge(MergePane),
    SwitchId(SwitchPaneId),
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
}

impl Default for PaneManager {
    fn default() -> Self {
        PaneManager::new()
    }
}

impl PaneManager {
    pub fn new() -> Self {
        let pane = Pane::new(1); // we start with 1;

        let mut panes = BTreeMap::new();
        panes.insert(pane.id, pane);

        PaneManager {
            next_id: 2,
            panes,
            operations: Vec::new(),
            view_assignment: BTreeMap::new(),
            focus: Point::new(0, 0),
        }
    }

    /// The large screen layout is:
    /// ```text
    /// +----------+-----------+-----------+
    /// | 1. Trace | 2. Source | 3. opcode |
    /// +----------+-------+---+-----------+
    /// | 4. Terminal Pane | 5. Data Pane  |
    /// +------------------+---------------+
    /// ```
    pub fn default_large_screen() -> Result<Self> {
        let mut manager = PaneManager::new();

        manager.split(1, Direction::Vertical, [3, 2])?;
        manager.split(1, Direction::Horizontal, [2, 3])?;
        manager.split(3, Direction::Horizontal, [2, 1])?;
        manager.split(2, Direction::Horizontal, [1, 1])?;

        // The current layout:
        // ```test
        // +----------+-----------+-----------+
        // | 1. Trace | 3. Source | 4. opcode |
        // +----------+-------+---+-----------+
        // | 2. Terminal Pane | 5. Data Pane  |
        // +------------------+---------------+
        // ```
        manager.switch_id(2, 3)?;
        manager.switch_id(3, 4)?;

        manager.assign(PaneView::Trace, 1)?;
        manager.assign(PaneView::Source, 2)?;
        manager.assign(PaneView::Opcode, 3)?;

        manager.assign(PaneView::Variable, 5)?;
        manager.assign(PaneView::Expression, 5)?;
        manager.assign(PaneView::Stack, 5)?;
        manager.assign(PaneView::Memory, 5)?;
        manager.assign(PaneView::Calldata, 5)?;
        manager.assign(PaneView::Returndata, 5)?;

        manager.assign(PaneView::Terminal, 4)?;

        Ok(manager)
    }

    /// The small screen layout is:
    /// ```text
    /// +-----------------+-------+
    /// | 1: CodePane     | 4:    |
    /// +-----------------+   op  |
    /// | 2: DataPane     |  code |
    /// +-----------------+  list |
    /// | 3: TerminalPane |       |
    /// +-----------------+-------+
    /// ```
    pub fn default_small_screen() -> Result<Self> {
        let mut manager = PaneManager::new();

        manager.split(1, Direction::Horizontal, [3, 1])?;
        manager.split(1, Direction::Vertical, [3, 4])?;
        manager.split(3, Direction::Vertical, [3, 1])?;

        // The curent layout is:
        // ```text
        // +-----------------+-------+
        // | 1: CodePane     | 2:    |
        // +-----------------+   op  |
        // | 3: DataPane     |  code |
        // +-----------------+  list |
        // | 4: TerminalPane |       |
        // +-----------------+-------+
        // ```
        manager.switch_id(2, 3)?;
        manager.switch_id(3, 4)?;

        manager.assign(PaneView::Trace, 1)?;
        manager.assign(PaneView::Source, 1)?;

        manager.assign(PaneView::Opcode, 4)?;

        manager.assign(PaneView::Variable, 2)?;
        manager.assign(PaneView::Expression, 2)?;
        manager.assign(PaneView::Stack, 2)?;
        manager.assign(PaneView::Memory, 2)?;
        manager.assign(PaneView::Calldata, 2)?;
        manager.assign(PaneView::Returndata, 2)?;

        manager.assign(PaneView::Terminal, 3)?;

        Ok(manager)
    }

    pub fn assign(&mut self, view: PaneView, target: PaneId) -> Result<()> {
        ensure!(view.is_valid(), "invalid view");

        // first check whether the view can be added
        let pane = self.panes.get_mut(&target).ok_or(eyre::eyre!("pane not found (assign)"))?;
        pane.add_view(view)?;

        // update the view assignment
        if let Some(old_pane_id) = self.view_assignment.insert(view, target) {
            if old_pane_id == target {
                return Ok(());
            }

            let old_pane =
                self.panes.get_mut(&old_pane_id).ok_or(eyre::eyre!("pane not found (assign)"))?;
            old_pane.remove_view(view);
        }

        Ok(())
    }

    pub fn unassign(&mut self, view: PaneView) -> Result<()> {
        ensure!(view.is_valid(), "invalid view");
        if let Some(id) = self.view_assignment.remove(&view) {
            let pane = self.panes.get_mut(&id).ok_or(eyre::eyre!("pane not found (unassign)"))?;
            pane.remove_view(view);
        }

        Ok(())
    }

    pub fn split_by_view(
        &mut self,
        view: PaneView,
        direction: Direction,
        ratio: [u32; 2],
    ) -> Result<usize> {
        let target_id = self
            .view_assignment
            .get(&view)
            .copied()
            .ok_or(eyre::eyre!("pane not found (split_by_view)"))?;
        self.split(target_id, direction, ratio)
    }

    pub fn merge_by_view(&mut self, input1: PaneView, input2: PaneView) -> Result<usize> {
        let input1_id = self
            .view_assignment
            .get(&input1)
            .copied()
            .ok_or(eyre::eyre!("pane not found (merge_by_view)"))?;
        let input2_id = self
            .view_assignment
            .get(&input2)
            .copied()
            .ok_or(eyre::eyre!("pane not found (merge_by_view)"))?;
        self.merge(input1_id, input2_id)
    }

    pub fn merge(&mut self, id1: PaneId, id2: PaneId) -> Result<usize> {
        let (id1, id2) = if id1 < id2 { (id1, id2) } else { (id2, id1) };

        let layout = self.get_layout(VIRTUAL_RECT)?;
        let rect1 = layout.0.get(&id1).ok_or(eyre::eyre!("pane not found (merge)"))?;
        let rect2 = layout.0.get(&id2).ok_or(eyre::eyre!("pane not found (merge)"))?;

        // we should first check if the two panes are adjacent
        if !mergeable(rect1, rect2) {
            return Err(eyre::eyre!("panes are not adjacent"));
        }

        let target1 = self.panes.get(&id1).ok_or(eyre::eyre!("pane not found (merge)"))?;
        let target2 = self.panes.get(&id2).ok_or(eyre::eyre!("pane not found (merge)"))?;

        let new_id = id1;

        let mut new_pane = Pane::new(new_id);
        for view in target1.views.iter().chain(target2.views.iter()) {
            new_pane.add_view(*view)?;
        }

        self.panes.remove(&id2);
        self.panes.insert(new_id, new_pane);

        let merge = MergePane { input_id: [id1, id2], output_id: new_id };
        self.operations.push(PaneOperation::Merge(merge));

        Ok(new_id)
    }

    pub fn split(&mut self, id: PaneId, direction: Direction, ratio: [u32; 2]) -> Result<usize> {
        let _ = self.panes.get(&id).ok_or(eyre::eyre!("pane not found (split)"))?;

        let new_id = self.next_id;
        self.next_id += 1;

        let split = SplitPane { input_id: id, output_id: [id, new_id], ratio, direction };

        let new_pane = Pane::new(new_id);
        self.panes.insert(new_id, new_pane);

        self.operations.push(PaneOperation::Split(split));

        Ok(new_id)
    }

    // Switch the pane ids
    pub fn switch_id(&mut self, input1: PaneId, input2: PaneId) -> Result<()> {
        let _ = self.panes.get(&input1).ok_or(eyre::eyre!("pane not found (switch_id)"))?;
        let _ = self.panes.get(&input2).ok_or(eyre::eyre!("pane not found (switch_id)"))?;

        let switch = SwitchPaneId { input_id: [input1, input2] };
        self.operations.push(PaneOperation::SwitchId(switch));

        Ok(())
    }

    /// Force the focus to a specific point. Make sure the point is on the focused pane.
    pub fn force_goto(&mut self, point: Point) {
        self.focus = point;
    }

    pub fn force_goto_by_view(&mut self, view: PaneView) -> Result<()> {
        let target_id = self.get_pane_id(view).ok_or(eyre::eyre!("pane not found (force_goto)"))?;
        let layout = self.get_layout(VIRTUAL_RECT)?;

        let rect = layout.0.get(&target_id).ok_or(eyre::eyre!("pane not found (force_goto)"))?;
        self.focus = Point::new(rect.x, rect.y);

        let pane =
            self.panes.get_mut(&target_id).ok_or(eyre::eyre!("pane not found (force_goto)"))?;
        pane.select_view(view);

        Ok(())
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

    pub fn get_focused_pane_mut(&mut self) -> Result<&mut Pane> {
        let info = self.get_focused_info()?;
        Ok(self.panes.get_mut(&info.pane_id).ok_or(eyre!("invalid pane id"))?)
    }

    pub fn get_focused_pane(&self) -> Result<&Pane> {
        let info = self.get_focused_info()?;
        Ok(self.panes.get(&info.pane_id).ok_or(eyre!("invalid pane id"))?)
    }

    pub fn get_focused_view(&self) -> Result<PaneView> {
        let pane = self.get_focused_pane()?;
        Ok(pane.get_current_view())
    }

    fn get_focused_info(&self) -> Result<FocusInfo> {
        let layout = self.get_layout(VIRTUAL_RECT)?;
        for (id, rect) in layout.0.iter() {
            if self.focus.in_rect(*rect) {
                ensure!(self.panes.contains_key(id), "pane not found (get_focused_info)");
                let info = FocusInfo { pane_id: *id, rect: *rect };
                return Ok(info);
            }
        }

        Err(eyre::eyre!("No pane found at {:?}", self.focus))
    }

    pub fn get_pane_id(&self, view: PaneView) -> Option<PaneId> {
        self.view_assignment.get(&view).copied()
    }

    pub fn get_flattened_layout(&self, app: Rect) -> Result<Vec<PaneFlattened>> {
        let layout = self.get_layout(app)?;
        let focus_info = self.get_focused_info()?;
        Ok(layout
            .0
            .iter()
            .map(|(id, rect)| {
                let pane = self
                    .panes
                    .get(&id)
                    .ok_or_else(|| eyre::eyre!("pane not found (get_flattened_layout)"))?;
                Ok(PaneFlattened {
                    rect: *rect,
                    view: pane.get_current_view(),
                    focused: focus_info.pane_id == *id,
                    id: *id,
                })
            })
            .collect::<Result<Vec<PaneFlattened>>>()?)
    }

    pub fn get_layout(&self, app: Rect) -> Result<PaneLayout> {
        let mut layout = BTreeMap::new();
        layout.insert(1usize, app);

        for op in &self.operations {
            match op {
                PaneOperation::Split(split) => {
                    let input = layout
                        .get(&split.input_id)
                        .ok_or(eyre::eyre!("pane not found (get_layout)"))?;
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
                    let input1 = layout
                        .get(&merge.input_id[0])
                        .ok_or(eyre::eyre!("pane not found (get_layout)"))?;
                    let input2 = layout
                        .get(&merge.input_id[1])
                        .ok_or(eyre::eyre!("pane not found (get_layout)"))?;
                    let output = input1.union(*input2);

                    layout.remove(&merge.input_id[0]);
                    layout.remove(&merge.input_id[1]);
                    layout.insert(merge.output_id, output);
                }
                PaneOperation::SwitchId(switch) => {
                    let input1 = *layout
                        .get(&switch.input_id[0])
                        .ok_or(eyre::eyre!("pane not found (get_layout)"))?;
                    let input2 = *layout
                        .get(&switch.input_id[1])
                        .ok_or(eyre::eyre!("pane not found (get_layout)"))?;

                    layout.insert(switch.input_id[0], input2);
                    layout.insert(switch.input_id[1], input1);
                }
            }
        }

        Ok(PaneLayout(layout))
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

    pub fn project(&self, rect: Rect) -> Point {
        Point::new(
            ((self.0 as u64) * (VIRTUAL_RECT.width as u64) / (rect.width as u64)) as u16,
            ((self.1 as u64) * (VIRTUAL_RECT.height as u64) / (rect.height as u64)) as u16,
        )
    }
}

#[inline]
fn mergeable(rect1: &Rect, rect2: &Rect) -> bool {
    if rect1.width == rect2.width {
        if rect1.bottom() == rect2.top() || rect1.top() == rect2.bottom() {
            return true;
        }
    }

    if rect1.height == rect2.height {
        if rect1.right() == rect2.left() || rect1.left() == rect2.right() {
            return true;
        }
    }

    false
}
