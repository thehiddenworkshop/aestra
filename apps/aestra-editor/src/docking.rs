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
mod model;
pub(crate) use model::*;

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
            .add_observer(crate::dock_ui::scroll_dock_tabs)
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
                    crate::dock_ui::sync_dock_tab_dirty,
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

mod maximized;
pub(crate) use maximized::MaximizedPanel;

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

mod panels;
pub(crate) use panels::ToolPanel;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_changes_panels_migrate_to_diagnostics_without_resetting_the_workspace() {
        let layout = WorkspaceLayout::default();
        let source = ron::to_string(&layout).unwrap();
        let legacy = source.replace("Tool(Diagnostics)", "Tool(Diagnostics),Tool(Changes)");
        assert_ne!(source, legacy);
        let loaded: WorkspaceLayout = ron::from_str(&legacy).unwrap();
        assert_eq!(loaded.normalized(), layout);
        let panel: ToolPanel = ron::from_str("Changes").unwrap();
        assert_eq!(panel, ToolPanel::Diagnostics);
        assert!(!ron::to_string(&layout).unwrap().contains("Changes"));
    }

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
    fn assets_cutover_preserves_docked_closed_and_floating_layouts() {
        for placement in 0..3 {
            let mut layout = WorkspaceLayout::default();
            match placement {
                1 => assert!(layout.close(ToolPanel::Assets)),
                2 => assert!(layout.float_panel(
                    ToolPanel::Assets,
                    [-1200.0, 80.0],
                    [1800.0, 1100.0]
                )),
                _ => {}
            }
            let slot = layout.root.node_containing(ToolPanel::Assets);
            let source = ron::to_string(&layout).unwrap();
            let restored = ron::from_str::<WorkspaceLayout>(&source)
                .unwrap()
                .normalized();
            assert_eq!(restored.root.node_containing(ToolPanel::Assets), slot);
            assert_eq!(restored, layout);
            assert_eq!(ron::to_string(&ToolPanel::Assets).unwrap(), "Assets");
        }
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
            ]
        );

        // Pull the timeline out of the central stack into the bottom strip.
        assert!(layout.reorder_tab(ToolPanel::Timeline, ToolPanel::Diagnostics, false));
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
