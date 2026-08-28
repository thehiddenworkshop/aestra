//! Dock-tree entity construction and transient docking interactions.

use crate::*;

pub(crate) fn update_floating_window_titles(
    localizer: Res<Localizer>,
    mut windows: Query<(&NativeFloatingWindow, &mut Window)>,
) {
    if !localizer.is_changed() {
        return;
    }
    for (floating, mut window) in &mut windows {
        window.title = format!("{} — Aestra", localizer.text(floating.0.message_id()));
    }
}

pub(crate) fn persist_native_window_geometry(
    mut moved: MessageReader<WindowMoved>,
    mut resized: MessageReader<WindowResized>,
    floating_windows: Query<&NativeFloatingWindow>,
    mut layout: ResMut<WorkspaceLayout>,
) {
    let mut changed = false;
    for event in moved.read() {
        let Ok(floating) = floating_windows.get(event.window) else {
            continue;
        };
        changed |= layout.update_floating_geometry(
            floating.0,
            Some([event.position.x as f32, event.position.y as f32]),
            None,
        );
    }
    for event in resized.read() {
        let Ok(floating) = floating_windows.get(event.window) else {
            continue;
        };
        changed |=
            layout.update_floating_geometry(floating.0, None, Some([event.width, event.height]));
    }
    if changed && let Err(error) = layout.save() {
        warn!("failed to save editor workspace layout: {error}");
    }
}

pub(crate) fn sync_native_floating_windows(
    mut commands: Commands,
    session: Res<EditorSession>,
    editor_resources: UiBuildResources,
    windows: Query<(Entity, &NativeFloatingWindow)>,
    cameras: Query<(Entity, &NativeFloatingCamera)>,
    roots: Query<(Entity, &NativeFloatingUi)>,
) {
    for (entity, native) in &windows {
        if editor_resources
            .layout
            .floating
            .iter()
            .all(|floating| floating.panel != native.0)
        {
            commands.entity(entity).despawn();
            for (camera_entity, camera) in &cameras {
                if camera.0 == native.0 {
                    commands.entity(camera_entity).despawn();
                }
            }
            for (root_entity, root) in &roots {
                if root.panel == native.0 {
                    commands.entity(root_entity).despawn();
                }
            }
        }
    }

    let sources = PanelSources {
        session: &session,
        timeline: &editor_resources.timeline,
        catalog: &editor_resources.catalog,
        registry: &editor_resources.registry,
        palette: &editor_resources.palette,
        diagnostics_panel: &editor_resources.diagnostics_panel,
        profiler: &editor_resources.profiler,
        settings: &editor_resources.settings,
        settings_panel: &editor_resources.settings_panel,
        settings_persistence: &editor_resources.settings_persistence,
        localizer: &editor_resources.localizer,
    };
    for floating in &editor_resources.layout.floating {
        if windows.iter().any(|(_, native)| native.0 == floating.panel) {
            continue;
        }
        let window = commands
            .spawn((
                Window {
                    title: format!(
                        "{} — Aestra",
                        editor_resources.localizer.text(floating.panel.message_id())
                    ),
                    resolution: WindowResolution::new(
                        floating.size[0].round() as u32,
                        floating.size[1].round() as u32,
                    ),
                    position: WindowPosition::At(IVec2::new(
                        floating.position[0].round() as i32,
                        floating.position[1].round() as i32,
                    )),
                    resize_constraints: WindowResizeConstraints {
                        min_width: 260.0,
                        min_height: 180.0,
                        ..default()
                    },
                    resizable: true,
                    ..default()
                },
                CursorIcon::default(),
                NativeFloatingWindow(floating.panel),
            ))
            .id();
        let camera = commands
            .spawn((
                Camera2d,
                RenderTarget::Window(WindowRef::Entity(window)),
                RenderLayers::layer(31),
                NativeFloatingCamera(floating.panel),
            ))
            .id();
        spawn_native_floating_ui(
            &mut commands,
            floating.panel,
            window,
            camera,
            &editor_resources.workspace,
            sources,
        );
    }
}

