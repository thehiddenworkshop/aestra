use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DockPanel {
    #[default]
    Viewport,
    Assets,
    Inspector,
    Timeline,
    Curves,
    Changes,
}

impl DockPanel {
    pub(crate) const ALL: [Self; 6] = [
        Self::Viewport,
        Self::Assets,
        Self::Inspector,
        Self::Timeline,
        Self::Curves,
        Self::Changes,
    ];

    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Viewport => "VIEWPORT",
            Self::Assets => "ASSETS",
            Self::Inspector => "INSPECTOR",
            Self::Timeline => "TIMELINE",
            Self::Curves => "CURVES",
            Self::Changes => "CHANGES",
        }
    }

    pub(crate) fn closable(self) -> bool {
        self != Self::Viewport
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DockAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DockDrop {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct DockNodeId(pub(crate) u64);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DockStack {
    pub(crate) tabs: Vec<DockPanel>,
    pub(crate) active: Option<DockPanel>,
}

impl DockStack {
    pub(crate) fn new(tabs: impl IntoIterator<Item = DockPanel>, active: DockPanel) -> Self {
        let mut stack = Self {
            tabs: tabs.into_iter().collect(),
            active: Some(active),
        };
        stack.normalize();
        stack
    }

    fn normalize(&mut self) {
        let mut unique = Vec::with_capacity(self.tabs.len());
        self.tabs.retain(|panel| {
            if unique.contains(panel) {
                false
            } else {
                unique.push(*panel);
                true
            }
        });
        if !self
            .active
            .is_some_and(|active| self.tabs.contains(&active))
        {
            self.active = self.tabs.last().copied();
        }
    }

    fn remove(&mut self, panel: DockPanel) {
        self.tabs.retain(|candidate| *candidate != panel);
        self.normalize();
    }

    fn push_active(&mut self, panel: DockPanel) {
        self.remove(panel);
        self.tabs.push(panel);
        self.active = Some(panel);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum DockNode {
    Split {
        id: DockNodeId,
        axis: DockAxis,
        ratio: f32,
        first: Box<DockNode>,
        second: Box<DockNode>,
    },
    Tabs {
        id: DockNodeId,
        stack: DockStack,
    },
}

impl DockNode {
    pub(crate) fn id(&self) -> DockNodeId {
        match self {
            Self::Split { id, .. } | Self::Tabs { id, .. } => *id,
        }
    }

    fn tabs(id: u64, panels: &[DockPanel], active: DockPanel) -> Self {
        Self::Tabs {
            id: DockNodeId(id),
            stack: DockStack::new(panels.iter().copied(), active),
        }
    }

    fn split(id: u64, axis: DockAxis, ratio: f32, first: DockNode, second: DockNode) -> Self {
        Self::Split {
            id: DockNodeId(id),
            axis,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    fn find_mut(&mut self, target: DockNodeId) -> Option<&mut Self> {
        if self.id() == target {
            return Some(self);
        }
        match self {
            Self::Split { first, second, .. } => {
                first.find_mut(target).or_else(|| second.find_mut(target))
            }
            Self::Tabs { .. } => None,
        }
    }

    fn find_tabs_mut(&mut self, target: DockNodeId) -> Option<&mut DockStack> {
        match self.find_mut(target)? {
            Self::Tabs { stack, .. } => Some(stack),
            Self::Split { .. } => None,
        }
    }

    fn remove_panel(&mut self, panel: DockPanel) {
        match self {
            Self::Split { first, second, .. } => {
                first.remove_panel(panel);
                second.remove_panel(panel);
            }
            Self::Tabs { stack, .. } => stack.remove(panel),
        }
    }

    fn activate(&mut self, panel: DockPanel) -> bool {
        match self {
            Self::Split { first, second, .. } => first.activate(panel) || second.activate(panel),
            Self::Tabs { stack, .. } => {
                if !stack.tabs.contains(&panel) || stack.active == Some(panel) {
                    false
                } else {
                    stack.active = Some(panel);
                    true
                }
            }
        }
    }

    fn contains(&self, panel: DockPanel) -> bool {
        match self {
            Self::Split { first, second, .. } => first.contains(panel) || second.contains(panel),
            Self::Tabs { stack, .. } => stack.tabs.contains(&panel),
        }
    }

    fn node_containing(&self, panel: DockPanel) -> Option<DockNodeId> {
        match self {
            Self::Split { first, second, .. } => first
                .node_containing(panel)
                .or_else(|| second.node_containing(panel)),
            Self::Tabs { id, stack } => stack.tabs.contains(&panel).then_some(*id),
        }
    }

    fn normalize(&mut self) {
        let Self::Split { first, second, .. } = self else {
            if let Self::Tabs { stack, .. } = self {
                stack.normalize();
            }
            return;
        };
        first.normalize();
        second.normalize();
        let first_empty = first.is_empty();
        let second_empty = second.is_empty();
        if first_empty && !second_empty {
            *self = (**second).clone();
        } else if second_empty && !first_empty {
            *self = (**first).clone();
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Split { first, second, .. } => first.is_empty() && second.is_empty(),
            Self::Tabs { stack, .. } => stack.tabs.is_empty(),
        }
    }
}

#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct WorkspaceLayout {
    pub(crate) root: DockNode,
    next_node_id: u64,
}

impl Default for WorkspaceLayout {
    fn default() -> Self {
        let assets = DockNode::tabs(1, &[DockPanel::Assets], DockPanel::Assets);
        let viewport = DockNode::tabs(2, &[DockPanel::Viewport], DockPanel::Viewport);
        let inspector = DockNode::tabs(3, &[DockPanel::Inspector], DockPanel::Inspector);
        let bottom = DockNode::tabs(
            4,
            &[DockPanel::Timeline, DockPanel::Curves, DockPanel::Changes],
            DockPanel::Timeline,
        );
        let center_right = DockNode::split(5, DockAxis::Horizontal, 0.68, viewport, inspector);
        let top = DockNode::split(6, DockAxis::Horizontal, 0.17, assets, center_right);
        Self {
            root: DockNode::split(7, DockAxis::Vertical, 0.71, top, bottom),
            next_node_id: 8,
        }
    }
}

impl WorkspaceLayout {
    pub(crate) fn load() -> Self {
        fs::read_to_string(workspace_layout_path())
            .ok()
            .and_then(|source| ron::from_str(&source).ok())
            .map(Self::normalized)
            .unwrap_or_default()
    }

    pub(crate) fn save(&self) -> io::Result<()> {
        let path = workspace_layout_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let source = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(io::Error::other)?;
        fs::write(path, source)
    }

    pub(crate) fn dock(&mut self, panel: DockPanel, target: DockNodeId, drop: DockDrop) -> bool {
        let previous = self.clone();
        self.root.remove_panel(panel);
        if drop == DockDrop::Center {
            let Some(stack) = self.root.find_tabs_mut(target) else {
                *self = previous;
                return false;
            };
            stack.push_active(panel);
        } else {
            let new_tabs_id = self.allocate_id();
            let new_split_id = self.allocate_id();
            let Some(target_node) = self.root.find_mut(target) else {
                *self = previous;
                return false;
            };
            let placeholder = DockNode::Tabs {
                id: target,
                stack: DockStack::default(),
            };
            let existing = std::mem::replace(target_node, placeholder);
            let new_panel = DockNode::Tabs {
                id: new_tabs_id,
                stack: DockStack::new([panel], panel),
            };
            let (axis, ratio, first, second) = match drop {
                DockDrop::Left => (DockAxis::Horizontal, 0.28, new_panel, existing),
                DockDrop::Right => (DockAxis::Horizontal, 0.72, existing, new_panel),
                DockDrop::Top => (DockAxis::Vertical, 0.30, new_panel, existing),
                DockDrop::Bottom => (DockAxis::Vertical, 0.70, existing, new_panel),
                DockDrop::Center => unreachable!(),
            };
            *target_node = DockNode::Split {
                id: new_split_id,
                axis,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
            };
        }
        self.root.normalize();
        *self != previous
    }

    pub(crate) fn activate(&mut self, panel: DockPanel) -> bool {
        self.root.activate(panel)
    }

    pub(crate) fn reorder_tab(
        &mut self,
        panel: DockPanel,
        target: DockPanel,
        before: bool,
    ) -> bool {
        if panel == target || !self.root.contains(panel) || !self.root.contains(target) {
            return false;
        }
        let previous = self.clone();
        self.root.remove_panel(panel);
        let Some(target_node) = self.root.node_containing(target) else {
            *self = previous;
            return false;
        };
        let Some(stack) = self.root.find_tabs_mut(target_node) else {
            *self = previous;
            return false;
        };
        let Some(target_index) = stack.tabs.iter().position(|candidate| *candidate == target)
        else {
            *self = previous;
            return false;
        };
        let insertion_index = target_index + usize::from(!before);
        stack.tabs.insert(insertion_index, panel);
        stack.active = Some(panel);
        self.root.normalize();
        *self != previous
    }

    pub(crate) fn is_active(&self, panel: DockPanel) -> bool {
        fn visit(node: &DockNode, panel: DockPanel) -> bool {
            match node {
                DockNode::Split { first, second, .. } => {
                    visit(first, panel) || visit(second, panel)
                }
                DockNode::Tabs { stack, .. } => stack.active == Some(panel),
            }
        }

        visit(&self.root, panel)
    }

    pub(crate) fn close(&mut self, panel: DockPanel) -> bool {
        if !panel.closable() || !self.root.contains(panel) {
            return false;
        }
        self.root.remove_panel(panel);
        self.root.normalize();
        true
    }

    pub(crate) fn show(&mut self, panel: DockPanel) -> bool {
        if self.root.contains(panel) {
            return self.root.activate(panel);
        }
        let bottom_group = [DockPanel::Timeline, DockPanel::Curves, DockPanel::Changes];
        let target_and_drop = if bottom_group.contains(&panel) {
            bottom_group
                .into_iter()
                .find_map(|candidate| self.root.node_containing(candidate))
                .map(|target| (target, DockDrop::Center))
                .or_else(|| {
                    self.root
                        .node_containing(DockPanel::Viewport)
                        .map(|target| (target, DockDrop::Bottom))
                })
        } else {
            self.root
                .node_containing(DockPanel::Viewport)
                .map(|target| {
                    (
                        target,
                        if panel == DockPanel::Assets {
                            DockDrop::Left
                        } else {
                            DockDrop::Right
                        },
                    )
                })
        };
        let Some((target, drop)) = target_and_drop else {
            return false;
        };
        self.dock(panel, target, drop)
    }

    pub(crate) fn resize_split(&mut self, id: DockNodeId, delta: f32, span: f32) -> bool {
        if span <= 0.0 {
            return false;
        }
        let Some(DockNode::Split { ratio, .. }) = self.root.find_mut(id) else {
            return false;
        };
        let next = (*ratio + delta / span).clamp(0.12, 0.88);
        if (*ratio - next).abs() <= f32::EPSILON {
            return false;
        }
        *ratio = next;
        true
    }

    fn allocate_id(&mut self) -> DockNodeId {
        let id = DockNodeId(self.next_node_id);
        self.next_node_id += 1;
        id
    }

    fn normalized(mut self) -> Self {
        self.root.normalize();
        let maximum_id = max_node_id(&self.root);
        self.next_node_id = self.next_node_id.max(maximum_id + 1);
        if !self.root.contains(DockPanel::Viewport) {
            return Self::default();
        }
        for panel in DockPanel::ALL {
            remove_duplicate_occurrences(&mut self.root, panel, &mut false);
        }
        self.root.normalize();
        self
    }
}

fn remove_duplicate_occurrences(node: &mut DockNode, panel: DockPanel, found: &mut bool) {
    match node {
        DockNode::Split { first, second, .. } => {
            remove_duplicate_occurrences(first, panel, found);
            remove_duplicate_occurrences(second, panel, found);
        }
        DockNode::Tabs { stack, .. } => {
            stack.tabs.retain(|candidate| {
                if *candidate != panel {
                    true
                } else if *found {
                    false
                } else {
                    *found = true;
                    true
                }
            });
            stack.normalize();
        }
    }
}

fn max_node_id(node: &DockNode) -> u64 {
    match node {
        DockNode::Split {
            id, first, second, ..
        } => id.0.max(max_node_id(first)).max(max_node_id(second)),
        DockNode::Tabs { id, .. } => id.0,
    }
}

fn workspace_layout_path() -> PathBuf {
    if let Some(path) = std::env::var_os("AESTRA_CONFIG_DIR") {
        return PathBuf::from(path).join("editor-layout.ron");
    }
    #[cfg(target_os = "windows")]
    if let Some(path) = std::env::var_os("APPDATA") {
        return PathBuf::from(path).join("Aestra").join("editor-layout.ron");
    }
    #[cfg(not(target_os = "windows"))]
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("aestra").join("editor-layout.ron");
    }
    PathBuf::from(".aestra").join("editor-layout.ron")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_round_trips_through_ron() {
        let layout = WorkspaceLayout::default();
        let source = ron::to_string(&layout).unwrap();
        assert_eq!(ron::from_str::<WorkspaceLayout>(&source).unwrap(), layout);
    }

    #[test]
    fn center_drop_builds_a_tab_stack() {
        let mut layout = WorkspaceLayout::default();
        let target = layout.root.node_containing(DockPanel::Inspector).unwrap();
        assert!(layout.dock(DockPanel::Assets, target, DockDrop::Center));
        assert_eq!(layout.root.node_containing(DockPanel::Assets), Some(target));
        assert!(layout.is_active(DockPanel::Assets));
    }

    #[test]
    fn edge_drop_creates_a_nested_split() {
        let mut layout = WorkspaceLayout::default();
        let target = layout.root.node_containing(DockPanel::Viewport).unwrap();
        assert!(layout.dock(DockPanel::Curves, target, DockDrop::Left));
        assert_ne!(layout.root.node_containing(DockPanel::Curves), Some(target));
        assert!(layout.root.contains(DockPanel::Viewport));
    }

    #[test]
    fn closing_the_last_tab_prunes_its_branch() {
        let mut layout = WorkspaceLayout::default();
        assert!(layout.close(DockPanel::Inspector));
        assert!(!layout.root.contains(DockPanel::Inspector));
        assert!(layout.root.contains(DockPanel::Viewport));
        assert!(layout.show(DockPanel::Inspector));
        assert!(layout.is_active(DockPanel::Inspector));
    }

    #[test]
    fn split_resizing_is_clamped() {
        let mut layout = WorkspaceLayout::default();
        let root = layout.root.id();
        assert!(layout.resize_split(root, 10_000.0, 100.0));
        let DockNode::Split { ratio, .. } = layout.root else {
            panic!("default root should be split");
        };
        assert_eq!(ratio, 0.88);
    }

    #[test]
    fn tabs_can_be_reordered_and_moved_between_stacks() {
        let mut layout = WorkspaceLayout::default();
        let bottom = layout.root.node_containing(DockPanel::Timeline).unwrap();
        assert!(layout.reorder_tab(DockPanel::Changes, DockPanel::Timeline, true));
        let DockNode::Tabs { stack, .. } = layout.root.find_mut(bottom).unwrap() else {
            panic!("bottom node should be a tab stack");
        };
        assert_eq!(
            stack.tabs,
            vec![DockPanel::Changes, DockPanel::Timeline, DockPanel::Curves]
        );

        assert!(layout.reorder_tab(DockPanel::Assets, DockPanel::Curves, false));
        assert_eq!(layout.root.node_containing(DockPanel::Assets), Some(bottom));
        assert!(layout.is_active(DockPanel::Assets));
    }
}
