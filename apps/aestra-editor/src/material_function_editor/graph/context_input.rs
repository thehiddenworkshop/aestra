//! Production function context-menu hit testing, dispatch and dismissal.
#[cfg(test)]
#[path = "arrange_context_tests.rs"]
mod arrange_context_tests;
#[cfg(test)]
#[path = "context_input/tests.rs"]
mod tests;
use crate::feathers::{
    context_menu::{
        PointerContextSubmenuSurface, pointer_position_in_node, should_dismiss_pointer_context_menu,
    },
    node_graph::{
        geometry::{GraphDocumentKey, GraphNodeKey, GraphViewKey},
        *,
    },
};
use crate::material_function_editor::graph::{
    clipboard,
    clipboard_context::{FunctionGraphNodeAction, View, ViewScope, activate_function_graph_target},
    connection::socket_entity,
    menu_state::{FunctionGraphMenuKind, FunctionGraphMenuOpen, FunctionGraphMenuState, Target},
    model::*,
    modified_input::duplicate_function_selection,
    socket_palette,
};
use crate::material_function_editor::{FunctionEditor, signature_input::finish};
use crate::material_graph::click_input::{GraphClick, PointerClick};
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_authoring::{MaterialExpressionInput, MaterialFunctionBodyCommand as Edit};
use aestra_core::{
    MaterialFunctionId,
    material::{MaterialExpression, MaterialExpressionKind},
};
use bevy::{prelude::*, ui::RelativeCursorPosition, ui_widgets::Activate};
use std::collections::BTreeSet;

pub(super) fn register(app: &mut App) {
    app.add_observer(open_function_graph_palette)
        .add_observer(open_function_graph_pin_menu)
        .add_observer(open_function_graph_node_menu)
        .add_observer(handle_function_graph_context_action);
}
pub(super) fn register_dismissal(app: &mut App) {
    app.add_systems(Update, dismiss_function_graph_menu);
}

fn function_socket_positions(
    viewport: Entity,
    owner: MaterialFunctionId,
    sockets: &Query<(&Socket, &UiGlobalTransform, &Anchor)>,
    graph_nodes: &Query<(&FeathersGraphNode, &ComputedNode, &UiGlobalTransform)>,
    parents: &Query<&ChildOf>,
) -> Vec<(SocketKind, Vec2)> {
    sockets
        .iter()
        .filter(|(socket, _, _)| {
            socket.owner == owner
                && parents
                    .iter_ancestors(socket.node)
                    .any(|ancestor| ancestor == viewport)
        })
        .filter_map(|(socket, transform, anchor)| {
            let (node, computed, node_transform) = graph_nodes.get(socket.node).ok()?;
            let (_, _, world) = transform.to_scale_angle_translation();
            let offset = crate::feathers::node_graph::compact_graph_socket_offset(
                node,
                computed,
                socket.kind.side(),
            )
            .unwrap_or_else(|| {
                anchor.0.unwrap_or_else(|| {
                    crate::material_graph::viewport_local_position(computed, node_transform, world)
                })
            });
            Some((socket.kind, node.position() + offset))
        })
        .collect()
}

fn function_socket_position(positions: &[(SocketKind, Vec2)], kind: SocketKind) -> Option<Vec2> {
    positions
        .iter()
        .find_map(|(candidate, position)| (*candidate == kind).then_some(*position))
}