pub(crate) fn spawn_dock_node(
    parent: &mut ChildSpawnerCommands,
    node: &DockNode,
    workspace: &WorkspaceState,
    sources: PanelSources<'_>,
) {
    match node {
        DockNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => {
            let direction = match axis {
                DockAxis::Horizontal => FlexDirection::Row,
                DockAxis::Vertical => FlexDirection::Column,
            };
            parent
                .spawn(Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    flex_direction: direction,
                    ..default()
                })
                .with_children(|split| {
                    split
                        .spawn((
                            DockFirstPane(*id),
                            Node {
                                width: if *axis == DockAxis::Horizontal {
                                    Val::Percent(*ratio * 100.0)
                                } else {
                                    Val::Percent(100.0)
                                },
                                height: if *axis == DockAxis::Vertical {
                                    Val::Percent(*ratio * 100.0)
                                } else {
                                    Val::Percent(100.0)
                                },
                                min_width: Val::Px(0.0),
                                min_height: Val::Px(0.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ))
                        .with_children(|pane| {
                            spawn_dock_node(pane, first, workspace, sources);
                        });
                    spawn_tree_splitter(split, *id, *axis);
                    split
                        .spawn(Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            ..default()
                        })
                        .with_children(|pane| {
                            spawn_dock_node(pane, second, workspace, sources);
                        });
                });
        }
        DockNode::Tabs { id, stack } => {
            spawn_dock_stack(parent, *id, stack, workspace, sources);
        }
    }
}

fn spawn_dock_stack(
    parent: &mut ChildSpawnerCommands,
    node: DockNodeId,
    stack: &DockStack,
    workspace: &WorkspaceState,
    sources: PanelSources<'_>,
) {
    parent
        .spawn((
            DockPane(node),
            RelativeCursorPosition::default(),
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
        ))
        .apply_scene(ui_shell::dock_pane())
        .insert(BackgroundColor(dock_pane_background(stack.active)))
        .with_children(|pane| {
            spawn_dock_tab_bar(pane, node, stack, sources.localizer);
            if let Some(panel) = stack.active {
                spawn_panel_content(pane, panel, workspace, sources);
            }
            spawn_dock_drop_overlay(pane, node);
        });
}

pub(crate) fn dock_pane_background(active: Option<DockPanel>) -> Color {
    if active == Some(DockPanel::Viewport) {
        Color::NONE
    } else {
        theme::PANEL_DARK
    }
}

fn spawn_panel_content(
    parent: &mut ChildSpawnerCommands,
    panel: DockPanel,
    workspace: &WorkspaceState,
    sources: PanelSources<'_>,
) {
    match panel {
        DockPanel::Viewport => viewport::spawn_preview(parent, sources.localizer),
        DockPanel::Assets => spawn_asset_browser(parent, sources.session, sources.catalog),
        DockPanel::Inspector => {
            spawn_inspector(
                parent,
                sources.session,
                sources.registry,
                sources.palette,
                sources.localizer,
                sources.settings,
            );
        }
        DockPanel::Timeline => timeline::spawn_timeline(parent, sources.session, sources.timeline),
        DockPanel::Curves => {
            spawn_curves_workspace(parent, sources.session, sources.registry, workspace);
        }
        DockPanel::Diagnostics => {
            spawn_diagnostics_workspace(parent, sources.session, sources.diagnostics_panel);
        }
        DockPanel::GeneratedCode => spawn_generated_code_workspace(parent, sources.session),
        DockPanel::Profiler => spawn_profiler_workspace(parent, sources.session, sources.profiler),
        DockPanel::Changes => spawn_changes_workspace(parent, sources.session),
        DockPanel::Settings => spawn_settings_workspace(
            parent,
            sources.settings,
            sources.settings_panel,
            sources.settings_persistence,
            sources.localizer,
        ),
    }
}

pub(crate) fn spawn_native_floating_ui(
    commands: &mut Commands,
    panel: DockPanel,
    window: Entity,
    camera: Entity,
    workspace: &WorkspaceState,
    sources: PanelSources<'_>,
) {
    commands
        .spawn((
            NativeFloatingUi {
                panel,
                window,
                camera,
            },
            UiTargetCamera(camera),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
        ))
        .with_children(|root| {
            spawn_panel_content(root, panel, workspace, sources);
        });
}

