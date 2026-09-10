use crate::{
    EditorSession, FeathersActionButton, Localizer, MenuState, PendingFeathersActivation, theme,
};
use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    ui::InteractionDisabled,
    ui_widgets::Activate,
    window::{PrimaryWindow, WindowPosition},
};
use fluent_bundle::FluentArgs;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

const DEFAULT_TOP_SPLIT_RATIO: f32 = 0.64;

/// Owns the editor's docking lifecycle while panel content remains supplied by the editor shell.
///
/// The persistent [`WorkspaceLayout`] is deliberately separate from transient pointer state. This
/// lets the UI be reconciled from one serializable model instead of treating spawned UI entities as
/// the source of truth.
pub(crate) struct DockingPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DockingSet {
    /// Captures native-window geometry before the rest of the editor responds to the frame.
    Input,
    /// Applies explicit tab, menu, and workspace-layout commands.
    Actions,
    /// Updates drag/drop affordances and floating-window labels before the dock tree is rebuilt.
    Reconcile,
    /// Reconciles native floating windows after the main editor UI has been rebuilt.
    Sync,
}

impl Plugin for DockingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DockDragState>()
            .init_resource::<ResizeState>()
            .init_resource::<MaximizedPanel>()
            .insert_resource(WorkspaceLayout::load())
            .add_observer(queue_docking_action_activation)
            .add_systems(First, crate::dock_ui::activate_staged_native_floating_ui)
            .add_systems(Update, handle_docking_actions.in_set(DockingSet::Actions))
            .add_systems(
                Update,
                crate::dock_ui::persist_native_window_geometry.in_set(DockingSet::Input),
            )
            .add_systems(
                Update,
                (
                    crate::dock_ui::update_floating_window_titles,
                    crate::dock_ui::clear_finished_dock_drag,
                    crate::dock_ui::sync_dock_drop_hints,
                    crate::dock_ui::sync_tab_reorder_hints,
                    crate::dock_ui::sync_tab_append_hint,
                    crate::dock_ui::update_dock_zone_style,
                )
                    .chain()
                    .in_set(DockingSet::Reconcile),
            )
            .add_systems(
                Update,
                (
                    crate::dock_ui::build_added_dock_trees,
                    crate::dock_ui::sync_native_floating_windows,
                )
                    .chain()
                    .in_set(DockingSet::Sync),
            );
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) enum DockingAction {
    Select(DockTab),
    Close(ToolPanel),
    Show(ToolPanel),
    Toggle(ToolPanel),
    Float(ToolPanel, [f32; 2]),
    ResetWorkspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DockingStatus {
    Closed(ToolPanel),
    Showing(ToolPanel),
    Hidden(ToolPanel),
    Floated(ToolPanel),
    WorkspaceReset,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DockingActionOutcome {
    changed: bool,
    status: Option<DockingStatus>,
}

fn queue_docking_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<DockingAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

#[allow(clippy::type_complexity)]
fn handle_docking_actions(
    mut commands: Commands,
    mut actions: Query<
        (
            Entity,
            &Interaction,
            &DockingAction,
            Option<&DockTabButton>,
            Option<&DockCloseButton>,
            Option<&FeathersActionButton>,
            Option<&PendingFeathersActivation>,
            Option<&InteractionDisabled>,
            &mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<Button>, With<FeathersActionButton>)>,
        ),
    >,
    mut layout: ResMut<WorkspaceLayout>,
    mut session: ResMut<EditorSession>,
    mut menu: ResMut<MenuState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    localizer: Res<Localizer>,
) {
    for (
        entity,
        interaction,
        action,
        dock_tab,
        dock_close,
        feathers_action,
        pending_activation,
        disabled,
        mut background,
    ) in &mut actions
    {
        if disabled.is_some() {
            if feathers_action.is_none() {
                background.0 = theme::PANEL_DARK;
            }
            continue;
        }
        match *interaction {
            Interaction::Hovered if feathers_action.is_none() => {
                background.0 = theme::BUTTON_HOVER;
            }
            Interaction::None if feathers_action.is_none() => {
                background.0 = if let Some(tab) = dock_tab {
                    if layout.is_active(tab.0) {
                        theme::PANEL
                    } else {
                        theme::PANEL_DARK
                    }
                } else if dock_close.is_some() {
                    Color::NONE
                } else {
                    theme::BUTTON
                };
            }
            Interaction::Pressed => {
                if feathers_action.is_some() {
                    if pending_activation.is_none() {
                        continue;
                    }
                    commands
                        .entity(entity)
                        .remove::<PendingFeathersActivation>()
                        .insert(Interaction::None);
                } else {
                    background.0 = theme::ACCENT_DIM;
                }
                // Dock tabs activate on release (see `select_dock_tab`), not on press: a
                // press that becomes a drag must not rebuild the tab bar and cancel the
                // drag gesture before `DragStart` can fire.
                if matches!(action, DockingAction::Select(_)) {
                    continue;
                }
                if !matches!(action, DockingAction::Toggle(_)) {
                    menu.open = None;
                    menu.panels_open = false;
                }
                if menu.tab_context.take().is_some() {
                    session.ui_revision += 1;
                }
                let outcome = apply_docking_action(*action, &mut layout, windows.iter().next());
                if !outcome.changed {
                    continue;
                }
                if let Err(error) = layout.save() {
                    warn!("failed to save editor workspace layout: {error}");
                }
                session.ui_revision += 1;
                if let Some(status) = outcome.status {
                    session.status = localize_docking_status(status, &localizer);
                }
            }
            _ => {}
        }
    }
}

fn apply_docking_action(
    action: DockingAction,
    layout: &mut WorkspaceLayout,
    window: Option<&Window>,
) -> DockingActionOutcome {
    match action {
        DockingAction::Select(panel) => DockingActionOutcome {
            changed: layout.activate(panel),
            status: None,
        },
        DockingAction::Close(panel) => DockingActionOutcome {
            changed: layout.close(panel),
            status: Some(DockingStatus::Closed(panel)),
        },
        DockingAction::Show(panel) => DockingActionOutcome {
            changed: layout.show(panel),
            status: Some(DockingStatus::Showing(panel)),
        },
        DockingAction::Toggle(panel) => {
            let was_visible = layout.is_visible(panel);
            DockingActionOutcome {
                changed: if was_visible {
                    layout.close(panel)
                } else {
                    layout.show(panel)
                },
                status: Some(if was_visible {
                    DockingStatus::Hidden(panel)
                } else {
                    DockingStatus::Showing(panel)
                }),
            }
        }
        DockingAction::Float(panel, pointer_position) => {
            let Some(window) = window else {
                return DockingActionOutcome::default();
            };
            let available_size = [window.width(), (window.height() - 108.0).max(180.0)];
            let origin = match window.position {
                WindowPosition::At(position) => position,
                _ => IVec2::new(80, 80),
            };
            let scale = window.scale_factor();
            let position = [
                origin.x as f32 + (pointer_position[0] - 92.0) * scale,
                origin.y as f32 + (pointer_position[1] + 68.0) * scale,
            ];
            DockingActionOutcome {
                changed: layout.float_panel(panel, position, available_size),
                status: Some(DockingStatus::Floated(panel)),
            }
        }
        DockingAction::ResetWorkspace => {
            *layout = WorkspaceLayout::default();
            DockingActionOutcome {
                changed: true,
                status: Some(DockingStatus::WorkspaceReset),
            }
        }
    }
}

fn localize_docking_status(status: DockingStatus, localizer: &Localizer) -> String {
    let (message_id, panel) = match status {
        DockingStatus::Closed(panel) => ("dock-status-closed", Some(panel)),
        DockingStatus::Showing(panel) => ("dock-status-showing", Some(panel)),
        DockingStatus::Hidden(panel) => ("dock-status-hidden", Some(panel)),
        DockingStatus::Floated(panel) => ("dock-status-floated", Some(panel)),
        DockingStatus::WorkspaceReset => ("dock-status-workspace-reset", None),
    };
    let mut args = FluentArgs::new();
    if let Some(panel) = panel {
        args.set("panel", localizer.text(panel.message_id()));
    }
    localizer.text_with(message_id, &args)
}

// Runtime-only docking state and entity markers live beside the plugin rather than the editor
// shell. None of these types are serialized; the dock tree below remains the only persisted source
// of truth.
#[derive(Component)]
pub(crate) struct DockPane(pub(crate) DockNodeId);

/// Editor-shell slot populated exclusively by [`DockingPlugin`].
#[derive(Component)]
#[require(
    Node = dock_tree_host_node(),
    BackgroundColor = BackgroundColor(Color::NONE)
)]
pub(crate) struct DockTreeHost;

