use std::{collections::BTreeMap, fmt::Display};

use eyre::{ensure, OptionExt, Result};
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::context::RecoverableError;

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

impl Display for PaneView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Terminal => write!(f, "Terminal"),
            Self::Trace => write!(f, "Call Trace"),
            Self::Source => write!(f, "Source Code"),
            Self::Opcode => write!(f, "Opcode"),
            Self::Variable => write!(f, "Variables"),
            Self::Expression => write!(f, "Watchers"),
            Self::Memory => write!(f, "Memory"),
            Self::Calldata => write!(f, "Calldata"),
            Self::Returndata => write!(f, "Returndata"),
            Self::Stack => write!(f, "Stack"),
            Self::Null => write!(f, "Null"),
        }
    }
}

impl From<u8> for PaneView {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Terminal,
            1 => Self::Trace,
            2 => Self::Source,
            3 => Self::Opcode,
            4 => Self::Variable,
            5 => Self::Expression,
            6 => Self::Memory,
            7 => Self::Calldata,
            8 => Self::Returndata,
            9 => Self::Stack,
            _ => Self::Null,
        }
    }
}

impl PaneView {
    pub fn is_valid(&self) -> bool {
        !matches!(self, Self::Null)
    }

    pub fn num_of_valid_views() -> u8 {
        10
    }
}

#[derive(Debug, Clone)]
pub struct PaneLayout(BTreeMap<PaneId, Rect>);

#[derive(Debug, Clone, Copy, Default)]
pub struct VirtCoord(u16, u16);

/// A virtual rect to help find the focused pane.
pub const VIRTUAL_RECT: Rect = Rect { x: 0, y: 0, width: 2048, height: 2048 };

/// 1/4 of the screen size
const MIN_SPLITABLE_PANE_SIZE: u16 = 512;
const MIN_PANE_SIZE: u16 = MIN_SPLITABLE_PANE_SIZE / 2;

#[derive(Debug, Clone)]
pub struct Pane {
    pub id: PaneId,
    views: Vec<PaneView>,
    current_view: usize,
}

#[derive(Debug, Clone)]
pub struct PaneFlattened<'a> {
    pub views: &'a Vec<PaneView>,
    pub rect: Rect,
    pub view: PaneView,
    pub focused: bool,
    pub id: PaneId,
}