fn open_function_graph_palette(
    mut click: On<PointerClick>,
    views: Query<(
        &View,
        &ViewScope,
        &FeathersGraphViewport,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    graph_nodes: Query<(&FeathersGraphNode, &ComputedNode, &UiGlobalTransform)>,
    sockets: Query<(&Socket, &UiGlobalTransform, &Anchor)>,
    wires: Query<(Entity, &Wire)>,
    controls: Query<
        (),
        Or<(
            With<FunctionGraphNodeAction>,
            With<Socket>,
            With<Constant>,
            With<BodyAction>,
            With<socket_palette::Surface>,
            With<FunctionGraphContextMenu>,
        )>,
    >,
    parents: Query<&ChildOf>,
    mut menus: ResMut<FunctionGraphMenuState>,
    mut session: ResMut<EditorSession>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let mut entity = click.event_target();
    let viewport = loop {
        if controls.contains(entity) {
            return;
        }
        if let Ok((view, scope, viewport, computed, transform)) = views.get(entity) {
            let position =
                pointer_position_in_node(click.graph_click_position(), computed, transform);
            let socket_positions =
                function_socket_positions(entity, view.0, &sockets, &graph_nodes, &parents);
            if let Some((source, target)) = wires
                .iter()
                .filter(|(wire_entity, wire)| {
                    wire.owner == view.0
                        && parents
                            .iter_ancestors(*wire_entity)
                            .any(|ancestor| ancestor == entity)
                })
                .filter_map(|(_, wire)| {
                    let start = function_socket_position(
                        &socket_positions,
                        SocketKind::Source(wire.source),
                    )?;
                    let end = function_socket_position(
                        &socket_positions,
                        SocketKind::Target(wire.target),
                    )?;
                    let distance = crate::material_graph::distance_to_graph_wire(
                        position,
                        viewport.project_graph_point(start),
                        viewport.project_graph_point(end),
                    );
                    (distance <= 9.0).then_some((distance, wire.source, wire.target))
                })
                .min_by(|left, right| left.0.total_cmp(&right.0))
                .map(|(_, source, target)| (source, target))
            {
                menus.open = Some(FunctionGraphMenuOpen {
                    owner: view.0,
                    scope: scope.0,
                    position,
                    kind: FunctionGraphMenuKind::Connections(vec![(source, target)]),
                });
                session.ui_revision += 1;
                click.propagate(false);
                return;
            }
            break entity;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let pointer = click.graph_click_position();
    commands.queue(move |world: &mut World| socket_palette::open_canvas(world, viewport, pointer));
    click.propagate(false);
}

fn open_function_graph_pin_menu(
    mut click: On<PointerClick>,
    sockets: Query<(Entity, &Socket)>,
    views: Query<(&View, &ViewScope, &ComputedNode, &UiGlobalTransform)>,
    wires: Query<(Entity, &Wire)>,
    parents: Query<&ChildOf>,
    mut menus: ResMut<FunctionGraphMenuState>,
    mut session: ResMut<EditorSession>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let Some(socket_entity) = socket_entity(click.event_target(), &sockets, &parents) else {
        return;
    };
    let Ok((_, socket)) = sockets.get(socket_entity) else {
        return;
    };
    let mut ancestor = socket_entity;
    let (viewport_entity, scope, computed, transform) = loop {
        if let Ok((_, scope, computed, transform)) = views.get(ancestor) {
            break (ancestor, *scope, computed, transform);
        }
        let Ok(parent) = parents.get(ancestor) else {
            return;
        };
        ancestor = parent.parent();
    };
    let connections = wires
        .iter()
        .filter(|(wire_entity, wire)| {
            wire.owner == socket.owner
                && parents
                    .iter_ancestors(*wire_entity)
                    .any(|ancestor| ancestor == viewport_entity)
                && match socket.kind {
                    SocketKind::Source(source) => wire.source == source,
                    SocketKind::Target(target) => wire.target == target,
                }
        })
        .map(|(_, wire)| (wire.source, wire.target))
        .collect::<Vec<_>>();
    if connections.is_empty() {
        let pointer = click.graph_click_position();
        commands
            .queue(move |world: &mut World| socket_palette::open(world, socket_entity, pointer));
    } else {
        menus.open = Some(FunctionGraphMenuOpen {
            owner: socket.owner,
            scope: scope.0,
            position: pointer_position_in_node(click.graph_click_position(), computed, transform),
            kind: FunctionGraphMenuKind::Connections(connections),
        });
        session.ui_revision += 1;
    }
    click.propagate(false);
}

fn open_function_graph_node_menu(
    mut click: On<PointerClick>,
    actions: Query<&FunctionGraphNodeAction>,
    sockets: Query<(), With<Socket>>,
    views: Query<(&View, &ViewScope, &ComputedNode, &UiGlobalTransform)>,
    parents: Query<&ChildOf>,
    mut selection: ResMut<crate::material_graph::MaterialGraphSelectionState>,
    mut menus: ResMut<FunctionGraphMenuState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let mut entity = click.event_target();
    let action = loop {
        if sockets.contains(entity) {
            return;
        }
        if let Ok(action) = actions.get(entity) {
            break *action;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let mut ancestor = entity;
    let (scope, computed, transform) = loop {
        if let Ok((_, scope, computed, transform)) = views.get(ancestor) {
            break (*scope, computed, transform);
        }
        let Ok(parent) = parents.get(ancestor) else {
            return;
        };
        ancestor = parent.parent();
    };
    if !selection.is_function_expression_selected(action.scope, action.owner, action.expression) {
        selection.select_function_expression(
            action.scope,
            action.owner,
            action.expression,
            false,
            false,
        );
    }
    menus.open = Some(FunctionGraphMenuOpen {
        owner: action.owner,
        scope: scope.0,
        position: pointer_position_in_node(click.graph_click_position(), computed, transform),
        kind: FunctionGraphMenuKind::Node(action.expression),
    });
    session.ui_revision += 1;
    click.propagate(false);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_function_graph_context_action(
    event: On<Activate>,
    actions: Query<&FunctionGraphContextAction>,
    graph_nodes: Query<(&FunctionGraphNodeAction, &FeathersGraphNode)>,
    graph_views: Query<(&View, &ViewScope, &FeathersGraphViewport)>,
    menu_surfaces: Query<&ChildOf, With<FunctionGraphContextMenu>>,
    mut menus: ResMut<FunctionGraphMenuState>,
    mut selection: ResMut<crate::material_graph::MaterialGraphSelectionState>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut memory: ResMut<GraphViewportMemory>,
    mut clipboard: ResMut<crate::material_graph::clipboard::GraphClipboard>,
    mut commands: Commands,
) {
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    let Some(open) = menus.open.clone() else {
        return;
    };
    if let FunctionGraphContextAction::Arrange(scope) = *action {
        commands.trigger(crate::material_graph::arrange::ArrangeGraph {
            view: GraphViewKey {
                document: GraphDocumentKey {
                    project: catalog.root().to_owned(),
                    asset: crate::document::DocumentKey::MaterialFunction(open.owner),
                },
                view: open.scope,
            },
            editing_target: crate::material_document::MaterialEditingTarget::Function {
                root: catalog.root().to_owned(),
                id: open.owner,
            },
            scope,
            seeds: selection.function_arrange_seeds(open.scope, open.owner),
        });
        menus.open = None;
        // Keep the mounted graph and its measurement token intact until arrangement completes.
        for parent in &menu_surfaces {
            commands.entity(parent.parent()).despawn();
        }
        return;
    }
    if !activate_function_graph_target(&mut session, &catalog, open.owner) {
        menus.open = None;
        return;
    }
    match *action {
        FunctionGraphContextAction::Clipboard(action) => {
            let anchor = graph_views
                .iter()
                .find(|(view, scope, _)| view.0 == open.owner && scope.0 == open.scope)
                .map(|(_, _, graph)| graph.unproject_viewport_point(open.position));
            session.status = clipboard::execute(
                Some(action),
                false,
                open.owner,
                open.scope,
                anchor,
                &graph_nodes,
                &mut clipboard,
                &mut selection,
                &mut editor,
                &mut session,
                &mut catalog,
                &mut memory,
            )
            .unwrap_or_else(|error| format!("Could not edit function nodes: {error}"));
        }
        FunctionGraphContextAction::Arrange(_) => {
            unreachable!("handled before semantic activation")
        }
        FunctionGraphContextAction::Open(expression) => {
            if let Ok(function) = session.graph_function(&catalog)
                && let Some(MaterialExpression {
                    kind:
                        MaterialExpressionKind::FunctionCall {
                            function: aestra_core::material::MaterialFunctionRef::Project(function),
                            ..
                        },
                    ..
                }) = function
                    .expressions
                    .iter()
                    .find(|candidate| candidate.id == expression)
            {
                commands.trigger(crate::asset_browser::OpenFunction {
                    function: *function,
                    new_view: false,
                });
            }
        }
        FunctionGraphContextAction::Duplicate(expression) => {
            match duplicate_function_selection(
                open.owner,
                open.scope,
                expression,
                Vec2::splat(24.0),
                &graph_nodes,
                &mut editor,
                &mut session,
                &mut catalog,
                &mut memory,
                &mut selection,
            ) {
                Ok(count) => session.status = format!("Duplicated {count} function node(s)"),
                Err(error) => {
                    session.status = format!("Could not duplicate function nodes: {error}")
                }
            }
        }
        FunctionGraphContextAction::Delete(expression) => {
            let mut selected = selection
                .function_arrange_seeds(open.scope, open.owner)
                .into_iter()
                .filter_map(|key| match key {
                    GraphNodeKey::Expression(id) => Some(id),
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            if selected.is_empty() {
                selected.insert(expression);
            }
            let graph_key =
                crate::material_graph::function_graph_memory_key(catalog.root(), open.owner);
            let before = crate::material_graph::presentation::Snapshot::capture(
                &graph_key, &catalog, &session, &memory,
            );
            let result = editor.edit_body(
                &mut session,
                &mut catalog,
                selected
                    .iter()
                    .rev()
                    .map(|expression| Edit::Remove {
                        expression: *expression,
                    })
                    .collect(),
            );
            match result {
                Ok(()) => {
                    for expression in &selected {
                        memory.remove_node(&graph_key, &expression.to_string());
                    }
                    if let Some(before) = before {
                        before.attach(&catalog, &mut session, &mut memory);
                    }
                    selection.clear_function_selection(open.scope, open.owner);
                    session.status = format!("Deleted {} function node(s)", selected.len());
                }
                Err(error) => session.status = format!("Could not delete function nodes: {error}"),
            }
        }
        FunctionGraphContextAction::Disconnect(target) => {
            let result = match target {
                Target::Input(expression, MaterialExpressionInput::FunctionArgument(input)) => {
                    editor.edit_body(
                        &mut session,
                        &mut catalog,
                        vec![Edit::SetArgument {
                            expression,
                            input,
                            source: None,
                        }],
                    )
                }
                _ => Err("This required connection needs a replacement source".into()),
            };
            finish(&mut session, result);
        }
    }
    menus.open = None;
    session.ui_revision += 1;
}

pub(super) fn dismiss_function_graph_menu(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    surfaces: Query<
        &RelativeCursorPosition,
        Or<(
            With<FunctionGraphContextMenu>,
            With<PointerContextSubmenuSurface>,
        )>,
    >,
    mut menus: ResMut<FunctionGraphMenuState>,
    mut session: ResMut<EditorSession>,
) {
    if should_dismiss_pointer_context_menu(
        menus.open.is_some(),
        buttons.just_pressed(MouseButton::Left),
        keys.just_pressed(KeyCode::Escape),
        surfaces.iter().any(RelativeCursorPosition::cursor_over),
    ) {
        menus.open = None;
        session.ui_revision += 1;
    }
}