fn spawn_dock_drop_overlay(parent: &mut ChildSpawnerCommands, node: DockNodeId) {
    parent
        .spawn((
            DockDropHint(node),
            Visibility::Hidden,
            Pickable::IGNORE,
            GlobalZIndex(80),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(32.0),
                bottom: Val::Px(0.0),
                ..default()
            },
        ))
        .with_children(|overlay| {
            spawn_dock_drop_zone(overlay, node, DockDrop::Left);
            spawn_dock_drop_zone(overlay, node, DockDrop::Right);
            spawn_dock_drop_zone(overlay, node, DockDrop::Top);
            spawn_dock_drop_zone(overlay, node, DockDrop::Bottom);
        });
}

fn spawn_dock_drop_zone(parent: &mut ChildSpawnerCommands, node: DockNodeId, drop: DockDrop) {
    let (node_style, label) = match drop {
        DockDrop::Left => (
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Percent(25.0),
                width: Val::Percent(50.0),
                height: Val::Percent(50.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            "SPLIT LEFT",
        ),
        DockDrop::Right => (
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Percent(25.0),
                width: Val::Percent(50.0),
                height: Val::Percent(50.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            "SPLIT RIGHT",
        ),
        DockDrop::Top => (
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                height: Val::Percent(25.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            "SPLIT ABOVE",
        ),
        DockDrop::Bottom => (
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                height: Val::Percent(25.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            "SPLIT BELOW",
        ),
        DockDrop::Center => return,
    };
    parent
        .spawn((
            DockDropZone { node, drop },
            Interaction::None,
            RelativeCursorPosition::default(),
            Pickable::default(),
            node_style,
            BackgroundColor(theme::DOCK_TARGET_IDLE),
        ))
        .observe(dock_panel_drop)
        .with_children(|zone| {
            zone.spawn((
                DockDropZoneLabel,
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::DOCK_TARGET_TEXT_IDLE),
                Node {
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(theme::DOCK_TARGET_LABEL_IDLE),
                Pickable::IGNORE,
            ));
        });
}

fn spawn_tree_splitter(parent: &mut ChildSpawnerCommands, node: DockNodeId, axis: DockAxis) {
    let splitter = DockSplitter { node, axis };
    let horizontal_bar = axis == DockAxis::Vertical;
    parent
        .spawn(splitter)
        .apply_scene(ui_shell::splitter(horizontal_bar))
        .observe(begin_workspace_resize)
        .observe(resize_workspace_pane)
        .observe(finish_workspace_resize)
        .observe(show_resize_cursor)
        .observe(reset_cursor)
        .with_children(|gutter| {
            gutter.spawn((
                SplitterGrip,
                Node {
                    width: if horizontal_bar {
                        Val::Px(52.0)
                    } else {
                        Val::Px(2.0)
                    },
                    height: if horizontal_bar {
                        Val::Px(2.0)
                    } else {
                        Val::Px(52.0)
                    },
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(theme::SPLITTER),
                Pickable::IGNORE,
            ));
        });
}

fn spawn_dock_tab_bar(
    parent: &mut ChildSpawnerCommands,
    node: DockNodeId,
    stack: &DockStack,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(32.0),
                align_items: AlignItems::End,
                padding: UiRect::horizontal(Val::Px(4.0)),
                column_gap: Val::Px(2.0),
                border: UiRect::bottom(Val::Px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme::MENU),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
        .with_children(|bar| {
            for panel in &stack.tabs {
                spawn_dock_tab(bar, *panel, stack.active == Some(*panel), localizer);
            }
            bar.spawn((
                DockTabAppendZone(node),
                RelativeCursorPosition::default(),
                Pickable::default(),
                Node {
                    height: Val::Percent(100.0),
                    min_width: Val::Px(28.0),
                    flex_grow: 1.0,
                    align_items: AlignItems::Center,
                    padding: UiRect::left(Val::Px(2.0)),
                    ..default()
                },
            ))
            .observe(append_dock_tab)
            .with_children(|zone| {
                zone.spawn((
                    DockTabAppendIndicator(node),
                    Visibility::Hidden,
                    Node {
                        width: Val::Px(4.0),
                        height: Val::Px(24.0),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(theme::DOCK_TARGET),
                    Pickable::IGNORE,
                ));
            });
        });
}

fn spawn_dock_tab(
    parent: &mut ChildSpawnerCommands,
    panel: DockPanel,
    selected: bool,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Button,
            EditorNativeControl,
            EditorAction::SelectDockPanel(panel),
            DockTab(panel),
            RelativeCursorPosition::default(),
            Pickable {
                should_block_lower: false,
                is_hoverable: true,
            },
            Node {
                height: Val::Px(28.0),
                min_width: Val::Px(92.0),
                padding: UiRect::left(Val::Px(9.0)),
                align_items: AlignItems::Center,
                border: UiRect::new(
                    Val::Px(1.0),
                    Val::Px(1.0),
                    Val::Px(1.0),
                    Val::Px(if selected { 0.0 } else { 1.0 }),
                ),
                border_radius: BorderRadius::top(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::PANEL
            } else {
                theme::PANEL_DARK
            }),
            BorderColor::all(if selected {
                theme::ACCENT_DIM
            } else {
                theme::BORDER
            }),
        ))
        .observe(begin_dock_tab_drag)
        .observe(move_dock_tab)
        .observe(reset_dock_tab)
        .observe(reorder_dock_tab)
        .observe(open_dock_tab_context_menu)
        .with_children(|tab| {
            tab.spawn((
                LocalizedText(panel.message_id()),
                Text::new(localizer.text(panel.message_id())),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Pickable::IGNORE,
            ));
            tab.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if panel.closable() {
                tab.spawn((
                    Button,
                    EditorNativeControl,
                    EditorAction::CloseDockPanel(panel),
                    DockCloseButton,
                    Node {
                        width: Val::Px(24.0),
                        height: Val::Percent(100.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|close| {
                    close.spawn((
                        Text::new("x"),
                        TextFont {
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                        Pickable::IGNORE,
                    ));
                });
            }
        });
}

fn resize_workspace_pane(
    drag: On<Pointer<Drag>>,
    mut queries: DockResizeQueries,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
    mut layout: ResMut<WorkspaceLayout>,
) {
    let Ok(splitter) = queries.splitters.get(drag.event_target()) else {
        return;
    };
    let Ok(parent) = queries.parents.get(drag.event_target()) else {
        return;
    };
    let Ok(parent_node) = queries.computed.get(parent.parent()) else {
        return;
    };
    let scale = window.scale_factor().max(0.01);
    let (delta, span) = match splitter.axis {
        DockAxis::Horizontal => (drag.delta.x / scale, parent_node.size().x / scale),
        DockAxis::Vertical => (drag.delta.y / scale, parent_node.size().y / scale),
    };
    if !layout.resize_split(splitter.node, delta, span) {
        return;
    }
    let Some(DockNode::Split { ratio, .. }) = find_dock_node(&layout.root, splitter.node) else {
        return;
    };
    for (pane, mut node) in &mut queries.first_panes {
        if pane.0 != splitter.node {
            continue;
        }
        match splitter.axis {
            DockAxis::Horizontal => node.width = Val::Percent(*ratio * 100.0),
            DockAxis::Vertical => node.height = Val::Percent(*ratio * 100.0),
        }
    }
    **cursor = resize_cursor(*splitter);
    if let Ok(mut color) = queries.colors.get_mut(drag.event_target()) {
        color.0 = theme::SPLITTER_HOVER;
    }
}

fn find_dock_node(node: &DockNode, target: DockNodeId) -> Option<&DockNode> {
    if node.id() == target {
        return Some(node);
    }
    match node {
        DockNode::Split { first, second, .. } => {
            find_dock_node(first, target).or_else(|| find_dock_node(second, target))
        }
        DockNode::Tabs { .. } => None,
    }
}

fn begin_workspace_resize(
    drag: On<Pointer<DragStart>>,
    splitters: Query<&DockSplitter>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
    mut colors: Query<&mut BackgroundColor, With<DockSplitter>>,
    mut state: ResMut<ResizeState>,
) {
    let Ok(splitter) = splitters.get(drag.event_target()) else {
        return;
    };
    state.0 = Some(*splitter);
    **cursor = resize_cursor(*splitter);
    if let Ok(mut color) = colors.get_mut(drag.event_target()) {
        color.0 = theme::SPLITTER_HOVER;
    }
}

fn finish_workspace_resize(
    drag: On<Pointer<DragEnd>>,
    splitters: Query<&DockSplitter>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
    mut colors: Query<&mut BackgroundColor, With<DockSplitter>>,
    mut state: ResMut<ResizeState>,
    layout: Res<WorkspaceLayout>,
) {
    if !splitters.contains(drag.event_target()) {
        return;
    }
    state.0 = None;
    **cursor = CursorIcon::System(SystemCursorIcon::Default);
    for mut color in &mut colors {
        color.0 = theme::SPLITTER_GUTTER;
    }
    if let Err(error) = layout.save() {
        warn!("failed to save editor workspace layout: {error}");
    }
}

fn resize_cursor(splitter: DockSplitter) -> CursorIcon {
    CursorIcon::System(match splitter.axis {
        DockAxis::Horizontal => SystemCursorIcon::EwResize,
        DockAxis::Vertical => SystemCursorIcon::NsResize,
    })
}

fn show_resize_cursor(
    over: On<Pointer<Over>>,
    splitters: Query<&DockSplitter>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
    mut colors: Query<&mut BackgroundColor, With<DockSplitter>>,
) {
    let Ok(splitter) = splitters.get(over.event_target()) else {
        return;
    };
    **cursor = resize_cursor(*splitter);
    if let Ok(mut color) = colors.get_mut(over.event_target()) {
        color.0 = theme::SPLITTER_HOVER;
    }
}

fn reset_cursor(
    out: On<Pointer<Out>>,
    splitters: Query<&DockSplitter>,
    state: Res<ResizeState>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
    mut colors: Query<&mut BackgroundColor, With<DockSplitter>>,
) {
    if splitters.contains(out.event_target()) && state.0.is_none() {
        **cursor = CursorIcon::System(SystemCursorIcon::Default);
        if let Ok(mut color) = colors.get_mut(out.event_target()) {
            color.0 = theme::SPLITTER_GUTTER;
        }
    }
}

fn move_dock_tab(drag: On<Pointer<Drag>>, mut tabs: Query<&mut UiTransform, With<DockTab>>) {
    if let Ok(mut transform) = tabs.get_mut(drag.event_target()) {
        transform.translation = Val2::px(drag.distance.x, drag.distance.y);
    }
}

fn begin_dock_tab_drag(
    drag: On<Pointer<DragStart>>,
    tabs: Query<&DockTab>,
    mut commands: Commands,
    mut state: ResMut<DockDragState>,
) {
    if let Ok(tab) = tabs.get(drag.event_target()) {
        state.0 = Some(tab.0);
        commands
            .entity(drag.event_target())
            .insert(GlobalZIndex(160));
    }
}

fn reset_dock_tab(
    drag: On<Pointer<DragEnd>>,
    mut tabs: Query<&mut UiTransform, With<DockTab>>,
    mut commands: Commands,
    mut state: ResMut<DockDragState>,
) {
    if let Ok(mut transform) = tabs.get_mut(drag.event_target()) {
        transform.translation = Val2::ZERO;
        commands
            .entity(drag.event_target())
            .remove::<GlobalZIndex>();
    }
    state.0 = None;
}

pub(crate) fn clear_finished_dock_drag(
    buttons: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<DockDragState>,
    mut tabs: Query<(Entity, &mut UiTransform), With<DockTab>>,
    mut commands: Commands,
) {
    if state.0.is_none() || buttons.pressed(MouseButton::Left) {
        return;
    }
    state.0 = None;
    for (entity, mut transform) in &mut tabs {
        transform.translation = Val2::ZERO;
        commands.entity(entity).remove::<GlobalZIndex>();
    }
}

fn open_dock_tab_context_menu(
    mut click: On<Pointer<Click>>,
    tabs: Query<&DockTab>,
    parents: Query<&ChildOf>,
    layout: Res<WorkspaceLayout>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let mut entity = click.event_target();
    let tab = loop {
        if let Ok(tab) = tabs.get(entity) {
            break tab;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    if tab.0 == DockPanel::Viewport
        || layout
            .floating
            .iter()
            .any(|floating| floating.panel == tab.0)
    {
        return;
    }
    menu.open = None;
    let maximum_x = (window.width() - 188.0).max(0.0);
    let maximum_y = (window.height() - 148.0).max(0.0);
    menu.tab_context = Some(TabContextMenu {
        panel: tab.0,
        position: [
            click.pointer_location.position.x.clamp(0.0, maximum_x),
            (click.pointer_location.position.y - 84.0).clamp(0.0, maximum_y),
        ],
    });
    session.ui_revision += 1;
    click.propagate(false);
}

fn dock_panel_drop(
    mut drop: On<Pointer<DragDrop>>,
    queries: DockDropQueries,
    mut drag_state: ResMut<DockDragState>,
    mut layout: ResMut<WorkspaceLayout>,
    mut session: ResMut<EditorSession>,
) {
    let mut target_entity = drop.event_target();
    let zone = loop {
        if let Ok(zone) = queries.zones.get(target_entity) {
            break zone;
        }
        let Ok(parent) = queries.parents.get(target_entity) else {
            return;
        };
        target_entity = parent.parent();
    };
    let mut tab_entity = drop.dropped;
    let tab = loop {
        if let Ok(tab) = queries.tabs.get(tab_entity) {
            break tab;
        }
        let Ok(parent) = queries.parents.get(tab_entity) else {
            return;
        };
        tab_entity = parent.parent();
    };
    drag_state.0 = None;
    if layout.dock(tab.0, zone.node, zone.drop) {
        if let Err(error) = layout.save() {
            warn!("failed to save editor workspace layout: {error}");
        }
        session.ui_revision += 1;
        session.status = format!("Docked {} panel", tab.0.title().to_ascii_lowercase());
    }
    drop.propagate(false);
}

fn reorder_dock_tab(
    mut drop: On<Pointer<DragDrop>>,
    tabs: Query<(&DockTab, &RelativeCursorPosition)>,
    parents: Query<&ChildOf>,
    mut drag_state: ResMut<DockDragState>,
    mut layout: ResMut<WorkspaceLayout>,
    mut session: ResMut<EditorSession>,
) {
    let mut target_entity = drop.event_target();
    let (target, cursor) = loop {
        if let Ok(tab) = tabs.get(target_entity) {
            break tab;
        }
        let Ok(parent) = parents.get(target_entity) else {
            return;
        };
        target_entity = parent.parent();
    };
    let mut source_entity = drop.dropped;
    let source = loop {
        if let Ok((tab, _)) = tabs.get(source_entity) {
            break tab;
        }
        let Ok(parent) = parents.get(source_entity) else {
            return;
        };
        source_entity = parent.parent();
    };
    if source.0 == target.0 {
        return;
    }
    let before = cursor.normalized.is_none_or(|position| position.x < 0.5);
    drag_state.0 = None;
    if layout.reorder_tab(source.0, target.0, before) {
        if let Err(error) = layout.save() {
            warn!("failed to save editor workspace layout: {error}");
        }
        session.ui_revision += 1;
        session.status = format!(
            "Moved {} {} {}",
            source.0.title().to_ascii_lowercase(),
            if before { "before" } else { "after" },
            target.0.title().to_ascii_lowercase()
        );
    }
    drop.propagate(false);
}

fn append_dock_tab(
    mut drop: On<Pointer<DragDrop>>,
    append_zones: Query<&DockTabAppendZone>,
    tabs: Query<&DockTab>,
    parents: Query<&ChildOf>,
    mut drag_state: ResMut<DockDragState>,
    mut layout: ResMut<WorkspaceLayout>,
    mut session: ResMut<EditorSession>,
) {
    let Ok(zone) = append_zones.get(drop.event_target()) else {
        return;
    };
    let mut source_entity = drop.dropped;
    let source = loop {
        if let Ok(tab) = tabs.get(source_entity) {
            break tab;
        }
        let Ok(parent) = parents.get(source_entity) else {
            return;
        };
        source_entity = parent.parent();
    };
    drag_state.0 = None;
    if layout.dock(source.0, zone.0, DockDrop::Center) {
        if let Err(error) = layout.save() {
            warn!("failed to save editor workspace layout: {error}");
        }
        session.ui_revision += 1;
        session.status = format!(
            "Moved {} to the end of the tab strip",
            source.0.title().to_ascii_lowercase()
        );
    }
    drop.propagate(false);
}

pub(crate) fn sync_dock_drop_hints(
    state: Res<DockDragState>,
    panes: Query<(&DockPane, &RelativeCursorPosition, &ComputedNode)>,
    mut hints: Query<(&DockDropHint, &mut Visibility)>,
) {
    let hovered = state.0.and_then(|_| {
        panes
            .iter()
            .filter(|(_, cursor, _)| cursor.cursor_over())
            .min_by(|(_, _, left), (_, _, right)| {
                left.size()
                    .element_product()
                    .total_cmp(&right.size().element_product())
            })
            .map(|(pane, _, _)| pane.0)
    });
    for (hint, mut visibility) in &mut hints {
        *visibility = if hovered == Some(hint.0) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn sync_tab_reorder_hints(
    state: Res<DockDragState>,
    layout: Res<WorkspaceLayout>,
    mut tabs: Query<(
        &DockTab,
        &RelativeCursorPosition,
        &mut Node,
        &mut BorderColor,
    )>,
) {
    for (tab, cursor, mut node, mut border) in &mut tabs {
        let base = if layout.is_active(tab.0) {
            theme::ACCENT_DIM
        } else {
            theme::BORDER
        };
        node.border.left = Val::Px(1.0);
        node.border.right = Val::Px(1.0);
        border.left = base;
        border.right = base;

        if state.0.is_none_or(|dragged| dragged == tab.0) || !cursor.cursor_over() {
            continue;
        }
        let before = cursor.normalized.is_none_or(|position| position.x < 0.5);
        if before {
            node.border.left = Val::Px(4.0);
            border.left = theme::DOCK_TARGET;
        } else {
            node.border.right = Val::Px(4.0);
            border.right = theme::DOCK_TARGET;
        }
    }
}

pub(crate) fn sync_tab_append_hint(
    state: Res<DockDragState>,
    zones: Query<(&DockTabAppendZone, &RelativeCursorPosition)>,
    mut indicators: Query<(&DockTabAppendIndicator, &mut Visibility)>,
) {
    for (indicator, mut visibility) in &mut indicators {
        let hovered = state.0.is_some()
            && zones
                .iter()
                .any(|(zone, cursor)| zone.0 == indicator.0 && cursor.cursor_over());
        *visibility = if hovered {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn update_dock_zone_style(
    state: Res<DockDragState>,
    mut zones: Query<
        (&RelativeCursorPosition, &Children, &mut BackgroundColor),
        With<DockDropZone>,
    >,
    mut labels: Query<
        (&mut TextColor, &mut BackgroundColor),
        (With<DockDropZoneLabel>, Without<DockDropZone>),
    >,
) {
    for (cursor, children, mut background) in &mut zones {
        let hovered = state.0.is_some() && cursor.cursor_over();
        background.0 = if hovered {
            theme::DOCK_TARGET_HOVER
        } else {
            theme::DOCK_TARGET_IDLE
        };
        for child in children.iter() {
            if let Ok((mut text, mut label_background)) = labels.get_mut(child) {
                text.0 = if hovered {
                    theme::TEXT
                } else {
                    theme::DOCK_TARGET_TEXT_IDLE
                };
                label_background.0 = if hovered {
                    theme::DOCK_TARGET_LABEL
                } else {
                    theme::DOCK_TARGET_LABEL_IDLE
                };
            }
        }
    }
}