fn dock_tree_host_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        flex_grow: 1.0,
        min_width: Val::Px(0.0),
        min_height: Val::Px(0.0),
        ..default()
    }
}

#[derive(Component)]
pub(crate) struct DockTabButton(pub(crate) DockTab);

#[derive(Component)]
pub(crate) struct DockTabAppendZone(pub(crate) DockNodeId);

#[derive(Component)]
pub(crate) struct DockTabAppendIndicator(pub(crate) DockNodeId);

#[derive(Component)]
pub(crate) struct NativeFloatingWindow(pub(crate) ToolPanel);

#[derive(Component)]
pub(crate) struct NativeFloatingCamera(pub(crate) ToolPanel);

#[derive(Component)]
pub(crate) struct NativeFloatingUi {
    pub(crate) panel: ToolPanel,
    pub(crate) revision: u64,
}

#[derive(Component)]
pub(crate) struct StagedNativeFloatingUi;

#[derive(Component)]
pub(crate) struct SplitterGrip;

#[derive(Component)]
pub(crate) struct DockCloseButton;

#[derive(Component)]
pub(crate) struct DockDropHint(pub(crate) DockNodeId);

#[derive(Component)]
pub(crate) struct DockDropZone {
    pub(crate) node: DockNodeId,
    pub(crate) drop: DockDrop,
}