impl Pane {
    pub fn new(id: PaneId) -> Self {
        Self { id, views: vec![], current_view: 0 }
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

    pub fn get_views(&self) -> &Vec<PaneView> {
        &self.views
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
    side_len: u16,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderSide {
    Top,
    Bottom,
    Left,
    Right,
}

impl TryFrom<usize> for BorderSide {
    type Error = eyre::Error;

    fn try_from(value: usize) -> Result<Self> {
        match value {
            0 => Ok(Self::Top),
            1 => Ok(Self::Bottom),
            2 => Ok(Self::Left),
            3 => Ok(Self::Right),
            _ => Err(eyre::eyre!("invalid border side")),
        }
    }
}

impl From<BorderSide> for usize {
    fn from(value: BorderSide) -> Self {
        match value {
            BorderSide::Top => 0,
            BorderSide::Bottom => 1,
            BorderSide::Left => 2,
            BorderSide::Right => 3,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaneManager {
    next_id: PaneId,
    panes: BTreeMap<usize, Pane>,
    operations: Vec<PaneOperation>,

    // This is used to quickly find the pane that contains a view
    view_assignment: BTreeMap<PaneView, PaneId>,

    // A virtual focus point to help find the focused pane
    focus: VirtCoord,

    // The creator (operation id) of each border
    borders: BTreeMap<PaneId, [Option<usize>; 4]>,
}

impl Default for PaneManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneManager {
    pub fn new() -> Self {
        let pane = Pane::new(1); // we start with 1;

        let mut panes = BTreeMap::new();
        panes.insert(pane.id, pane);

        let mut borders = BTreeMap::new();
        borders.insert(1, [None; 4]);

        Self {
            next_id: 2,
            panes,
            operations: Vec::new(),
            view_assignment: BTreeMap::new(),
            focus: VirtCoord::new(0, 0),
            borders,
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
        let mut manager = Self::new();

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
        let mut manager = Self::new();

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
        let pane = self.panes.get_mut(&target).ok_or_eyre("pane not found (assign)")?;
        pane.add_view(view)?;

        // update the view assignment
        if let Some(old_pane_id) = self.view_assignment.insert(view, target) {
            if old_pane_id == target {
                return Ok(());
            }

            let old_pane =
                self.panes.get_mut(&old_pane_id).ok_or_eyre("pane not found (assign)")?;
            old_pane.remove_view(view);
        }

        Ok(())
    }

    pub fn get_borders(&self, id: PaneId) -> Result<[Option<usize>; 4]> {
        self.borders.get(&id).copied().ok_or_eyre("pane not found (get_borders)")
    }

    pub fn unassign(&mut self, view: PaneView) -> Result<()> {
        ensure!(view.is_valid(), "invalid view");
        if let Some(id) = self.view_assignment.remove(&view) {
            let pane = self.panes.get_mut(&id).ok_or_eyre("pane not found (unassign)")?;
            pane.remove_view(view);
        }

        Ok(())
    }

    pub fn merge(&mut self, id1: PaneId, id2: PaneId) -> Result<usize> {
        let (id1, id2) = if id1 < id2 { (id1, id2) } else { (id2, id1) };

        let layout = self.get_layout(VIRTUAL_RECT)?;
        let rect1 = layout.0.get(&id1).ok_or_eyre("pane not found (merge)")?;
        let rect2 = layout.0.get(&id2).ok_or_eyre("pane not found (merge)")?;

        // We should first check if the two panes are adjacent.
        // We will also check the rect1 is on which side of rect2
        let side = mergeable(rect1, rect2)?;

        let target1 = self.panes.get(&id1).ok_or_eyre("pane not found (merge)")?;
        let target2 = self.panes.get(&id2).ok_or_eyre("pane not found (merge)")?;

        // We also need to make sure the two panes are from the same parent pane
        let borders1 = self.borders.get(&id1).ok_or_eyre("pane not found (merge)")?;
        let borders2 = self.borders.get(&id2).ok_or_eyre("pane not found (merge)")?;
        let new_borders = match side {
            BorderSide::Left | BorderSide::Right => {
                if borders1[usize::from(BorderSide::Top)] != borders2[usize::from(BorderSide::Top)]
                {
                    return Err(RecoverableError::new(
                        "the two panes are not from the same parent pane".to_string(),
                    )
                    .into());
                }
                if borders1[usize::from(BorderSide::Bottom)] !=
                    borders2[usize::from(BorderSide::Bottom)]
                {
                    return Err(RecoverableError::new(
                        "the two panes are not from the same parent pane".to_string(),
                    )
                    .into());
                }

                let mut new_borders = *borders1;
                if side == BorderSide::Left {
                    new_borders[usize::from(BorderSide::Right)] =
                        borders2[usize::from(BorderSide::Right)];
                } else {
                    new_borders[usize::from(BorderSide::Left)] =
                        borders2[usize::from(BorderSide::Left)];
                }
                new_borders
            }
            BorderSide::Top | BorderSide::Bottom => {
                if borders1[usize::from(BorderSide::Left)] !=
                    borders2[usize::from(BorderSide::Left)]
                {
                    return Err(RecoverableError::new(
                        "the two panes are not from the same parent pane".to_string(),
                    )
                    .into());
                }
                if borders1[usize::from(BorderSide::Right)] !=
                    borders2[usize::from(BorderSide::Right)]
                {
                    return Err(RecoverableError::new(
                        "the two panes are not from the same parent pane".to_string(),
                    )
                    .into());
                }

                let mut new_borders = *borders1;
                if side == BorderSide::Top {
                    new_borders[usize::from(BorderSide::Bottom)] =
                        borders2[usize::from(BorderSide::Bottom)];
                } else {
                    new_borders[usize::from(BorderSide::Top)] =
                        borders2[usize::from(BorderSide::Top)];
                }
                new_borders
            }
        };

        let new_id = id1;

        let mut new_pane = Pane::new(new_id);
        for view in target1.views.iter().chain(target2.views.iter()) {
            new_pane.add_view(*view)?;
        }

        // merge needs to update the view assignment
        for view in &new_pane.views {
            self.view_assignment.insert(*view, new_id);
        }

        self.panes.remove(&id2);
        self.panes.insert(new_id, new_pane);

        self.borders.remove(&id2);
        self.borders.insert(new_id, new_borders);

        let merge = MergePane { input_id: [id1, id2], output_id: new_id };
        self.operations.push(PaneOperation::Merge(merge));

        Ok(new_id)
    }

    pub fn split(&mut self, id: PaneId, direction: Direction, ratio: [u32; 2]) -> Result<usize> {
        let _ = self.panes.get(&id).ok_or_eyre("pane not found (split)")?;
        let layout = self.get_layout(VIRTUAL_RECT)?;
        let rect = layout.0.get(&id).ok_or_eyre("pane not found")?;
        let side_len = match direction {
            Direction::Horizontal => rect.width,
            Direction::Vertical => rect.height,
        };

        if side_len < MIN_SPLITABLE_PANE_SIZE {
            return Err(RecoverableError::new("pane is too small to split".to_string()).into());
        }

        let new_id = self.next_id;
        self.next_id += 1;

        // id is the left pane and new_id is the right pane
        let split = SplitPane { input_id: id, output_id: [id, new_id], ratio, direction, side_len };

        // update the panes
        let new_pane = Pane::new(new_id);
        self.panes.insert(new_id, new_pane);

        // update the border info
        let id1_borders = self.borders.get_mut(&id).ok_or_eyre("pane not found (split)")?;
        let mut id2_borders = *id1_borders;

        match direction {
            Direction::Vertical => {
                id1_borders[usize::from(BorderSide::Bottom)] = Some(self.operations.len());
                id2_borders[usize::from(BorderSide::Top)] = Some(self.operations.len());
            }
            Direction::Horizontal => {
                id1_borders[usize::from(BorderSide::Right)] = Some(self.operations.len());
                id2_borders[usize::from(BorderSide::Left)] = Some(self.operations.len());
            }
        }
        self.borders.insert(new_id, id2_borders);

        self.operations.push(PaneOperation::Split(split));

        Ok(new_id)
    }

    // Switch the pane ids
    pub fn switch_id(&mut self, input1: PaneId, input2: PaneId) -> Result<()> {
        let _ = self.panes.get(&input1).ok_or_eyre("pane not found (switch_id)")?;
        let _ = self.panes.get(&input2).ok_or_eyre("pane not found (switch_id)")?;

        // remember to update borders
        let id1_borders = self.borders.remove(&input1).ok_or_eyre("pane not found (switch_id)")?;
        let id2_borders = self.borders.remove(&input2).ok_or_eyre("pane not found (switch_id)")?;
        self.borders.insert(input1, id2_borders);
        self.borders.insert(input2, id1_borders);

        let switch = SwitchPaneId { input_id: [input1, input2] };
        self.operations.push(PaneOperation::SwitchId(switch));

        Ok(())
    }

    /// Force the focus to a specific point. Make sure the point is on the focused pane.
    pub fn force_goto(&mut self, point: VirtCoord) {
        self.focus = point;
    }

    pub fn force_goto_by_view(&mut self, view: PaneView) -> Result<()> {
        let target_id = self.get_pane_id(view).ok_or_eyre("pane not found (force_goto)")?;
        let layout = self.get_layout(VIRTUAL_RECT)?;

        let rect = layout.0.get(&target_id).ok_or_eyre("pane not found (force_goto)")?;
        self.focus = VirtCoord::new(rect.x, rect.y);

        let pane = self.panes.get_mut(&target_id).ok_or_eyre("pane not found (force_goto)")?;
        pane.select_view(view);

        Ok(())
    }

    pub fn pane_num(&self) -> usize {
        self.panes.len()
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

    pub fn scale_pane(
        &mut self,
        id: PaneId,
        side: BorderSide,
        amount: i32,
        screen: Rect,
    ) -> Result<()> {
        let borders = self.borders.get(&id).ok_or_eyre("pane not found (scale_pane)")?;

        let op_id = borders[side as usize].ok_or_eyre("cannot scale the pane to this side")?;
        let op = self.operations.get_mut(op_id).ok_or_eyre("operation not found")?;

        if let PaneOperation::Split(split) = op {
            let (side_len, min_len) = match split.direction {
                Direction::Horizontal => (
                    (split.side_len as u32) * (screen.width as u32) / (VIRTUAL_RECT.width as u32),
                    MIN_PANE_SIZE as u32 * screen.width as u32 / VIRTUAL_RECT.width as u32,
                ),
                Direction::Vertical => (
                    (split.side_len as u32) * (screen.height as u32) / (VIRTUAL_RECT.height as u32),
                    MIN_PANE_SIZE as u32 * screen.height as u32 / VIRTUAL_RECT.height as u32,
                ),
            };

            let len1 = side_len * split.ratio[0] / (split.ratio[0] + split.ratio[1]);
            let len2 = side_len - len1;

            if (len1 as i32 + amount) < min_len as i32 || (len2 as i32 - amount) < min_len as i32 {
                return Err(eyre::eyre!("pane is too small to scale"));
            }

            split.ratio = [(len1 as i32 + amount) as u32, (len2 as i32 - amount) as u32];

            Ok(())
        } else {
            Err(eyre::eyre!("invalid operation"))
        }
    }

    pub fn get_focused_pane_mut(&mut self) -> Result<&mut Pane> {
        let info = self.get_focused_info()?;
        self.panes.get_mut(&info.pane_id).ok_or_eyre("invalid pane id")
    }

    pub fn get_focused_pane(&self) -> Result<&Pane> {
        let info = self.get_focused_info()?;
        self.panes.get(&info.pane_id).ok_or_eyre("invalid pane id")
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

    pub fn get_flattened_layout<'a>(&'a self, app: Rect) -> Result<Vec<PaneFlattened<'a>>> {
        let layout = self.get_layout(app)?;
        let focus_info = self.get_focused_info()?;
        layout
            .0
            .iter()
            .map(|(id, rect)| {
                let pane = self
                    .panes
                    .get(id)
                    .ok_or_else(|| eyre::eyre!("pane not found (get_flattened_layout)"))?;
                Ok(PaneFlattened {
                    rect: *rect,
                    views: pane.get_views(),
                    view: pane.get_current_view(),
                    focused: focus_info.pane_id == *id,
                    id: *id,
                })
            })
            .collect::<Result<Vec<PaneFlattened<'a>>>>()
    }

    pub fn get_layout(&self, app: Rect) -> Result<PaneLayout> {
        let mut layout = BTreeMap::new();
        layout.insert(1usize, app);

        for op in &self.operations {
            match op {
                PaneOperation::Split(split) => {
                    let input =
                        layout.get(&split.input_id).ok_or_eyre("pane not found (get_layout)")?;
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
                        layout.get(&merge.input_id[0]).ok_or_eyre("pane not found (get_layout)")?;
                    let input2 =
                        layout.get(&merge.input_id[1]).ok_or_eyre("pane not found (get_layout)")?;
                    let output = input1.union(*input2);

                    layout.remove(&merge.input_id[0]);
                    layout.remove(&merge.input_id[1]);
                    layout.insert(merge.output_id, output);
                }
                PaneOperation::SwitchId(switch) => {
                    let input1 = *layout
                        .get(&switch.input_id[0])
                        .ok_or_eyre("pane not found (get_layout)")?;
                    let input2 = *layout
                        .get(&switch.input_id[1])
                        .ok_or_eyre("pane not found (get_layout)")?;

                    layout.insert(switch.input_id[0], input2);
                    layout.insert(switch.input_id[1], input1);
                }
            }
        }

        Ok(PaneLayout(layout))
    }
}

impl VirtCoord {
    pub fn new(x: u16, y: u16) -> Self {
        Self(x, y)
    }

    pub fn project(x: u16, y: u16, rect: Rect) -> Self {
        Self::new(
            ((x as u64) * (VIRTUAL_RECT.width as u64) / (rect.width as u64)) as u16,
            ((y as u64) * (VIRTUAL_RECT.height as u64) / (rect.height as u64)) as u16,
        )
    }

    pub fn in_rect(&self, rect: Rect) -> bool {
        self.0 >= rect.left() &&
            self.0 < rect.right() &&
            self.1 >= rect.top() &&
            self.1 < rect.bottom()
    }
}

#[inline]
fn mergeable(rect1: &Rect, rect2: &Rect) -> Result<BorderSide> {
    if rect1.width == rect2.width {
        if rect1.bottom() == rect2.top() {
            return Ok(BorderSide::Top);
        }
        if rect1.top() == rect2.bottom() {
            return Ok(BorderSide::Bottom);
        }
    }

    if rect1.height == rect2.height {
        if rect1.right() == rect2.left() {
            return Ok(BorderSide::Left);
        }
        if rect1.left() == rect2.right() {
            return Ok(BorderSide::Right);
        }
    }

    Err(eyre::eyre!("panes are not adjacent"))
}
