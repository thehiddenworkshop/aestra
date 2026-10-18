//! Production persisted workspace model; native docking input/window systems stay separate.
use super::ToolPanel;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};
pub(super) const DEFAULT_TOP_SPLIT_RATIO: f32 = 0.64;

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

pub(crate) use crate::editor_identity::EditorViewId;

/// What a dock tab hosts: a singleton [`ToolPanel`], or a dynamic asset-editor view. Tool panels are
/// deduplicated; editor views are not, so multiple asset editors can coexist as independent tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum DockTab {
    Tool(ToolPanel),
    Editor(EditorViewId),
}

impl From<ToolPanel> for DockTab {
    fn from(panel: ToolPanel) -> Self {
        Self::Tool(panel)
    }
}

impl DockTab {
    /// The tool panel this tab hosts, or `None` for a dynamic editor view.
    pub(crate) fn tool(self) -> Option<ToolPanel> {
        match self {
            Self::Tool(panel) => Some(panel),
            Self::Editor(_) => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DockStack {
    pub(crate) tabs: Vec<DockTab>,
    pub(crate) active: Option<DockTab>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct FloatingPanel {
    pub(crate) panel: ToolPanel,
    pub(crate) position: [f32; 2],
    pub(crate) size: [f32; 2],
}

impl Default for FloatingPanel {
    fn default() -> Self {
        Self {
            panel: ToolPanel::Properties,
            position: [120.0, 80.0],
            size: [420.0, 520.0],
        }
    }
}

impl DockStack {
    pub(crate) fn new(tabs: impl IntoIterator<Item = DockTab>, active: DockTab) -> Self {
        let mut stack = Self {
            tabs: tabs.into_iter().collect(),
            active: Some(active),
        };
        stack.normalize();
        stack
    }

    pub(super) fn normalize(&mut self) {
        let mut unique = Vec::with_capacity(self.tabs.len());
        self.tabs.retain(|tab| {
            if unique.contains(tab) {
                false
            } else {
                unique.push(*tab);
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

    pub(super) fn remove(&mut self, tab: DockTab) {
        self.tabs.retain(|candidate| *candidate != tab);
        self.normalize();
    }

    pub(super) fn push_active(&mut self, tab: DockTab) {
        self.remove(tab);
        self.tabs.push(tab);
        self.active = Some(tab);
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

    pub(super) fn tool_tabs(id: u64, panels: &[ToolPanel], active: ToolPanel) -> Self {
        Self::Tabs {
            id: DockNodeId(id),
            stack: DockStack::new(
                panels.iter().copied().map(DockTab::Tool),
                DockTab::Tool(active),
            ),
        }
    }

    pub(super) fn split(
        id: u64,
        axis: DockAxis,
        ratio: f32,
        first: DockNode,
        second: DockNode,
    ) -> Self {
        Self::Split {
            id: DockNodeId(id),
            axis,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    pub(super) fn find_mut(&mut self, target: DockNodeId) -> Option<&mut Self> {
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

    pub(super) fn find_tabs_mut(&mut self, target: DockNodeId) -> Option<&mut DockStack> {
        match self.find_mut(target)? {
            Self::Tabs { stack, .. } => Some(stack),
            Self::Split { .. } => None,
        }
    }

    pub(super) fn find_tabs(&self, target: DockNodeId) -> Option<&DockStack> {
        if self.id() == target {
            return match self {
                Self::Tabs { stack, .. } => Some(stack),
                Self::Split { .. } => None,
            };
        }
        match self {
            Self::Split { first, second, .. } => {
                first.find_tabs(target).or_else(|| second.find_tabs(target))
            }
            Self::Tabs { .. } => None,
        }
    }

    pub(super) fn remove_tab(&mut self, tab: DockTab) {
        match self {
            Self::Split { first, second, .. } => {
                first.remove_tab(tab);
                second.remove_tab(tab);
            }
            Self::Tabs { stack, .. } => stack.remove(tab),
        }
    }

    pub(super) fn activate(&mut self, tab: DockTab) -> bool {
        match self {
            Self::Split { first, second, .. } => first.activate(tab) || second.activate(tab),
            Self::Tabs { stack, .. } => {
                if !stack.tabs.contains(&tab) || stack.active == Some(tab) {
                    false
                } else {
                    stack.active = Some(tab);
                    true
                }
            }
        }
    }

    pub(crate) fn contains(&self, tab: impl Into<DockTab>) -> bool {
        self.contains_tab(tab.into())
    }

    pub(super) fn contains_tab(&self, tab: DockTab) -> bool {
        match self {
            Self::Split { first, second, .. } => {
                first.contains_tab(tab) || second.contains_tab(tab)
            }
            Self::Tabs { stack, .. } => stack.tabs.contains(&tab),
        }
    }

    pub(super) fn node_containing(&self, tab: impl Into<DockTab>) -> Option<DockNodeId> {
        self.node_containing_tab(tab.into())
    }

    pub(super) fn node_containing_tab(&self, tab: DockTab) -> Option<DockNodeId> {
        match self {
            Self::Split { first, second, .. } => first
                .node_containing_tab(tab)
                .or_else(|| second.node_containing_tab(tab)),
            Self::Tabs { id, stack } => stack.tabs.contains(&tab).then_some(*id),
        }
    }

    pub(super) fn normalize(&mut self) {
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

    pub(super) fn is_empty(&self) -> bool {
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
    pub(crate) floating: Vec<FloatingPanel>,
    pub(super) next_node_id: u64,
}

impl Default for WorkspaceLayout {
    fn default() -> Self {
        // The material graph is the central workspace; the viewport sits on the left, properties on
        // the right, and the utility panels group into a bottom strip. The profiler is hidden by
        // default and reopens beneath the viewport.
        let viewport = DockNode::tool_tabs(2, &[ToolPanel::Viewport], ToolPanel::Viewport);
        let center = DockNode::tool_tabs(
            8,
            &[ToolPanel::Timeline, ToolPanel::MaterialGraph],
            ToolPanel::MaterialGraph,
        );
        // Roughly square viewport on a typical 16:9 window; the graph takes the rest.
        let left_center = DockNode::split(9, DockAxis::Horizontal, 0.42, viewport, center);
        // The right column is the Niagara-style navigate/edit split: the module stack on top selects an
        // item, the properties panel below edits the selection (§28.1).
        let module_stack =
            DockNode::tool_tabs(3, &[ToolPanel::ModuleStack], ToolPanel::ModuleStack);
        let properties = DockNode::tool_tabs(10, &[ToolPanel::Properties], ToolPanel::Properties);
        let right = DockNode::split(11, DockAxis::Vertical, 0.5, module_stack, properties);
        let top = DockNode::split(5, DockAxis::Horizontal, 0.75, left_center, right);
        let bottom = DockNode::tool_tabs(
            4,
            &[ToolPanel::Curves, ToolPanel::Diagnostics, ToolPanel::Assets],
            ToolPanel::Assets,
        );
        Self {
            root: DockNode::split(7, DockAxis::Vertical, DEFAULT_TOP_SPLIT_RATIO, top, bottom),
            floating: Vec::new(),
            next_node_id: 12,
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

    pub(crate) fn dock(
        &mut self,
        tab: impl Into<DockTab>,
        target: DockNodeId,
        drop: DockDrop,
    ) -> bool {
        let tab = tab.into();
        let previous = self.clone();
        self.root.remove_tab(tab);
        self.floating
            .retain(|floating| DockTab::Tool(floating.panel) != tab);
        if drop == DockDrop::Center {
            let Some(stack) = self.root.find_tabs_mut(target) else {
                *self = previous;
                return false;
            };
            stack.push_active(tab);
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
                stack: DockStack::new([tab], tab),
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

    pub(crate) fn activate(&mut self, tab: impl Into<DockTab>) -> bool {
        self.root.activate(tab.into())
    }

    pub(crate) fn reorder_tab(
        &mut self,
        tab: impl Into<DockTab>,
        target: impl Into<DockTab>,
        before: bool,
    ) -> bool {
        let tab = tab.into();
        let target = target.into();
        if tab == target || !self.contains(tab) || !self.root.contains(target) {
            return false;
        }
        let previous = self.clone();
        self.root.remove_tab(tab);
        self.floating
            .retain(|floating| DockTab::Tool(floating.panel) != tab);
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
        stack.tabs.insert(insertion_index, tab);
        stack.active = Some(tab);
        self.root.normalize();
        *self != previous
    }

    pub(crate) fn is_active(&self, tab: impl Into<DockTab>) -> bool {
        let tab = tab.into();
        if self
            .floating
            .iter()
            .any(|floating| DockTab::Tool(floating.panel) == tab)
        {
            return true;
        }
        fn visit(node: &DockNode, tab: DockTab) -> bool {
            match node {
                DockNode::Split { first, second, .. } => visit(first, tab) || visit(second, tab),
                DockNode::Tabs { stack, .. } => stack.active == Some(tab),
            }
        }

        visit(&self.root, tab)
    }

    pub(crate) fn is_visible(&self, tab: impl Into<DockTab>) -> bool {
        self.contains(tab)
    }

    pub(crate) fn close(&mut self, panel: ToolPanel) -> bool {
        if !panel.closable() || !self.contains(panel) {
            return false;
        }
        self.root.remove_tab(panel.into());
        self.floating.retain(|floating| floating.panel != panel);
        self.root.normalize();
        true
    }

    pub(crate) fn show(&mut self, panel: ToolPanel) -> bool {
        if self.floating.iter().any(|floating| floating.panel == panel) {
            return false;
        }
        if self.root.contains(panel) {
            return self.root.activate(panel.into());
        }
        if panel == ToolPanel::AssetInspector
            && let Some(target) = self.root.node_containing(ToolPanel::Properties)
        {
            return self.dock(panel, target, DockDrop::Center);
        }
        if panel == ToolPanel::Settings {
            let Some(target) = self.root.node_containing(ToolPanel::Viewport) else {
                return false;
            };
            let previous = self.clone();
            if !self.dock(panel, target, DockDrop::Center) {
                return false;
            }
            self.reorder_tab(panel, ToolPanel::Viewport, false);
            return *self != previous;
        }
        // Panels reopen next to their default neighbours: the material graph and timeline share the
        // central stack, the utility panels the bottom strip, and the profiler sits under the
        // viewport.
        let center_group = [ToolPanel::MaterialGraph, ToolPanel::Timeline];
        let bottom_group = [
            ToolPanel::Curves,
            ToolPanel::Diagnostics,
            ToolPanel::Assets,
            ToolPanel::CompilerInspector,
        ];
        let co_locate = |layout: &Self, group: &[ToolPanel]| {
            group
                .iter()
                .find_map(|candidate| layout.root.node_containing(*candidate))
                .map(|target| (target, DockDrop::Center))
        };
        let target_and_drop = if center_group.contains(&panel) {
            co_locate(self, &center_group).or_else(|| {
                self.root
                    .node_containing(ToolPanel::Viewport)
                    .map(|target| (target, DockDrop::Right))
            })
        } else if bottom_group.contains(&panel) {
            co_locate(self, &bottom_group).or_else(|| {
                self.root
                    .node_containing(ToolPanel::Viewport)
                    .map(|target| (target, DockDrop::Bottom))
            })
        } else if panel == ToolPanel::Profiler {
            self.root
                .node_containing(ToolPanel::Viewport)
                .map(|target| (target, DockDrop::Bottom))
        } else {
            self.root
                .node_containing(ToolPanel::Viewport)
                .map(|target| (target, DockDrop::Right))
        };
        let Some((target, drop)) = target_and_drop else {
            return false;
        };
        self.dock(panel, target, drop)
    }

    /// Collects the editor views docked anywhere in the tree, in traversal order.
    #[allow(dead_code)] // Also used by workspace persistence/close in later milestones.
    pub(crate) fn editor_views(&self) -> Vec<EditorViewId> {
        fn collect(node: &DockNode, out: &mut Vec<EditorViewId>) {
            match node {
                DockNode::Split { first, second, .. } => {
                    collect(first, out);
                    collect(second, out);
                }
                DockNode::Tabs { stack, .. } => {
                    for tab in &stack.tabs {
                        if let DockTab::Editor(view) = tab {
                            out.push(*view);
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        collect(&self.root, &mut out);
        out
    }

    /// Removes every docked editor-view tab whose id is not in `keep`, then normalizes. Used on
    /// restore to drop tabs whose backing view could not be reconstructed (e.g. its manifest entry
    /// was lost). Returns whether the layout changed.
    pub(crate) fn prune_editor_views(
        &mut self,
        keep: &std::collections::HashSet<EditorViewId>,
    ) -> bool {
        let stale: Vec<EditorViewId> = self
            .editor_views()
            .into_iter()
            .filter(|view| !keep.contains(view))
            .collect();
        if stale.is_empty() {
            return false;
        }
        for view in stale {
            self.root.remove_tab(DockTab::Editor(view));
        }
        self.root.normalize();
        true
    }

    /// Removes a single editor-view tab from the tree, then normalizes. Returns whether the layout
    /// changed. Used when a document's asset is gone (restore) or the view is closed.
    pub(crate) fn close_editor(&mut self, view: EditorViewId) -> bool {
        if !self.root.contains(DockTab::Editor(view)) {
            return false;
        }
        self.root.remove_tab(DockTab::Editor(view));
        self.root.normalize();
        true
    }

    /// Shows an editor-view tab: activates it if already docked, otherwise docks it beside the
    /// material-graph area (falling back to the viewport). Returns whether the layout changed.
    pub(crate) fn show_editor(&mut self, view: EditorViewId) -> bool {
        let tab = DockTab::Editor(view);
        if self.root.contains(tab) {
            return self.root.activate(tab);
        }
        let target = self
            .root
            .node_containing(ToolPanel::MaterialGraph)
            .or_else(|| self.root.node_containing(ToolPanel::Viewport));
        let Some(target) = target else {
            return false;
        };
        self.dock(tab, target, DockDrop::Center)
    }

    pub(crate) fn float_panel(
        &mut self,
        panel: ToolPanel,
        position: [f32; 2],
        available_size: [f32; 2],
    ) -> bool {
        if panel == ToolPanel::Viewport || !self.root.contains(panel) {
            return false;
        }
        self.root.remove_tab(panel.into());
        self.root.normalize();
        let size = default_floating_size(panel, available_size);
        self.floating.push(FloatingPanel {
            panel,
            position,
            size,
        });
        true
    }

    pub(crate) fn update_floating_geometry(
        &mut self,
        panel: ToolPanel,
        position: Option<[f32; 2]>,
        size: Option<[f32; 2]>,
    ) -> bool {
        let floating = self
            .floating
            .iter_mut()
            .find(|floating| floating.panel == panel);
        let Some(floating) = floating else {
            return false;
        };
        let previous = floating.clone();
        if let Some(position) = position {
            floating.position = position;
        }
        if let Some(size) = size {
            floating.size = [size[0].max(260.0), size[1].max(180.0)];
        }
        *floating != previous
    }

    pub(crate) fn redock(&mut self, panel: ToolPanel) -> bool {
        if !self.floating.iter().any(|floating| floating.panel == panel) {
            return false;
        }
        let previous = self.clone();
        self.floating.retain(|floating| floating.panel != panel);
        if self.show(panel) {
            true
        } else {
            *self = previous;
            false
        }
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

    pub(super) fn allocate_id(&mut self) -> DockNodeId {
        let id = DockNodeId(self.next_node_id);
        self.next_node_id += 1;
        id
    }

    pub(super) fn contains(&self, tab: impl Into<DockTab>) -> bool {
        let tab = tab.into();
        self.root.contains(tab)
            || self
                .floating
                .iter()
                .any(|floating| DockTab::Tool(floating.panel) == tab)
    }

    pub(super) fn normalized(mut self) -> Self {
        self.root.normalize();
        let maximum_id = max_node_id(&self.root);
        self.next_node_id = self.next_node_id.max(maximum_id + 1);
        if !self.root.contains(ToolPanel::Viewport) {
            return Self::default();
        }
        for panel in ToolPanel::ALL {
            let mut found = false;
            remove_duplicate_occurrences(&mut self.root, panel, &mut found);
            self.floating.retain(|floating| {
                if floating.panel != panel {
                    true
                } else if found || panel == ToolPanel::Viewport {
                    false
                } else {
                    found = true;
                    true
                }
            });
        }
        self.root.normalize();
        self.migrate_lonely_settings_panel();
        self.migrate_add_module_stack();
        self
    }

    /// Surfaces the Module Stack panel for layouts saved before it existed (§28.1). If the panel is
    /// absent from a saved workspace, dock it alongside Properties (or beside the viewport as a
    /// fallback) so the navigate/edit split appears without discarding the user's arrangement.
    pub(super) fn migrate_add_module_stack(&mut self) {
        if self.root.contains(ToolPanel::ModuleStack)
            || self
                .floating
                .iter()
                .any(|floating| floating.panel == ToolPanel::ModuleStack)
        {
            return;
        }
        if let Some(target) = self.root.node_containing(ToolPanel::Properties) {
            // Split the module stack above the properties panel — the navigate/edit column.
            self.dock(ToolPanel::ModuleStack, target, DockDrop::Top);
        } else if let Some(target) = self.root.node_containing(ToolPanel::Viewport) {
            self.dock(ToolPanel::ModuleStack, target, DockDrop::Center);
        }
    }

    pub(super) fn migrate_lonely_settings_panel(&mut self) {
        if self
            .floating
            .iter()
            .any(|floating| floating.panel == ToolPanel::Settings)
        {
            return;
        }
        let Some(settings_node) = self.root.node_containing(ToolPanel::Settings) else {
            return;
        };
        let Some(viewport_node) = self.root.node_containing(ToolPanel::Viewport) else {
            return;
        };
        if settings_node == viewport_node
            || !self
                .root
                .find_tabs(settings_node)
                .is_some_and(|stack| stack.tabs == [DockTab::Tool(ToolPanel::Settings)])
        {
            return;
        }
        self.dock(ToolPanel::Settings, viewport_node, DockDrop::Center);
        self.reorder_tab(ToolPanel::Settings, ToolPanel::Viewport, false);
    }
}

pub(super) fn default_floating_size(panel: ToolPanel, available_size: [f32; 2]) -> [f32; 2] {
    let preferred: [f32; 2] = match panel {
        ToolPanel::Timeline
        | ToolPanel::Curves
        | ToolPanel::Diagnostics
        | ToolPanel::CompilerInspector
        | ToolPanel::MaterialGraph
        | ToolPanel::Profiler => [720.0, 320.0],
        ToolPanel::Assets
        | ToolPanel::Properties
        | ToolPanel::ModuleStack
        | ToolPanel::AssetInspector => [420.0, 520.0],
        ToolPanel::Settings => [520.0, 620.0],
        ToolPanel::Viewport => [760.0, 540.0],
    };
    [
        preferred[0].min(available_size[0].max(260.0)),
        preferred[1].min(available_size[1].max(180.0)),
    ]
}

pub(super) fn remove_duplicate_occurrences(
    node: &mut DockNode,
    panel: ToolPanel,
    found: &mut bool,
) {
    match node {
        DockNode::Split { first, second, .. } => {
            remove_duplicate_occurrences(first, panel, found);
            remove_duplicate_occurrences(second, panel, found);
        }
        DockNode::Tabs { stack, .. } => {
            stack.tabs.retain(|candidate| {
                if *candidate != DockTab::Tool(panel) {
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

pub(super) fn max_node_id(node: &DockNode) -> u64 {
    match node {
        DockNode::Split {
            id, first, second, ..
        } => id.0.max(max_node_id(first)).max(max_node_id(second)),
        DockNode::Tabs { id, .. } => id.0,
    }
}

pub(super) fn workspace_layout_path() -> PathBuf {
    workspace_config_path("editor-layout.ron")
}

/// Resolves a file inside the editor's per-user config directory, shared by the workspace layout and
/// its companion editor-document manifest so both persist to the same place.
pub(crate) fn workspace_config_path(file: &str) -> PathBuf {
    if let Some(path) = std::env::var_os("AESTRA_CONFIG_DIR") {
        return PathBuf::from(path).join(file);
    }
    #[cfg(target_os = "windows")]
    if let Some(path) = std::env::var_os("APPDATA") {
        return PathBuf::from(path).join("Aestra").join(file);
    }
    #[cfg(not(target_os = "windows"))]
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("aestra").join(file);
    }
    PathBuf::from(".aestra").join(file)
}