#[derive(Component)]
pub(crate) struct DockDropZoneLabel;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DockSplitter {
    pub(crate) node: DockNodeId,
    pub(crate) axis: DockAxis,
}

#[derive(Component)]
pub(crate) struct DockFirstPane(pub(crate) DockNodeId);

#[derive(Resource, Default)]
pub(crate) struct DockDragState(pub(crate) Option<DockTab>);

#[derive(Resource, Default)]
pub(crate) struct ResizeState(pub(crate) Option<DockSplitter>);

/// The panel currently maximized to fill the whole editor, hiding the rest of the dock tree.
/// Transient: not part of the persisted [`WorkspaceLayout`].
#[derive(Resource, Default)]
pub(crate) struct MaximizedPanel(pub(crate) Option<ToolPanel>);

#[derive(SystemParam)]
pub(crate) struct DockDropQueries<'w, 's> {
    pub(crate) zones: Query<'w, 's, &'static DockDropZone>,
    pub(crate) tabs: Query<'w, 's, &'static DockTabButton>,
    pub(crate) parents: Query<'w, 's, &'static ChildOf>,
}

#[derive(SystemParam)]
pub(crate) struct DockResizeQueries<'w, 's> {
    pub(crate) splitters: Query<'w, 's, &'static DockSplitter>,
    pub(crate) parents: Query<'w, 's, &'static ChildOf>,
    pub(crate) computed: Query<'w, 's, &'static ComputedNode>,
    pub(crate) first_panes: Query<'w, 's, (&'static DockFirstPane, &'static mut Node)>,
    pub(crate) colors: Query<'w, 's, &'static mut BackgroundColor, With<DockSplitter>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum ToolPanel {
    #[default]
    Viewport,
    Assets,
    AssetInspector,
    #[serde(alias = "Inspector")]
    Properties,
    Timeline,
    Curves,
    Diagnostics,
    #[serde(alias = "GeneratedCode")]
    CompilerInspector,
    MaterialGraph,
    Profiler,
    Changes,
    Settings,
}

impl ToolPanel {
    pub(crate) const ALL: [Self; 12] = [
        Self::Viewport,
        Self::Assets,
        Self::AssetInspector,
        Self::Properties,
        Self::Timeline,
        Self::Curves,
        Self::Diagnostics,
        Self::CompilerInspector,
        Self::MaterialGraph,
        Self::Profiler,
        Self::Changes,
        Self::Settings,
    ];

    pub(crate) fn message_id(self) -> &'static str {
        match self {
            Self::Viewport => "panel-viewport",
            Self::Assets => "panel-assets",
            Self::AssetInspector => "panel-asset-inspector",
            Self::Properties => "panel-properties",
            Self::Timeline => "panel-timeline",
            Self::Curves => "panel-curves",
            Self::Diagnostics => "panel-diagnostics",
            Self::CompilerInspector => "panel-compiler-inspector",
            Self::MaterialGraph => "panel-material-graph",
            Self::Profiler => "panel-profiler",
            Self::Changes => "panel-changes",
            Self::Settings => "panel-settings",
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

/// Identity of one open editor-view instance (a dockable asset editor). Distinct from the editor's
/// kind and from its document, so several editors of the same kind can be docked at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct EditorViewId(pub(crate) u64);

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

    fn normalize(&mut self) {
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

    fn remove(&mut self, tab: DockTab) {
        self.tabs.retain(|candidate| *candidate != tab);
        self.normalize();
    }

    fn push_active(&mut self, tab: DockTab) {
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

    fn tool_tabs(id: u64, panels: &[ToolPanel], active: ToolPanel) -> Self {
        Self::Tabs {
            id: DockNodeId(id),
            stack: DockStack::new(
                panels.iter().copied().map(DockTab::Tool),
                DockTab::Tool(active),
            ),
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

    fn find_tabs(&self, target: DockNodeId) -> Option<&DockStack> {
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

    fn remove_tab(&mut self, tab: DockTab) {
        match self {
            Self::Split { first, second, .. } => {
                first.remove_tab(tab);
                second.remove_tab(tab);
            }
            Self::Tabs { stack, .. } => stack.remove(tab),
        }
    }

    fn activate(&mut self, tab: DockTab) -> bool {
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

    fn contains_tab(&self, tab: DockTab) -> bool {
        match self {
            Self::Split { first, second, .. } => {
                first.contains_tab(tab) || second.contains_tab(tab)
            }
            Self::Tabs { stack, .. } => stack.tabs.contains(&tab),
        }
    }

    fn node_containing(&self, tab: impl Into<DockTab>) -> Option<DockNodeId> {
        self.node_containing_tab(tab.into())
    }

    fn node_containing_tab(&self, tab: DockTab) -> Option<DockNodeId> {
        match self {
            Self::Split { first, second, .. } => first
                .node_containing_tab(tab)
                .or_else(|| second.node_containing_tab(tab)),
            Self::Tabs { id, stack } => stack.tabs.contains(&tab).then_some(*id),
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
    pub(crate) floating: Vec<FloatingPanel>,
    next_node_id: u64,
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
        let properties = DockNode::tool_tabs(3, &[ToolPanel::Properties], ToolPanel::Properties);
        let top = DockNode::split(5, DockAxis::Horizontal, 0.75, left_center, properties);
        let bottom = DockNode::tool_tabs(
            4,
            &[
                ToolPanel::Curves,
                ToolPanel::Diagnostics,
                ToolPanel::Changes,
                ToolPanel::Assets,
            ],
            ToolPanel::Assets,
        );
        Self {
            root: DockNode::split(7, DockAxis::Vertical, DEFAULT_TOP_SPLIT_RATIO, top, bottom),
            floating: Vec::new(),
            next_node_id: 10,
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
            ToolPanel::Changes,
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

    fn allocate_id(&mut self) -> DockNodeId {
        let id = DockNodeId(self.next_node_id);
        self.next_node_id += 1;
        id
    }

    fn contains(&self, tab: impl Into<DockTab>) -> bool {
        let tab = tab.into();
        self.root.contains(tab)
            || self
                .floating
                .iter()
                .any(|floating| DockTab::Tool(floating.panel) == tab)
    }

    fn normalized(mut self) -> Self {
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
        self
    }

    fn migrate_lonely_settings_panel(&mut self) {
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

fn default_floating_size(panel: ToolPanel, available_size: [f32; 2]) -> [f32; 2] {
    let preferred: [f32; 2] = match panel {
        ToolPanel::Timeline
        | ToolPanel::Curves
        | ToolPanel::Diagnostics
        | ToolPanel::CompilerInspector
        | ToolPanel::MaterialGraph
        | ToolPanel::Profiler
        | ToolPanel::Changes => [720.0, 320.0],
        ToolPanel::Assets | ToolPanel::Properties | ToolPanel::AssetInspector => [420.0, 520.0],
        ToolPanel::Settings => [520.0, 620.0],
        ToolPanel::Viewport => [760.0, 540.0],
    };
    [
        preferred[0].min(available_size[0].max(260.0)),
        preferred[1].min(available_size[1].max(180.0)),
    ]
}

fn remove_duplicate_occurrences(node: &mut DockNode, panel: ToolPanel, found: &mut bool) {
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

fn max_node_id(node: &DockNode) -> u64 {
    match node {
        DockNode::Split {
            id, first, second, ..
        } => id.0.max(max_node_id(first)).max(max_node_id(second)),
        DockNode::Tabs { id, .. } => id.0,
    }
}

fn workspace_layout_path() -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_inspector_is_optional_closable_and_reuses_properties_stack() {
        let mut layout = WorkspaceLayout::default();
        assert!(!layout.contains(ToolPanel::AssetInspector));
        let assets = layout.root.node_containing(ToolPanel::Assets);
        let properties = layout.root.node_containing(ToolPanel::Properties);
        assert!(layout.show(ToolPanel::AssetInspector));
        assert_eq!(
            layout.root.node_containing(ToolPanel::AssetInspector),
            properties
        );
        assert_eq!(layout.root.node_containing(ToolPanel::Assets), assets);
        assert!(layout.close(ToolPanel::AssetInspector));
        assert!(layout.contains(ToolPanel::Assets));
        assert!(layout.show(ToolPanel::AssetInspector));
        assert!(layout.float_panel(ToolPanel::AssetInspector, [40.0, 40.0], [1000.0, 700.0]));
        assert!(layout.close(ToolPanel::AssetInspector));
    }

    #[test]
    fn docking_actions_own_panel_visibility_and_workspace_reset() {
        let mut layout = WorkspaceLayout::default();
        let closed = apply_docking_action(
            DockingAction::Close(ToolPanel::Properties),
            &mut layout,
            None,
        );
        assert!(closed.changed);
        assert_eq!(
            closed.status,
            Some(DockingStatus::Closed(ToolPanel::Properties))
        );
        assert!(!layout.is_visible(ToolPanel::Properties));

        let shown = apply_docking_action(
            DockingAction::Toggle(ToolPanel::Properties),
            &mut layout,
            None,
        );
        assert!(shown.changed);
        assert_eq!(
            shown.status,
            Some(DockingStatus::Showing(ToolPanel::Properties))
        );
        assert!(layout.is_visible(ToolPanel::Properties));

        assert!(layout.close(ToolPanel::Assets));
        let reset = apply_docking_action(DockingAction::ResetWorkspace, &mut layout, None);
        assert_eq!(reset.status, Some(DockingStatus::WorkspaceReset));
        assert_eq!(layout, WorkspaceLayout::default());
    }

    #[test]
    fn docking_statuses_use_localized_panel_names() {
        let english = Localizer::new("en-US").unwrap();
        let french = Localizer::new("fr-FR").unwrap();
        let english =
            localize_docking_status(DockingStatus::Floated(ToolPanel::Profiler), &english);
        assert!(english.starts_with("Floated"));
        assert!(english.contains("PROFILER"));
        let french = localize_docking_status(DockingStatus::Closed(ToolPanel::Properties), &french);
        assert!(french.contains("PROPRIÉTÉS"));
        assert!(french.ends_with("fermé · rouvrez-le depuis Affichage"));
    }

    #[test]
    fn feathers_activation_queues_one_docking_action() {
        let mut app = App::new();
        app.add_observer(queue_docking_action_activation);
        let action = app
            .world_mut()
            .spawn((
                DockingAction::ResetWorkspace,
                FeathersActionButton,
                Interaction::None,
            ))
            .id();

        app.world_mut().trigger(Activate { entity: action });
        app.update();

        let action = app.world().entity(action);
        assert!(action.contains::<PendingFeathersActivation>());
        assert_eq!(action.get::<Interaction>(), Some(&Interaction::Pressed));
    }

    #[test]
    fn docking_plugin_owns_layout_and_transient_interaction_state() {
        let mut app = App::new();
        app.add_plugins(DockingPlugin);

        assert!(app.world().contains_resource::<WorkspaceLayout>());
        assert!(app.world().contains_resource::<DockDragState>());
        assert!(app.world().contains_resource::<ResizeState>());

        let host = app.world_mut().spawn(DockTreeHost).id();
        let node = app.world().get::<Node>(host).unwrap();
        assert_eq!(node.width, Val::Percent(100.0));
        assert_eq!(node.flex_grow, 1.0);
        assert_eq!(
            app.world().get::<BackgroundColor>(host).unwrap().0,
            Color::NONE
        );
    }

    #[test]
    fn floating_panel_content_tracks_the_editor_ui_revision() {
        assert!(crate::dock_ui::floating_root_is_current(Some(7), 7));
        assert!(!crate::dock_ui::floating_root_is_current(Some(6), 7));
        assert!(!crate::dock_ui::floating_root_is_current(None, 7));
    }

    #[test]
    fn layout_round_trips_through_ron() {
        let layout = WorkspaceLayout::default();
        let source = ron::to_string(&layout).unwrap();
        assert_eq!(ron::from_str::<WorkspaceLayout>(&source).unwrap(), layout);
    }

    #[test]
    fn default_workspace_reserves_professional_choreography_height() {
        let layout = WorkspaceLayout::default();
        let DockNode::Split { axis, ratio, .. } = layout.root else {
            panic!("default workspace root should be a split");
        };
        assert_eq!(axis, DockAxis::Vertical);
        assert!((ratio - DEFAULT_TOP_SPLIT_RATIO).abs() < f32::EPSILON);
        assert!((1.0 - ratio - 0.36).abs() < f32::EPSILON);
    }

    #[test]
    fn serialized_workspace_preserves_an_existing_user_split() {
        let mut layout = WorkspaceLayout::default();
        let DockNode::Split { ratio, .. } = &mut layout.root else {
            panic!("default workspace root should be a split");
        };
        *ratio = 0.73;
        let source = ron::to_string(&layout).unwrap();
        let restored = ron::from_str::<WorkspaceLayout>(&source).unwrap();
        let DockNode::Split { ratio, .. } = restored.root else {
            panic!("restored workspace root should be a split");
        };
        assert_eq!(ratio, 0.73);
    }

    #[test]
    fn legacy_generated_code_panel_name_migrates_to_compiler_inspector() {
        assert_eq!(
            ron::from_str::<ToolPanel>("GeneratedCode").unwrap(),
            ToolPanel::CompilerInspector
        );
        assert_eq!(
            ron::to_string(&ToolPanel::CompilerInspector).unwrap(),
            "CompilerInspector"
        );
    }

    #[test]
    fn legacy_inspector_panel_name_migrates_to_properties() {
        assert_eq!(
            ron::from_str::<ToolPanel>("Inspector").unwrap(),
            ToolPanel::Properties
        );
        assert_eq!(
            ron::to_string(&ToolPanel::Properties).unwrap(),
            "Properties"
        );
    }

    #[test]
    fn center_drop_builds_a_tab_stack() {
        let mut layout = WorkspaceLayout::default();
        let target = layout.root.node_containing(ToolPanel::Properties).unwrap();
        assert!(layout.dock(ToolPanel::Assets, target, DockDrop::Center));
        assert_eq!(layout.root.node_containing(ToolPanel::Assets), Some(target));
        assert!(layout.is_active(ToolPanel::Assets));
    }

    #[test]
    fn edge_drop_creates_a_nested_split() {
        let mut layout = WorkspaceLayout::default();
        let target = layout.root.node_containing(ToolPanel::Viewport).unwrap();
        assert!(layout.dock(ToolPanel::Curves, target, DockDrop::Left));
        assert_ne!(layout.root.node_containing(ToolPanel::Curves), Some(target));
        assert!(layout.root.contains(ToolPanel::Viewport));
    }

    #[test]
    fn closing_the_last_tab_prunes_its_branch() {
        let mut layout = WorkspaceLayout::default();
        assert!(layout.close(ToolPanel::Properties));
        assert!(!layout.root.contains(ToolPanel::Properties));
        assert!(layout.root.contains(ToolPanel::Viewport));
        assert!(layout.show(ToolPanel::Properties));
        assert!(layout.is_active(ToolPanel::Properties));
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
        let bottom = layout.root.node_containing(ToolPanel::Curves).unwrap();
        assert!(layout.reorder_tab(ToolPanel::Assets, ToolPanel::Curves, true));
        let DockNode::Tabs { stack, .. } = layout.root.find_mut(bottom).unwrap() else {
            panic!("bottom node should be a tab stack");
        };
        assert_eq!(
            stack.tabs,
            vec![
                DockTab::Tool(ToolPanel::Assets),
                DockTab::Tool(ToolPanel::Curves),
                DockTab::Tool(ToolPanel::Diagnostics),
                DockTab::Tool(ToolPanel::Changes),
            ]
        );

        // Pull the timeline out of the central stack into the bottom strip.
        assert!(layout.reorder_tab(ToolPanel::Timeline, ToolPanel::Changes, false));
        assert_eq!(
            layout.root.node_containing(ToolPanel::Timeline),
            Some(bottom)
        );
        assert!(layout.is_active(ToolPanel::Timeline));
    }

    #[test]
    fn editor_views_coexist_and_move_as_independent_tabs() {
        let mut layout = WorkspaceLayout::default();
        let view_a = DockTab::Editor(EditorViewId(101));
        let view_b = DockTab::Editor(EditorViewId(102));
        let center = layout
            .root
            .node_containing(ToolPanel::MaterialGraph)
            .unwrap();
        assert!(layout.dock(view_a, center, DockDrop::Center));
        assert!(layout.dock(view_b, center, DockDrop::Center));
        // Two editor views of the same kind coexist — editor tabs are not deduplicated.
        assert!(layout.root.contains(view_a));
        assert!(layout.root.contains(view_b));
        assert_eq!(layout.root.node_containing(view_a), Some(center));
        assert!(layout.is_active(view_b));

        // An editor tab moves to another stack without disturbing the other.
        let bottom = layout.root.node_containing(ToolPanel::Curves).unwrap();
        assert!(layout.dock(view_a, bottom, DockDrop::Center));
        assert_eq!(layout.root.node_containing(view_a), Some(bottom));
        assert!(layout.root.contains(view_b));
    }

    #[test]
    fn tool_panels_are_not_duplicated_by_docking() {
        fn occurrences(node: &DockNode, tab: DockTab) -> usize {
            match node {
                DockNode::Split { first, second, .. } => {
                    occurrences(first, tab) + occurrences(second, tab)
                }
                DockNode::Tabs { stack, .. } => stack
                    .tabs
                    .iter()
                    .filter(|candidate| **candidate == tab)
                    .count(),
            }
        }
        let mut layout = WorkspaceLayout::default();
        let viewport = layout.root.node_containing(ToolPanel::Viewport).unwrap();
        // Docking an already-present tool panel relocates it rather than creating a duplicate.
        assert!(layout.dock(ToolPanel::Assets, viewport, DockDrop::Center));
        assert_eq!(
            occurrences(&layout.root, DockTab::Tool(ToolPanel::Assets)),
            1
        );
    }

    #[test]
    fn diagnostics_restores_to_the_bottom_tab_stack() {
        let mut layout = WorkspaceLayout::default();
        let bottom = layout.root.node_containing(ToolPanel::Curves).unwrap();
        assert_eq!(
            layout.root.node_containing(ToolPanel::Diagnostics),
            Some(bottom)
        );
        assert!(layout.close(ToolPanel::Diagnostics));
        assert!(layout.show(ToolPanel::Diagnostics));
        assert_eq!(
            layout.root.node_containing(ToolPanel::Diagnostics),
            Some(bottom)
        );
        assert!(layout.is_active(ToolPanel::Diagnostics));
    }

    #[test]
    fn compiler_inspector_is_advanced_and_restores_to_the_bottom_tab_stack() {
        let mut layout = WorkspaceLayout::default();
        let bottom = layout.root.node_containing(ToolPanel::Curves).unwrap();
        assert!(!layout.is_visible(ToolPanel::CompilerInspector));
        assert!(layout.show(ToolPanel::CompilerInspector));
        assert_eq!(
            layout.root.node_containing(ToolPanel::CompilerInspector),
            Some(bottom)
        );
        assert!(layout.is_active(ToolPanel::CompilerInspector));
    }

    #[test]
    fn profiler_restores_beneath_the_viewport() {
        let mut layout = WorkspaceLayout::default();
        // The profiler is hidden by default and reopens beneath the viewport, not in the bottom
        // utility strip.
        assert!(!layout.root.contains(ToolPanel::Profiler));
        assert!(layout.show(ToolPanel::Profiler));
        assert!(layout.root.contains(ToolPanel::Profiler));
        assert!(layout.is_active(ToolPanel::Profiler));
        assert_ne!(
            layout.root.node_containing(ToolPanel::Profiler),
            layout.root.node_containing(ToolPanel::Curves)
        );
    }

    #[test]
    fn settings_restores_beside_the_viewport_tab() {
        let mut layout = WorkspaceLayout::default();
        assert!(layout.show(ToolPanel::Settings));
        let viewport = layout.root.node_containing(ToolPanel::Viewport).unwrap();
        assert_eq!(
            layout.root.node_containing(ToolPanel::Settings),
            Some(viewport)
        );
        let stack = layout.root.find_tabs(viewport).unwrap();
        let viewport_index = stack
            .tabs
            .iter()
            .position(|panel| *panel == DockTab::Tool(ToolPanel::Viewport))
            .unwrap();
        assert_eq!(
            stack.tabs.get(viewport_index + 1),
            Some(&DockTab::Tool(ToolPanel::Settings))
        );
        assert!(layout.is_active(ToolPanel::Settings));
    }

    #[test]
    fn legacy_lonely_settings_split_migrates_to_the_viewport_tabs() {
        let mut layout = WorkspaceLayout::default();
        let viewport = layout.root.node_containing(ToolPanel::Viewport).unwrap();
        assert!(layout.dock(ToolPanel::Settings, viewport, DockDrop::Right));
        assert_ne!(
            layout.root.node_containing(ToolPanel::Settings),
            layout.root.node_containing(ToolPanel::Viewport)
        );

        let layout = layout.normalized();
        assert_eq!(
            layout.root.node_containing(ToolPanel::Settings),
            layout.root.node_containing(ToolPanel::Viewport)
        );
    }

    #[test]
    fn floating_panels_leave_no_empty_dock_and_can_redock() {
        let mut layout = WorkspaceLayout::default();
        assert!(layout.float_panel(ToolPanel::Properties, [900.0, 80.0], [1200.0, 800.0]));
        assert!(!layout.root.contains(ToolPanel::Properties));
        assert_eq!(layout.floating[0].panel, ToolPanel::Properties);

        assert!(layout.redock(ToolPanel::Properties));
        assert!(layout.floating.is_empty());
        assert!(layout.root.contains(ToolPanel::Properties));
    }

    #[test]
    fn floating_window_geometry_is_persisted_and_size_is_clamped() {
        let mut layout = WorkspaceLayout::default();
        assert!(layout.float_panel(ToolPanel::Assets, [40.0, 40.0], [1000.0, 700.0]));
        assert!(layout.update_floating_geometry(
            ToolPanel::Assets,
            Some([-2400.0, 160.0]),
            Some([100.0, 120.0]),
        ));
        assert_eq!(layout.floating[0].position, [-2400.0, 160.0]);
        assert_eq!(layout.floating[0].size, [260.0, 180.0]);
    }
}
