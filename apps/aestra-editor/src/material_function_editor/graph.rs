//! Function-native canvas: no surrogate material program or effect is created.
use super::*;
use crate::feathers::context_menu::{
    PointerContextSubmenuSurface, pointer_position_in_node, should_dismiss_pointer_context_menu,
    spawn_pointer_context_menu_item, spawn_pointer_context_menu_shortcut_item,
    spawn_pointer_context_menu_sized, spawn_pointer_context_submenu,
};
use crate::feathers::node_graph::geometry::{
    GraphDocumentKey, GraphGeometryNode, GraphGeometryPort, GraphGeometryView, GraphNodeKey,
    GraphViewKey,
};
use crate::feathers::{
    combo_box::{ComboOption, spawn_compact_action_menu, spawn_searchable_icon_action_menu},
    icon::load_svg_icon,
    node_graph::*,
};
use aestra_authoring::{
    MaterialConnectionTarget, MaterialExpressionInput, MaterialFunctionBodyCommand as Edit,
};
use aestra_compiler::{
    MaterialCompiler, MaterialFunctionBodyProjection, MaterialFunctionGraphTarget,
};
use bevy::ui::RelativeCursorPosition;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod arrange_context_tests;
mod clipboard;
mod socket_palette;

#[derive(Component, Clone, Copy)]
struct View(MaterialFunctionId);
#[derive(Component, Clone, Copy)]
struct ViewScope(crate::material_graph::MaterialSelectionScope);
#[derive(Component, Clone, Copy)]
struct FunctionGraphNodeAction {
    owner: MaterialFunctionId,
    expression: MaterialExpressionId,
    scope: crate::material_graph::MaterialSelectionScope,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Input(MaterialExpressionId, MaterialExpressionInput),
    Output(MaterialFunctionOutputId),
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum SocketKind {
    Source(MaterialExpressionId),
    Target(Target),
}

impl SocketKind {
    fn side(self) -> GraphSocketSide {
        match self {
            Self::Source(_) => GraphSocketSide::Output,
            Self::Target(_) => GraphSocketSide::Input,
        }
    }
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
#[derive(Component, Clone, Copy)]
struct Socket {
    owner: MaterialFunctionId,
    kind: SocketKind,
    node: Entity,
}
#[derive(Component, Default)]
struct Anchor(Option<Vec2>);
#[derive(Resource, Default)]
struct ConnectionPreview(
    Option<(Entity, Vec2)>,
    std::collections::BTreeSet<Entity>,
    Option<Entity>,
);
#[derive(Component)]
struct PreviewWire(MaterialFunctionId);
#[derive(Component)]
struct Wire {
    owner: MaterialFunctionId,
    source: MaterialExpressionId,
    target: Target,
}

#[derive(Debug, Clone)]
enum FunctionGraphMenuKind {
    Node(MaterialExpressionId),
    Connections(Vec<(MaterialExpressionId, Target)>),
}

#[derive(Debug, Clone)]
struct FunctionGraphMenuOpen {
    owner: MaterialFunctionId,
    scope: crate::material_graph::MaterialSelectionScope,
    position: Vec2,
    kind: FunctionGraphMenuKind,
}

#[derive(Resource, Default)]
pub(crate) struct FunctionGraphMenuState {
    open: Option<FunctionGraphMenuOpen>,
}

impl FunctionGraphMenuState {
    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

#[derive(Component)]
struct FunctionGraphContextMenu;

#[derive(Component, Clone, Copy)]
enum FunctionGraphContextAction {
    Clipboard(crate::material_graph::clipboard::Shortcut),
    Arrange(crate::material_graph::arrange::ArrangeScope),
    Open(MaterialExpressionId),
    Duplicate(MaterialExpressionId),
    Delete(MaterialExpressionId),
    Disconnect(Target),
}
#[derive(Component, Clone, Copy)]
struct BodyAction {
    owner: MaterialFunctionId,
    scope: Option<crate::docking::EditorViewId>,
    kind: BodyActionKind,
}
#[derive(Clone, Copy)]
enum BodyActionKind {
    Back,
    Locate,
    Arrange(crate::material_graph::arrange::ArrangeScope),
    Create(aestra_compiler::MaterialGraphCreateKind),
    Boolean(MaterialExpressionId, bool),
    Input(MaterialFunctionInputId),
    Remove(MaterialExpressionId),
}
#[derive(Component)]
struct Constant {
    owner: MaterialFunctionId,
    expression: MaterialExpressionId,
    component: usize,
}

pub(super) fn register(app: &mut App) {
    socket_palette::register(app);
    app.init_resource::<ConnectionPreview>()
        .init_resource::<FunctionGraphMenuState>()
        .init_resource::<crate::material_graph::MaterialGraphSelectionState>()
        .init_resource::<crate::material_graph::clipboard::GraphClipboard>()
        // Graph observers registered here (e.g. handle_function_graph_context_action) read the
        // viewport pan/zoom memory; own its presence so every context registering these observers —
        // including focused tests — has it, not just apps that also add the feathers node-graph plugin.
        .init_resource::<crate::feathers::node_graph::GraphViewportMemory>()
        .add_observer(start_connection)
        .add_observer(move_connection)
        .add_observer(end_connection)
        .add_observer(select_function_graph_node)
        .add_observer(open_nested_function_call)
        .add_observer(select_function_graph_canvas)
        .add_observer(select_function_graph_marquee)
        .add_observer(handle_modified_function_node_drag)
        .add_observer(open_function_graph_palette)
        .add_observer(open_function_graph_pin_menu)
        .add_observer(open_function_graph_node_menu)
        .add_observer(handle_function_graph_context_action)
        .add_observer(action)
        .add_observer(drop_socket)
        .add_observer(constant_text)
        .add_observer(constant_number)
        .add_systems(
            Update,
            (
                attach_wires,
                dismiss_function_graph_menu,
                clipboard::keyboard,
            ),
        )
        .add_systems(
            PostUpdate,
            update_wires.after(bevy::ui::UiSystems::PostLayout),
        );
}

fn activate_function_graph_target(
    session: &mut EditorSession,
    catalog: &ProjectEffectCatalog,
    owner: MaterialFunctionId,
) -> bool {
    match session.open_material_function(catalog, owner) {
        Ok(()) => true,
        Err(error) => {
            session.status = format!("Function unavailable: {error}");
            false
        }
    }
}

// Picking may target a child of the socket hit area (for example its visual dot).
// Resolve the semantic endpoint consistently for every phase of the gesture.
fn socket_entity(
    mut entity: Entity,
    sockets: &Query<(Entity, &Socket)>,
    parents: &Query<&ChildOf>,
) -> Option<Entity> {
    loop {
        if sockets.contains(entity) {
            return Some(entity);
        }
        entity = parents.get(entity).ok()?.parent();
    }
}

fn start_connection(
    mut event: On<Pointer<DragStart>>,
    sockets: Query<(Entity, &Socket)>,
    mut preview: ResMut<ConnectionPreview>,
    session: Option<Res<EditorSession>>,
    catalog: Option<Res<ProjectEffectCatalog>>,
    parents: Query<&ChildOf>,
    views: Query<(), With<View>>,
) {
    if event.button == bevy::picking::pointer::PointerButton::Primary
        && let Some(entity) = socket_entity(event.entity, &sockets, &parents)
    {
        preview.0 = Some((entity, event.pointer_location.position));
        preview.1.clear();
        preview.2 = None;
        if let (Some(session), Some(catalog), Ok((_, origin))) =
            (session, catalog, sockets.get(entity))
            && let Ok(document) = session.graph_authoring_document_for(
                &crate::material_document::MaterialEditingTarget::Function {
                    root: catalog.root().to_owned(),
                    id: origin.owner,
                },
                &catalog,
            )
        {
            for (entity, candidate) in &sockets {
                if parents
                    .iter_ancestors(origin.node)
                    .find(|id| views.contains(*id))
                    != parents
                        .iter_ancestors(candidate.node)
                        .find(|id| views.contains(*id))
                {
                    continue;
                }
                if let Some((source, target)) = socket_pair(origin, candidate) {
                    let mut candidate_document = document.clone();
                    let transaction = aestra_authoring::MaterialTransaction::new(
                        "Check connection",
                        vec![
                            aestra_authoring::MaterialCommand::EditMaterialFunctionBody {
                                function: origin.owner,
                                edit: connection_edit(source, target),
                            },
                        ],
                    );
                    if aestra_authoring::MaterialCommandExecutor::execute(
                        &mut candidate_document,
                        &transaction,
                    )
                    .is_ok()
                    {
                        preview.1.insert(entity);
                    }
                }
            }
        }
        event.propagate(false);
    }
}
fn move_connection(
    mut event: On<Pointer<Drag>>,
    mut preview: ResMut<ConnectionPreview>,
    sockets: Query<(Entity, &Socket)>,
    parents: Query<&ChildOf>,
) {
    if let Some((entity, cursor)) = &mut preview.0
        && Some(*entity) == socket_entity(event.entity, &sockets, &parents)
    {
        *cursor = event.pointer_location.position;
        event.propagate(false);
    }
}
fn end_connection(
    mut event: On<Pointer<DragEnd>>,
    mut preview: ResMut<ConnectionPreview>,
    sockets: Query<(Entity, &Socket)>,
    parents: Query<&ChildOf>,
    geometry: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<Socket>>,
    editor: Option<ResMut<FunctionEditor>>,
    session: Option<ResMut<EditorSession>>,
    catalog: Option<ResMut<ProjectEffectCatalog>>,
    mut commands: Commands,
    keys: Option<Res<ButtonInput<KeyCode>>>,
) {
    if let Some((entity, _)) = preview.0
        && Some(entity) == socket_entity(event.entity, &sockets, &parents)
    {
        if keys
            .as_ref()
            .is_some_and(|keys| keys.pressed(KeyCode::Escape))
        {
            *preview = default();
            event.propagate(false);
            return;
        }
        // Re-evaluate the release position; a fast last movement may precede PostUpdate.
        let snap = geometry
            .iter()
            .filter(|(entity, _, _)| preview.1.contains(entity))
            .filter_map(|(entity, computed, transform)| {
                let distance = (transform.translation.trunc() * computed.inverse_scale_factor)
                    .distance(event.pointer_location.position);
                (distance <= 18.0).then_some((entity, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(entity, _)| entity);
        if let (Some(target), Some(mut editor), Some(mut session), Some(mut catalog)) =
            (snap, editor, session, catalog)
            && let (Ok((_, from)), Ok((_, to))) = (sockets.get(entity), sockets.get(target))
            && activate_function_graph_target(&mut session, &catalog, from.owner)
            && let Some((source, target)) = socket_pair(from, to)
        {
            let result = editor.edit_body(
                &mut session,
                &mut catalog,
                vec![connection_edit(source, target)],
            );
            finish(&mut session, result);
        } else if snap.is_none() {
            let pointer = event.pointer_location.position;
            commands.queue(move |world: &mut World| socket_palette::open(world, entity, pointer));
        }
        *preview = ConnectionPreview::default();
        event.propagate(false);
    }
}

fn socket_pair(from: &Socket, to: &Socket) -> Option<(MaterialExpressionId, Target)> {
    if from.owner != to.owner {
        return None;
    }
    match (from.kind, to.kind) {
        (SocketKind::Source(source), SocketKind::Target(target))
        | (SocketKind::Target(target), SocketKind::Source(source)) => Some((source, target)),
        _ => None,
    }
}

fn target(value: &MaterialFunctionGraphTarget) -> Option<Target> {
    match value {
        MaterialFunctionGraphTarget::Output(id) => Some(Target::Output(*id)),
        MaterialFunctionGraphTarget::Argument { expression, input } => Some(Target::Input(
            *expression,
            MaterialExpressionInput::FunctionArgument(*input),
        )),
        MaterialFunctionGraphTarget::Input { expression, port } => {
            let MaterialConnectionTarget::ExpressionInput { expression, input } =
                crate::material_graph::input_target(*expression, port)?
            else {
                return None;
            };
            Some(Target::Input(expression, input))
        }
    }
}

fn connection_edit(source: MaterialExpressionId, target: Target) -> Edit {
    match target {
        Target::Output(output) => Edit::SetOutput { output, source },
        Target::Input(expression, input) => Edit::Rewire {
            expression,
            input,
            source,
        },
    }
}

fn drop_socket(
    mut event: On<Pointer<DragDrop>>,
    sockets: Query<(Entity, &Socket)>,
    parents: Query<&ChildOf>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut preview: ResMut<ConnectionPreview>,
    views: Query<(), With<View>>,
) {
    if event.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    let (Some(source), Some(destination)) = (
        socket_entity(event.dropped, &sockets, &parents),
        socket_entity(event.entity, &sockets, &parents),
    ) else {
        return;
    };
    let (Ok((_, from)), Ok((_, to))) = (sockets.get(source), sockets.get(destination)) else {
        return;
    };
    if from.owner != to.owner || !activate_function_graph_target(&mut session, &catalog, from.owner)
    {
        return;
    }
    if parents
        .iter_ancestors(from.node)
        .find(|id| views.contains(*id))
        != parents
            .iter_ancestors(to.node)
            .find(|id| views.contains(*id))
    {
        return;
    }
    let pair = match (from.kind, to.kind) {
        (SocketKind::Source(source), SocketKind::Target(target))
        | (SocketKind::Target(target), SocketKind::Source(source)) => Some((source, target)),
        _ => None,
    };
    let Some((source, target)) = pair else {
        return;
    };
    event.propagate(false);
    *preview = ConnectionPreview::default();
    let result = editor.edit_body(
        &mut session,
        &mut catalog,
        vec![connection_edit(source, target)],
    );
    finish(&mut session, result);
}

fn create_edits(
    function: &MaterialFunction,
    action: BodyActionKind,
    library: &aestra_compiler::MaterialFunctionLibrary,
) -> Result<Vec<Edit>, String> {
    if let BodyActionKind::Remove(expression) = action {
        return Ok(vec![Edit::Remove { expression }]);
    }
    if let BodyActionKind::Boolean(expression, value) = action {
        return Ok(vec![Edit::Replace {
            expression,
            replacement: MaterialExpression {
                id: expression,
                kind: MaterialExpressionKind::Constant(MaterialValue::Bool(value)),
            },
        }]);
    }
    let expressions = match action {
        BodyActionKind::Create(kind) => MaterialCompiler
            .function_graph_node_expressions(function, kind, library)
            .map_err(|error| error.to_string())?,
        BodyActionKind::Input(id) => vec![MaterialExpression {
            id: MaterialExpressionId::new(),
            kind: MaterialExpressionKind::FunctionInput(id),
        }],
        _ => return Err("Not a creation action".into()),
    };
    Ok(expressions
        .into_iter()
        .enumerate()
        .map(|(index, expression)| Edit::Add {
            expression,
            index: function.expressions.len() + index,
        })
        .collect())
}

pub(crate) fn estimated_size(
    id: MaterialExpressionId,
    nodes: &[MaterialExpression],
    edges: &[aestra_compiler::MaterialFunctionGraphEdge],
) -> Vec2 {
    let rows = nodes.iter().find(|node| node.id == id).map_or(1, |node| match &node.kind {
        MaterialExpressionKind::Constant(value) => components(value).len(),
        _ => edges.iter().filter(|edge| matches!(target(&edge.target), Some(Target::Input(expression, _)) if expression == id)).count(),
    });
    Vec2::new(NODE_WIDTH, 62.0 + rows.max(1) as f32 * 28.0)
}

fn select_function_graph_node(
    mut click: On<Pointer<Click>>,
    actions: Query<&FunctionGraphNodeAction>,
    mut graph_nodes: Query<&mut FeathersGraphNode>,
    parents: Query<&ChildOf>,
    sockets: Query<(), With<Socket>>,
    controls: Query<(), Or<(With<Constant>, With<BodyAction>)>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut selection: ResMut<crate::material_graph::MaterialGraphSelectionState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Primary || keys.pressed(KeyCode::Space) {
        return;
    }
    let mut entity = click.event_target();
    let (action, node_entity) = loop {
        if sockets.contains(entity) || controls.contains(entity) {
            return;
        }
        if let Ok(action) = actions.get(entity) {
            break (*action, entity);
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    if graph_nodes
        .get_mut(node_entity)
        .is_ok_and(|mut node| node.consume_suppressed_release_click())
    {
        click.propagate(false);
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    selection.select_function_expression(
        action.scope,
        action.owner,
        action.expression,
        control,
        shift,
    );
    session.ui_revision += 1;
    click.propagate(false);
}

fn open_nested_function_call(
    mut click: On<Pointer<Click>>,
    actions: Query<&FunctionGraphNodeAction>,
    parents: Query<&ChildOf>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary || click.count < 2 {
        return;
    }
    let mut entity = click.event_target();
    let action = loop {
        if let Ok(action) = actions.get(entity) {
            break *action;
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let Ok(function) = session.graph_function_for(
        &crate::material_document::MaterialEditingTarget::Function {
            root: catalog.root().to_owned(),
            id: action.owner,
        },
        &catalog,
    ) else {
        return;
    };
    if function.id != action.owner {
        return;
    }
    let Some(expression) = function
        .expressions
        .iter()
        .find(|expression| expression.id == action.expression)
    else {
        return;
    };
    let MaterialExpressionKind::FunctionCall {
        function: aestra_core::material::MaterialFunctionRef::Project(function),
        ..
    } = &expression.kind
    else {
        return;
    };
    commands.trigger(crate::asset_browser::OpenFunction {
        function: *function,
        new_view: false,
    });
    click.propagate(false);
}

fn select_function_graph_canvas(
    mut click: On<Pointer<Click>>,
    views: Query<(&View, &ViewScope)>,
    controls: Query<
        (),
        Or<(
            With<FunctionGraphNodeAction>,
            With<Socket>,
            With<Constant>,
            With<BodyAction>,
            With<socket_palette::Surface>,
        )>,
    >,
    parents: Query<&ChildOf>,
    mut selection: ResMut<crate::material_graph::MaterialGraphSelectionState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let mut entity = click.event_target();
    let view = loop {
        if controls.contains(entity) {
            return;
        }
        if let Ok(view) = views.get(entity) {
            break (*view.0, *view.1);
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    if selection.clear_function_selection(view.1.0, view.0.0) {
        session.ui_revision += 1;
    }
    click.propagate(false);
}

fn open_function_graph_palette(
    mut click: On<Pointer<Click>>,
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
                pointer_position_in_node(click.pointer_location.position, computed, transform);
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
    let pointer = click.pointer_location.position;
    commands.queue(move |world: &mut World| socket_palette::open_canvas(world, viewport, pointer));
    click.propagate(false);
}

fn open_function_graph_pin_menu(
    mut click: On<Pointer<Click>>,
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
        let pointer = click.pointer_location.position;
        commands
            .queue(move |world: &mut World| socket_palette::open(world, socket_entity, pointer));
    } else {
        menus.open = Some(FunctionGraphMenuOpen {
            owner: socket.owner,
            scope: scope.0,
            position: pointer_position_in_node(
                click.pointer_location.position,
                computed,
                transform,
            ),
            kind: FunctionGraphMenuKind::Connections(connections),
        });
        session.ui_revision += 1;
    }
    click.propagate(false);
}

fn open_function_graph_node_menu(
    mut click: On<Pointer<Click>>,
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
        position: pointer_position_in_node(click.pointer_location.position, computed, transform),
        kind: FunctionGraphMenuKind::Node(action.expression),
    });
    session.ui_revision += 1;
    click.propagate(false);
}

#[allow(clippy::too_many_arguments)]
fn handle_function_graph_context_action(
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

fn dismiss_function_graph_menu(
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

fn select_function_graph_marquee(
    event: On<GraphMarqueeSelection>,
    views: Query<(&View, &ViewScope)>,
    graph_nodes: Query<(&FunctionGraphNodeAction, &FeathersGraphNode)>,
    mut selection: ResMut<crate::material_graph::MaterialGraphSelectionState>,
    mut session: ResMut<EditorSession>,
) {
    let Ok((function, scope)) = views.get(event.viewport) else {
        return;
    };
    let expressions = graph_nodes
        .iter()
        .filter(|(action, node)| {
            action.owner == function.0 && event.nodes.contains(node.node_key())
        })
        .map(|(action, _)| action.expression)
        .collect::<BTreeSet<_>>();
    if selection.select_function_expressions(scope.0, function.0, &expressions, event.mode) {
        session.ui_revision += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn duplicate_function_selection(
    owner: MaterialFunctionId,
    scope: crate::material_graph::MaterialSelectionScope,
    anchor: MaterialExpressionId,
    offset: Vec2,
    graph_nodes: &Query<(&FunctionGraphNodeAction, &FeathersGraphNode)>,
    editor: &mut FunctionEditor,
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    memory: &mut GraphViewportMemory,
    selection: &mut crate::material_graph::MaterialGraphSelectionState,
) -> Result<usize, String> {
    let function = session.graph_function(catalog)?;
    if function.id != owner {
        return Err("The function editing target changed".into());
    }
    let mut selected = selection
        .function_arrange_seeds(scope, owner)
        .into_iter()
        .filter_map(|key| match key {
            GraphNodeKey::Expression(id) => Some(id),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    if selected.is_empty() {
        selected.insert(anchor);
    }
    let positions = graph_nodes
        .iter()
        .filter(|(node, _)| {
            node.owner == owner && node.scope == scope && selected.contains(&node.expression)
        })
        .map(|(node, graph)| (node.expression, graph.position()))
        .collect::<BTreeMap<_, _>>();
    let graph_key = crate::material_graph::function_graph_memory_key(catalog.root(), owner);
    let positions = selected
        .iter()
        .map(|id| {
            (
                *id,
                positions
                    .get(id)
                    .copied()
                    .or_else(|| memory.node_position(&graph_key, &id.to_string()))
                    .unwrap_or(Vec2::ZERO),
            )
        })
        .collect();
    let fragment = crate::material_graph::clipboard::Fragment::capture(
        &function.expressions,
        &selected,
        positions,
        &[],
        &[],
    )?;
    clipboard::insert(
        &fragment, offset, owner, scope, editor, session, catalog, memory, selection,
    )
}

#[allow(clippy::too_many_arguments)]
fn handle_modified_function_node_drag(
    event: On<GraphModifiedNodeDrag>,
    graph_nodes: Query<(&FunctionGraphNodeAction, &FeathersGraphNode)>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut memory: ResMut<GraphViewportMemory>,
    mut selection: ResMut<crate::material_graph::MaterialGraphSelectionState>,
    mut commands: Commands,
) {
    let Some((action, _)) = graph_nodes
        .iter()
        .find(|(_, node)| node.graph_key() == event.graph && node.node_key() == event.node)
    else {
        return;
    };
    if !activate_function_graph_target(&mut session, &catalog, action.owner) {
        return;
    }
    if !selection.is_function_expression_selected(action.scope, action.owner, action.expression) {
        selection.select_function_expression(
            action.scope,
            action.owner,
            action.expression,
            false,
            false,
        );
    }
    let delta = event.after.0 - event.before.0;
    match event.modifier {
        GraphNodeDragModifier::Duplicate => {
            match duplicate_function_selection(
                action.owner,
                action.scope,
                action.expression,
                delta,
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
        GraphNodeDragModifier::Upstream | GraphNodeDragModifier::Downstream => {
            let Ok(function) = session.graph_function(&catalog) else {
                return;
            };
            if function.id != action.owner {
                return;
            }
            let Ok(library) = catalog.material_function_library() else {
                return;
            };
            let projection = MaterialCompiler.project_function_graph(&function, &library);
            let keys = function_branch_nodes(
                &projection.body,
                GraphNodeKey::Expression(action.expression),
                event.modifier == GraphNodeDragModifier::Upstream,
            );
            let mut before = BTreeMap::new();
            for key in &keys {
                let node = match key {
                    GraphNodeKey::Expression(id) => id.to_string(),
                    GraphNodeKey::FunctionOutputs => "outputs".into(),
                    GraphNodeKey::MaterialOutputs => continue,
                };
                let Some(state) = memory.node(&event.graph, &node) else {
                    continue;
                };
                let original = if node == event.node {
                    event.before
                } else {
                    state
                };
                before.insert(node.clone(), original);
                memory.set_node(&event.graph, node, original.0 + delta, original.1);
            }
            let expressions = keys
                .into_iter()
                .filter_map(|key| match key {
                    GraphNodeKey::Expression(id) => Some(id),
                    _ => None,
                })
                .collect();
            selection.select_function_expressions(
                action.scope,
                action.owner,
                &expressions,
                GraphSelectionMode::Replace,
            );
            commands.trigger(GraphPresentationBatchEdit {
                graph: event.graph.clone(),
                before,
                origin: None,
            });
        }
    }
    session.ui_revision += 1;
}

fn function_branch_nodes(
    projection: &MaterialFunctionBodyProjection,
    seed: GraphNodeKey,
    upstream: bool,
) -> BTreeSet<GraphNodeKey> {
    let MaterialFunctionBodyProjection::Graph { edges, .. } = projection else {
        return BTreeSet::from([seed]);
    };
    let mut result = BTreeSet::from([seed]);
    let mut frontier = vec![seed];
    while let Some(current) = frontier.pop() {
        for edge in edges {
            let Some(target) = target(&edge.target).map(|target| match target {
                Target::Input(expression, _) => GraphNodeKey::Expression(expression),
                Target::Output(_) => GraphNodeKey::FunctionOutputs,
            }) else {
                continue;
            };
            let source = GraphNodeKey::Expression(edge.source);
            let candidate = if upstream && target == current {
                Some(source)
            } else if !upstream && source == current {
                Some(target)
            } else {
                None
            };
            if let Some(candidate) = candidate
                && result.insert(candidate)
            {
                frontier.push(candidate);
            }
        }
    }
    result
}

fn action(
    event: On<Activate>,
    actions: Query<&BodyAction>,
    selection: Res<crate::material_graph::MaterialGraphSelectionState>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut commands: Commands,
    mut memory: Option<ResMut<GraphViewportMemory>>,
    placement: placement::Context,
) {
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    if !activate_function_graph_target(&mut session, &catalog, action.owner) {
        return;
    }
    if matches!(action.kind, BodyActionKind::Back) {
        session.return_to_effect_material();
        return;
    }
    if matches!(action.kind, BodyActionKind::Locate) {
        commands.trigger(crate::asset_browser::LocateInAssets(
            aestra_project::ProjectAssetId::MaterialFunction(action.owner),
        ));
        return;
    }
    if let BodyActionKind::Arrange(arrange_scope) = action.kind {
        commands.trigger(crate::material_graph::arrange::ArrangeGraph {
            view: GraphViewKey {
                document: GraphDocumentKey {
                    project: catalog.root().to_owned(),
                    asset: crate::document::DocumentKey::MaterialFunction(action.owner),
                },
                view: action.scope,
            },
            editing_target: crate::material_document::MaterialEditingTarget::Function {
                root: catalog.root().to_owned(),
                id: action.owner,
            },
            scope: arrange_scope,
            seeds: selection.function_arrange_seeds(action.scope, action.owner),
        });
        return;
    }
    let graph_key = format!("function:{}:{}", catalog.root().display(), action.owner);
    let view_key = GraphViewKey {
        document: GraphDocumentKey {
            project: catalog.root().to_owned(),
            asset: crate::document::DocumentKey::MaterialFunction(action.owner),
        },
        view: action.scope,
    };
    let mut area = memory
        .as_deref()
        .map(|memory| placement.capture(&view_key, memory))
        .unwrap_or_default();
    let preferred = placement
        .center(&view_key)
        .map(|center| center - Vec2::new(NODE_WIDTH * 0.5, NODE_HEADER_HEIGHT * 0.5))
        .unwrap_or(Vec2::new(34.0, 68.0));
    let layout_before = memory.as_deref().and_then(|memory| {
        crate::material_graph::presentation::Snapshot::capture(
            &graph_key, &catalog, &session, memory,
        )
    });
    let mut created = Vec::new();
    let result = session.graph_function(&catalog).and_then(|function| {
        let library = catalog
            .material_function_library()
            .map_err(|error| error.to_string())?;
        let edits = create_edits(&function, action.kind, &library)?;
        created = edits
            .iter()
            .filter_map(|edit| match edit {
                Edit::Add { expression, .. } => Some(expression.id),
                _ => None,
            })
            .collect();
        editor.edit_body(&mut session, &mut catalog, edits)
    });
    if result.is_ok()
        && !created.is_empty()
        && let Some(memory) = memory.as_deref_mut()
        && let Ok(function) = session.graph_function(&catalog)
        && let Ok(library) = catalog.material_function_library()
        && let MaterialFunctionBodyProjection::Graph { nodes, edges } = MaterialCompiler
            .project_function_graph(&function, &library)
            .body
    {
        let mut unassisted = false;
        placement.preserve_existing(&view_key, memory);
        for id in created {
            let placed = area.place(
                preferred,
                estimated_size(id, &nodes, &edges),
                placement::Neighborhood::Cursor,
            );
            memory.place_node(&graph_key, id.to_string(), placed.position);
            unassisted |= !placed.assisted;
        }
        if unassisted {
            session
                .status
                .push_str(&format!(" · {}", placement.notice()));
        }
    }
    if result.is_ok()
        && let Some(before) = layout_before
        && let Some(memory) = memory.as_deref_mut()
    {
        before.attach(&catalog, &mut session, memory);
    }
    finish(&mut session, result);
}

fn replace_constant(
    control: &Constant,
    value: f32,
    editor: &mut FunctionEditor,
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
) {
    if !activate_function_graph_target(session, catalog, control.owner) {
        return;
    }
    let result = session.graph_function(catalog).and_then(|function| {
        let mut replacement = function
            .expressions
            .iter()
            .find(|expression| expression.id == control.expression)
            .cloned()
            .ok_or("Constant is no longer available")?;
        let MaterialExpressionKind::Constant(literal) = &mut replacement.kind else {
            return Err("Expression is no longer a constant".into());
        };
        set_component(literal, control.component, value)?;
        editor.edit_body(
            session,
            catalog,
            vec![Edit::Replace {
                expression: control.expression,
                replacement,
            }],
        )
    });
    finish(session, result);
}

fn components(value: &MaterialValue) -> Vec<(&'static str, f32)> {
    match value {
        MaterialValue::Float(v) => vec![("Value", *v)],
        MaterialValue::Vec2(v) => ["X", "Y"].into_iter().zip(*v).collect(),
        MaterialValue::Vec3(v) => ["X", "Y", "Z"].into_iter().zip(*v).collect(),
        MaterialValue::Vec4(v) => ["X", "Y", "Z", "W"].into_iter().zip(*v).collect(),
        MaterialValue::ColorSrgb(v) => ["R", "G", "B", "A"].into_iter().zip(*v).collect(),
        MaterialValue::Bool(v) => vec![("Boolean (0 or 1)", if *v { 1.0 } else { 0.0 })],
        MaterialValue::Texture2D(_) => Vec::new(),
    }
}

fn set_component(literal: &mut MaterialValue, component: usize, value: f32) -> Result<(), String> {
    if !value.is_finite() {
        return Err("Constant must be finite".into());
    }
    let values: &mut [f32] = match literal {
        MaterialValue::Float(v) => std::slice::from_mut(v),
        MaterialValue::Vec2(v) => v,
        MaterialValue::Vec3(v) => v,
        MaterialValue::Vec4(v) | MaterialValue::ColorSrgb(v) => v,
        MaterialValue::Bool(v) if component == 0 && (value == 0.0 || value == 1.0) => {
            *v = value == 1.0;
            return Ok(());
        }
        _ => return Err("Unsupported constant component".into()),
    };
    *values
        .get_mut(component)
        .ok_or("Missing constant component")? = value;
    Ok(())
}
fn constant_text(
    change: On<ValueChange<String>>,
    controls: Query<&Constant>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    match change.value.trim().parse::<f32>() {
        Ok(value) => replace_constant(control, value, &mut editor, &mut session, &mut catalog),
        Err(_) => finish(&mut session, Err("Enter a numeric constant".into())),
    }
}
fn constant_number(
    change: On<ValueChange<f32>>,
    controls: Query<&Constant>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    replace_constant(
        control,
        change.value,
        &mut editor,
        &mut session,
        &mut catalog,
    );
}

fn title(expression: &MaterialExpression, function: &MaterialFunction) -> String {
    if let MaterialExpressionKind::FunctionInput(id) = expression.kind {
        return function
            .inputs
            .iter()
            .find(|port| port.id == id)
            .map_or_else(|| "Missing input".into(), |port| port.name.clone());
    }
    format!("{:?}", expression.kind)
        .split(['(', '{'])
        .next()
        .unwrap()
        .trim()
        .into()
}

/// Shared with non-pointer semantic creation so hidden views retain the same bootstrap bases.
pub(crate) fn bootstrap_layout(
    nodes: &[MaterialExpression],
    edges: &[aestra_compiler::MaterialFunctionGraphEdge],
) -> (BTreeMap<MaterialExpressionId, Vec2>, Vec2, Vec2) {
    let mut inputs = nodes
        .iter()
        .map(|node| (node.id, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in edges {
        if let Some(Target::Input(expression, _)) = target(&edge.target) {
            inputs.entry(expression).or_default().push(edge.source);
        }
    }
    let mut depths = BTreeMap::new();
    for node in nodes {
        crate::material_graph::expression_depth(
            node.id,
            &inputs,
            &mut depths,
            &mut Default::default(),
        );
    }
    let mut columns = BTreeMap::<usize, f32>::new();
    let positions = nodes
        .iter()
        .map(|expression| {
            let depth = depths[&expression.id];
            let y = columns.entry(depth).or_insert(68.0);
            let initial = Vec2::new(34.0 + depth as f32 * 282.0, *y);
            let rows = match &expression.kind {
                MaterialExpressionKind::Constant(value) => components(value).len(),
                _ => inputs[&expression.id].len(),
            };
            *y += 62.0 + rows.max(1) as f32 * PORT_ROW_HEIGHT.max(28.0);
            (expression.id, initial)
        })
        .collect();
    let output = Vec2::new(
        34.0 + (depths.values().copied().max().unwrap_or_default() + 1) as f32 * 282.0,
        122.0,
    );
    let extent = Vec2::new(
        (output.x + NODE_WIDTH + 34.0).max(720.0),
        (columns.values().copied().reduce(f32::max).unwrap_or(0.0) + 34.0).max(420.0),
    );
    (positions, output, extent)
}

pub(crate) fn spawn(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    assets: &AssetServer,
    memory: &GraphViewportMemory,
    selection: &crate::material_graph::MaterialGraphSelectionState,
    menus: &FunctionGraphMenuState,
    editing_target: &crate::material_document::MaterialEditingTarget,
    view: Option<crate::docking::EditorViewId>,
) {
    let Ok(function) = session.graph_function_for(editing_target, catalog) else {
        return;
    };
    if function.custom_wesl.is_some() {
        return;
    }
    let Ok(library) = catalog.material_function_library() else {
        return;
    };
    let projection = MaterialCompiler.project_function_graph(&function, &library);
    let MaterialFunctionBodyProjection::Graph { nodes, edges } = projection.body else {
        return;
    };
    let graph_key = crate::material_graph::function_graph_memory_key(catalog.root(), function.id);
    // Placement stays document-scoped, but a toolbar frame action must target only its view.
    let viewport_key = view.map_or_else(
        || format!("{graph_key}#tool"),
        |view| format!("{graph_key}#view:{}", view.0),
    );
    let (mut positions, output_position, extent) = bootstrap_layout(&nodes, &edges);
    for (id, position) in &mut positions {
        *position = memory
            .node_position(&graph_key, &id.to_string())
            .unwrap_or(*position);
    }
    parent
        .spawn(graph_toolbar_bundle())
        .with_children(|toolbar| {
            spawn_graph_tool_button(
                toolbar,
                assets,
                "icons/chevron-left.svg",
                "Return to effect".into(),
                BodyAction {
                    owner: function.id,
                    kind: BodyActionKind::Back,
                    scope: view,
                },
            );
            let descriptors = MaterialCompiler.function_graph_node_catalog(&function, &library);
            let mut categories = descriptors
                .iter()
                .map(|descriptor| descriptor.category.clone())
                .collect::<Vec<_>>();
            categories.extend(function.inputs.iter().map(|_| "Function inputs".to_owned()));
            let mut options = descriptors
                .into_iter()
                .map(|descriptor| ComboOption {
                    label: descriptor.label,
                    selected: false,
                    action: BodyAction {
                        owner: function.id,
                        kind: BodyActionKind::Create(descriptor.kind),
                        scope: view,
                    },
                })
                .collect::<Vec<_>>();
            options.extend(function.inputs.iter().map(|input| ComboOption {
                label: format!("Input: {}", input.name),
                selected: false,
                action: BodyAction {
                    owner: function.id,
                    kind: BodyActionKind::Input(input.id),
                    scope: view,
                },
            }));
            spawn_searchable_icon_action_menu(
                toolbar,
                assets,
                "icons/plus.svg",
                "Add node",
                "Add a function node",
                &options,
                &categories,
            );
            spawn_graph_frame_button(
                toolbar,
                assets,
                "icons/frame-all.svg",
                "Frame all".into(),
                GraphFrameAction::new(&viewport_key, GraphFrameTarget::All),
            );
            spawn_graph_frame_button(
                toolbar,
                assets,
                "icons/frame-selection.svg",
                "Frame selection".into(),
                GraphFrameAction::new(&viewport_key, GraphFrameTarget::Selection),
            );
            spawn_graph_tool_button(
                toolbar,
                assets,
                "icons/graph-align.svg",
                "Arrange graph".into(),
                BodyAction {
                    owner: function.id,
                    kind: BodyActionKind::Arrange(
                        crate::material_graph::arrange::ArrangeScope::Graph,
                    ),
                    scope: view,
                },
            );
            spawn_graph_tool_button(
                toolbar,
                assets,
                "icons/folder.svg",
                "Locate in Assets".into(),
                BodyAction {
                    owner: function.id,
                    kind: BodyActionKind::Locate,
                    scope: view,
                },
            );
            spawn_graph_drag_controls(toolbar, assets);
            spawn_graph_toolbar_summary(
                toolbar,
                format!(
                    "FUNCTION · {}  ·  {} NODES  ·  {} LINKS",
                    function.name,
                    nodes.len(),
                    edges.len()
                ),
            );
        });
    let viewport = spawn_graph_viewport(
        parent,
        GraphViewportProps {
            key: viewport_key.clone(),
            content_size: extent,
            selection_bounds: None,
            initial_view: memory
                .view(&viewport_key)
                .or_else(|| memory.view(&graph_key)),
        },
        (),
        |wires| {
            wires.spawn((
                PreviewWire(function.id),
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            for edge in &edges {
                if let Some(target) = target(&edge.target) {
                    wires.spawn((
                        Wire {
                            owner: function.id,
                            source: edge.source,
                            target,
                        },
                        crate::material_graph::insertion::function_wire(edge.source, &edge.target)
                            .unwrap(),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                }
            }
        },
        |canvas| {
            for expression in &nodes {
                let props = props(
                    &graph_key,
                    &expression.id.to_string(),
                    title(expression, &function),
                    positions[&expression.id],
                    selection.is_function_expression_selected(view, function.id, expression.id),
                    memory.is_pinned(&graph_key, &expression.id.to_string()),
                    assets,
                );
                let geometry = GraphGeometryNode::new(
                    GraphNodeKey::Expression(expression.id),
                    expression,
                    false,
                );
                let graph_node = spawn_graph_node(canvas, props, geometry, |node, body| {
                    socket(
                        body,
                        function.id,
                        node,
                        SocketKind::Source(expression.id),
                        None,
                    );
                    let call = if let MaterialExpressionKind::FunctionCall { function, .. } =
                        expression.kind
                    {
                        library.get(function)
                    } else {
                        None
                    };
                    if let Some(call) = call {
                        for input in &call.inputs {
                            socket(
                                body,
                                function.id,
                                node,
                                SocketKind::Target(Target::Input(
                                    expression.id,
                                    MaterialExpressionInput::FunctionArgument(input.id),
                                )),
                                Some(input.name.clone()),
                            );
                        }
                    }
                    for edge in &edges {
                        if call.is_some() {
                            continue;
                        }
                        let belongs = match &edge.target {
                            MaterialFunctionGraphTarget::Input { expression: id, .. }
                            | MaterialFunctionGraphTarget::Argument { expression: id, .. } => {
                                *id == expression.id
                            }
                            _ => false,
                        };
                        if !belongs {
                            continue;
                        }
                        if let Some(target) = target(&edge.target) {
                            let name = match &edge.target {
                                MaterialFunctionGraphTarget::Input { port, .. } => {
                                    crate::material_graph::input_port_presentation(port).label
                                }
                                MaterialFunctionGraphTarget::Argument { input, .. } => {
                                    format!("Argument {input}")
                                }
                                _ => unreachable!(),
                            };
                            socket(
                                body,
                                function.id,
                                node,
                                SocketKind::Target(target),
                                Some(name),
                            );
                        }
                    }
                    if let MaterialExpressionKind::Constant(MaterialValue::Bool(value)) =
                        &expression.kind
                    {
                        spawn_combo_control(
                            body,
                            if *value { "True" } else { "False" },
                            "Boolean value",
                            &[false, true].map(|candidate| ComboOption {
                                label: if candidate { "True" } else { "False" }.into(),
                                selected: candidate == *value,
                                action: BodyAction {
                                    owner: function.id,
                                    kind: BodyActionKind::Boolean(expression.id, candidate),
                                    scope: view,
                                },
                            }),
                            170.0,
                        );
                    } else if let MaterialExpressionKind::Constant(literal) = &expression.kind {
                        for (component, (label, value)) in
                            components(literal).into_iter().enumerate()
                        {
                            body.spawn(Node {
                                width: Val::Px(170.0),
                                min_height: Val::Px(28.0),
                                ..default()
                            })
                            .with_children(|row| {
                                row.spawn((
                                    Text::new(label),
                                    TextFont {
                                        font_size: FontSize::Px(10.0),
                                        ..default()
                                    },
                                    Pickable::IGNORE,
                                ));
                                let control = spawn_text_input(
                                    row,
                                    &value.to_string(),
                                    "Constant value",
                                    Constant {
                                        owner: function.id,
                                        expression: expression.id,
                                        component,
                                    },
                                );
                                row.commands().entity(control).insert(
                                    crate::feathers::number_input::ScrubbableNumber::new(
                                        value,
                                        -f32::MAX,
                                        f32::MAX,
                                        0.01,
                                    ),
                                );
                            });
                        }
                    } else if expression.kind.dependencies().is_empty() {
                        body.spawn((
                            Node {
                                min_height: Val::Px(26.0),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                    }
                    body.commands().entity(node).with_children(|node| {
                        node.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                right: Val::Px(28.0),
                                top: Val::Px(2.0),
                                width: Val::Px(28.0),
                                height: Val::Px(28.0),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ))
                        .with_children(|header| {
                            spawn_compact_action_menu(
                                header,
                                "Node actions",
                                &[ComboOption {
                                    label: "Delete node".into(),
                                    selected: false,
                                    action: BodyAction {
                                        owner: function.id,
                                        kind: BodyActionKind::Remove(expression.id),
                                        scope: view,
                                    },
                                }],
                            )
                        });
                    });
                });
                canvas.commands().entity(graph_node).insert((
                    FunctionGraphNodeAction {
                        owner: function.id,
                        expression: expression.id,
                        scope: view,
                    },
                    GraphModifiedDragTarget,
                ));
            }
            spawn_graph_node(
                canvas,
                props(
                    &graph_key,
                    "outputs",
                    "Function outputs".into(),
                    output_position,
                    false,
                    memory.is_pinned(&graph_key, "outputs"),
                    assets,
                ),
                GraphGeometryNode::new(GraphNodeKey::FunctionOutputs, &function.outputs, false),
                |node, body| {
                    for output in &function.outputs {
                        socket(
                            body,
                            function.id,
                            node,
                            SocketKind::Target(Target::Output(output.id)),
                            Some(output.name.clone()),
                        );
                    }
                },
            );
        },
    );
    parent.commands().entity(viewport).insert((
        GraphGeometryView {
            key: GraphViewKey {
                document: GraphDocumentKey {
                    project: catalog.root().to_owned(),
                    asset: crate::document::DocumentKey::MaterialFunction(function.id),
                },
                view,
            },
            nodes: nodes
                .iter()
                .map(|expression| GraphNodeKey::Expression(expression.id))
                .chain([GraphNodeKey::FunctionOutputs])
                .collect(),
        },
        View(function.id),
        ViewScope(view),
        crate::material_graph::asset_drop::GraphDropTarget::function_for(
            session,
            function.id,
            editing_target,
        ),
    ));
    if let Some(open) = menus
        .open
        .as_ref()
        .filter(|open| open.owner == function.id && open.scope == view)
    {
        parent
            .commands()
            .entity(viewport)
            .with_children(|viewport| {
                spawn_function_graph_context_menu(viewport, open, &function);
            });
    }
}

fn spawn_function_graph_context_menu(
    parent: &mut ChildSpawnerCommands,
    open: &FunctionGraphMenuOpen,
    function: &MaterialFunction,
) {
    spawn_pointer_context_menu_sized(
        parent,
        open.position,
        216.0,
        (),
        (FunctionGraphContextMenu, FeathersGraphNavigationBlocker),
        |menu| match &open.kind {
            FunctionGraphMenuKind::Node(expression) => {
                for (label, shortcut, action) in [
                    (
                        "Copy",
                        "Ctrl+C",
                        crate::material_graph::clipboard::Shortcut::Copy,
                    ),
                    (
                        "Cut",
                        "Ctrl+X",
                        crate::material_graph::clipboard::Shortcut::Cut,
                    ),
                    (
                        "Paste",
                        "Ctrl+V",
                        crate::material_graph::clipboard::Shortcut::Paste,
                    ),
                ] {
                    spawn_pointer_context_menu_shortcut_item(
                        menu,
                        label,
                        shortcut,
                        FunctionGraphContextAction::Clipboard(action),
                    );
                }
                if function.expressions.iter().any(|candidate| {
                    candidate.id == *expression
                        && matches!(
                            candidate.kind,
                            MaterialExpressionKind::FunctionCall {
                                function: aestra_core::material::MaterialFunctionRef::Project(_),
                                ..
                            }
                        )
                }) {
                    spawn_pointer_context_menu_item(
                        menu,
                        "Open function",
                        FunctionGraphContextAction::Open(*expression),
                    );
                }
                spawn_pointer_context_menu_item(
                    menu,
                    "Duplicate node(s)",
                    FunctionGraphContextAction::Duplicate(*expression),
                );
                spawn_pointer_context_menu_item(
                    menu,
                    "Delete node(s)",
                    FunctionGraphContextAction::Delete(*expression),
                );
                spawn_pointer_context_submenu(menu, "Arrange", |menu| {
                    for (label, scope) in [
                        (
                            "Arrange selection",
                            crate::material_graph::arrange::ArrangeScope::Selection,
                        ),
                        (
                            "Arrange upstream",
                            crate::material_graph::arrange::ArrangeScope::Upstream,
                        ),
                        (
                            "Arrange downstream",
                            crate::material_graph::arrange::ArrangeScope::Downstream,
                        ),
                    ] {
                        spawn_pointer_context_menu_item(
                            menu,
                            label,
                            FunctionGraphContextAction::Arrange(scope),
                        );
                    }
                });
            }
            FunctionGraphMenuKind::Connections(connections) => {
                for (index, (_, target)) in connections.iter().copied().enumerate() {
                    let optional = matches!(
                        target,
                        Target::Input(_, MaterialExpressionInput::FunctionArgument(_))
                    );
                    let label = if optional {
                        if connections.len() == 1 {
                            "Break connection".to_owned()
                        } else {
                            format!("Break connection {}", index + 1)
                        }
                    } else if connections.len() == 1 {
                        "Required connection".to_owned()
                    } else {
                        format!("Required connection {}", index + 1)
                    };
                    spawn_pointer_context_menu_item(
                        menu,
                        &label,
                        FunctionGraphContextAction::Disconnect(target),
                    );
                }
            }
        },
    );
}

fn props(
    graph: &str,
    node: &str,
    title: String,
    position: Vec2,
    selected: bool,
    pinned: bool,
    assets: &AssetServer,
) -> GraphNodeProps {
    GraphNodeProps {
        graph_key: graph.into(),
        node_key: node.into(),
        title,
        position,
        selected,
        pinned,
        muted: false,
        collapse_icon: load_svg_icon(assets, "icons/chevron-down.svg"),
        expand_icon: load_svg_icon(assets, "icons/chevron-right.svg"),
        pin_icon: load_svg_icon(assets, "icons/pin.svg"),
        collapse_label: "Collapse".into(),
        expand_label: "Expand".into(),
    }
}
fn socket(
    body: &mut ChildSpawnerCommands,
    owner: MaterialFunctionId,
    node: Entity,
    kind: SocketKind,
    label: Option<String>,
) {
    let side = if matches!(kind, SocketKind::Source(_)) {
        GraphSocketSide::Output
    } else {
        GraphSocketSide::Input
    };
    spawn_graph_port(
        body,
        GraphPortProps {
            tooltip_title: label.clone().unwrap_or_else(|| "Output".into()),
            tooltip_description:
                "Drag to another socket. The compiler validates the resulting function and callers."
                    .into(),
            label,
            side,
            color: Color::srgb(0.4, 0.8, 1.0),
        },
        (
            Socket { owner, kind, node },
            Anchor::default(),
            match kind {
                SocketKind::Source(_) => GraphGeometryPort::Output,
                SocketKind::Target(Target::Input(_, input)) => GraphGeometryPort::Input(input),
                SocketKind::Target(Target::Output(output)) => {
                    GraphGeometryPort::FunctionOutput(output)
                }
            },
        ),
    );
}
fn attach_wires(
    wires: Query<
        Entity,
        (
            Or<(With<Wire>, With<PreviewWire>)>,
            Without<MaterialNode<GraphWireMaterial>>,
        ),
    >,
    materials: Option<ResMut<Assets<GraphWireMaterial>>>,
    mut commands: Commands,
) {
    let Some(mut materials) = materials else {
        return;
    };
    for entity in &wires {
        commands
            .entity(entity)
            .insert(MaterialNode(materials.add(GraphWireMaterial::default())));
    }
}
fn update_wires(
    materials: Option<ResMut<Assets<GraphWireMaterial>>>,
    wires: Query<(Entity, &Wire, &MaterialNode<GraphWireMaterial>)>,
    mut sockets: Query<(&Socket, &UiGlobalTransform, &mut Anchor)>,
    nodes: Query<(&FeathersGraphNode, &ComputedNode, &UiGlobalTransform)>,
    viewports: Query<(&View, &FeathersGraphViewport, &ComputedNode)>,
    previews: Query<(Entity, &PreviewWire, &MaterialNode<GraphWireMaterial>)>,
    mut preview: ResMut<ConnectionPreview>,
    preview_sockets: Query<(Entity, &Socket, &UiGlobalTransform)>,
    viewport_geometry: Query<(&View, &ComputedNode, &UiGlobalTransform)>,
    mut feedback: Query<(Entity, &mut BackgroundColor), With<Socket>>,
    insertion: Option<Res<insertion::State>>,
    parents: Query<&ChildOf>,
) {
    let Some(mut materials) = materials else {
        return;
    };
    let mut positions = Vec::new();
    for (socket, transform, mut anchor) in &mut sockets {
        let Ok((node, computed, node_transform)) = nodes.get(socket.node) else {
            continue;
        };
        if computed.size().min_element() <= 0.0 {
            continue;
        }
        let (_, _, world) = transform.to_scale_angle_translation();
        let offset = crate::feathers::node_graph::compact_graph_socket_offset(
            node,
            computed,
            socket.kind.side(),
        )
        .unwrap_or_else(|| {
            *anchor.0.get_or_insert_with(|| {
                crate::material_graph::viewport_local_position(computed, node_transform, world)
            })
        });
        positions.push((socket, node.position() + offset));
    }
    for (entity, wire, handle) in &wires {
        let Some((viewport_entity, (_, viewport, computed))) = parents
            .iter_ancestors(entity)
            .find_map(|id| viewports.get(id).ok().map(|view| (id, view)))
        else {
            continue;
        };
        let source = positions.iter().find(|(socket, _)| {
            socket.owner == wire.owner
                && parents
                    .iter_ancestors(socket.node)
                    .any(|id| id == viewport_entity)
                && matches!(socket.kind, SocketKind::Source(id) if id == wire.source)
        });
        let target = positions.iter().find(|(socket, _)| {
            socket.owner == wire.owner
                && parents
                    .iter_ancestors(socket.node)
                    .any(|id| id == viewport_entity)
                && matches!(socket.kind, SocketKind::Target(target) if target == wire.target)
        });
        let (Some((_, source)), Some((_, target))) = (source, target) else {
            continue;
        };
        crate::material_graph::update_wire_material(
            &mut materials,
            &handle.0,
            viewport.project_graph_point(*source),
            viewport.project_graph_point(*target),
            insertion
                .as_ref()
                .and_then(|state| state.color(entity))
                .unwrap_or(Vec4::new(0.4, 0.8, 1.0, 1.0)),
            if insertion
                .as_ref()
                .and_then(|state| state.color(entity))
                .is_some()
            {
                4.0
            } else {
                2.0
            },
            computed.inverse_scale_factor,
        );
    }
    if preview
        .0
        .is_some_and(|(entity, _)| !preview_sockets.contains(entity))
    {
        *preview = ConnectionPreview::default();
    }
    preview.2 = preview.0.and_then(|(_, cursor)| {
        preview_sockets
            .iter()
            .filter(|(entity, _, _)| preview.1.contains(entity))
            .filter_map(|(entity, socket, transform)| {
                let (_, computed, _) = parents
                    .iter_ancestors(socket.node)
                    .find_map(|id| viewport_geometry.get(id).ok())?;
                let position = transform.translation.trunc() * computed.inverse_scale_factor;
                let distance = position.distance(cursor);
                (distance <= 18.0).then_some((entity, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(entity, _)| entity)
    });
    for (entity, mut color) in &mut feedback {
        let next = if preview.0.is_some() && preview.1.contains(&entity) {
            Color::srgba(
                0.25,
                0.9,
                0.45,
                if preview.2 == Some(entity) { 0.65 } else { 0.2 },
            )
        } else {
            Color::NONE
        };
        if color.0 != next {
            color.0 = next;
        }
    }
    for (ghost_entity, ghost, handle) in &previews {
        let endpoints = preview.0.and_then(|(entity, cursor)| {
            let (_, socket, transform) = preview_sockets.get(entity).ok()?;
            if socket.owner != ghost.0 {
                return None;
            }
            let owner = parents
                .iter_ancestors(socket.node)
                .find(|id| viewport_geometry.contains(*id))?;
            if !parents.iter_ancestors(ghost_entity).any(|id| id == owner) {
                return None;
            }
            let (_, computed, viewport_transform) = viewport_geometry.get(owner).ok()?;
            let (_, _, origin) = transform.to_scale_angle_translation();
            let start = crate::feathers::context_menu::pointer_position_in_node(
                origin,
                computed,
                viewport_transform,
            );
            let end = crate::feathers::context_menu::pointer_position_in_node(
                preview
                    .2
                    .and_then(|target| preview_sockets.get(target).ok())
                    .map_or(
                        cursor / computed.inverse_scale_factor,
                        |(_, _, transform)| transform.translation.trunc(),
                    ),
                computed,
                viewport_transform,
            );
            let start = start * computed.inverse_scale_factor;
            let end = end * computed.inverse_scale_factor;
            Some(if matches!(socket.kind, SocketKind::Source(_)) {
                (start, end, computed.inverse_scale_factor)
            } else {
                (end, start, computed.inverse_scale_factor)
            })
        });
        let (start, end, inverse_scale) = endpoints.unwrap_or((Vec2::ZERO, Vec2::ZERO, 1.0));
        crate::material_graph::update_wire_material(
            &mut materials,
            &handle.0,
            start,
            end,
            Vec4::new(0.4, 0.8, 1.0, if endpoints.is_some() { 1.0 } else { 0.0 }),
            2.0,
            inverse_scale,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn function_selection_is_scoped_and_produces_partial_arrange_seeds() {
        let function = MaterialFunctionId::new();
        let other_function = MaterialFunctionId::new();
        let first = MaterialExpressionId::new();
        let second = MaterialExpressionId::new();
        let left = Some(crate::docking::EditorViewId(1));
        let right = Some(crate::docking::EditorViewId(2));
        let mut selection = crate::material_graph::MaterialGraphSelectionState::default();

        selection.select_function_expression(left, function, first, false, false);
        selection.select_function_expression(left, function, second, false, true);
        selection.select_function_expression(right, other_function, first, false, false);

        assert!(selection.is_function_expression_selected(left, function, first));
        assert!(selection.is_function_expression_selected(left, function, second));
        assert!(!selection.is_function_expression_selected(right, function, first));
        assert_eq!(
            selection.function_arrange_seeds(left, function),
            BTreeSet::from([
                GraphNodeKey::Expression(first),
                GraphNodeKey::Expression(second),
            ])
        );
        assert!(selection.function_arrange_seeds(right, function).is_empty());
        assert!(selection.clear_function_selection(left, function));
        assert!(selection.function_arrange_seeds(left, function).is_empty());
        assert!(!selection.clear_function_selection(left, function));
    }

    #[test]
    fn modifier_drag_branch_closures_follow_function_edge_direction() {
        let function = MaterialFunction::from_ron(include_str!(
            "../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
        ))
        .unwrap();
        let library = aestra_compiler::MaterialFunctionLibrary::new(vec![function.clone()]);
        let projection = MaterialCompiler.project_function_graph(&function, &library);
        let MaterialFunctionBodyProjection::Graph { edges, .. } = &projection.body else {
            panic!("fixture should have a graph body");
        };
        let edge = edges
            .iter()
            .find_map(|edge| target(&edge.target).map(|target| (edge.source, target)))
            .expect("fixture graph should contain a directed edge");
        let source = GraphNodeKey::Expression(edge.0);
        let target = match edge.1 {
            Target::Input(expression, _) => GraphNodeKey::Expression(expression),
            Target::Output(_) => GraphNodeKey::FunctionOutputs,
        };

        assert!(function_branch_nodes(&projection.body, target, true).contains(&source));
        assert!(function_branch_nodes(&projection.body, source, false).contains(&target));
    }

    #[test]
    fn function_graph_rebuild_uses_expression_ids_for_manual_placement_and_output_ids_for_sockets()
    {
        use bevy::ecs::system::RunSystemOnce;
        let root = tempfile::tempdir().unwrap();
        let function = MaterialFunction::from_ron(include_str!(
            "../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
        ))
        .unwrap();
        function
            .save_ron(root.path().join("body.aestra.material-function.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        let effect_before = session.effect.clone();
        let function_before = session.graph_function(&catalog).unwrap();
        let editing_target = session.material_target.clone();
        let graph_key = format!("function:{}:{}", catalog.root().display(), function.id);
        let mut memory = GraphViewportMemory::default();
        let positions = function
            .expressions
            .iter()
            .enumerate()
            .map(|(index, expression)| {
                let position = Vec2::new(-400.0 + index as f32 * 17.0, 700.0 + index as f32 * 23.0);
                memory.set_node(&graph_key, expression.id.to_string(), position, false);
                (expression.id, position)
            })
            .collect::<BTreeMap<_, _>>();
        let (mut app, ui_root) = geometry::tests::layout_app(1.25);
        geometry::tests::enable_overlays(&mut app);
        app.insert_resource(session)
            .insert_resource(catalog)
            .insert_resource(memory);

        for index in 0..2 {
            // The explicitly rendered function need not be the session's current target.
            app.world_mut()
                .resource_mut::<EditorSession>()
                .material_target = crate::material_document::MaterialEditingTarget::EffectInstance;
            let target = editing_target.clone();
            let host = app
                .world_mut()
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    ChildOf(ui_root),
                ))
                .id();
            app.world_mut()
                .run_system_once(
                    move |mut commands: Commands,
                          session: Res<EditorSession>,
                          catalog: Res<ProjectEffectCatalog>,
                          assets: Res<AssetServer>,
                          memory: Res<GraphViewportMemory>| {
                        commands.entity(host).with_children(|parent| {
                            spawn(
                                parent,
                                &session,
                                &catalog,
                                &assets,
                                &memory,
                                &crate::material_graph::MaterialGraphSelectionState::default(),
                                &FunctionGraphMenuState::default(),
                                &target,
                                Some(crate::docking::EditorViewId(index)),
                            )
                        });
                    },
                )
                .unwrap();
            for _ in 0..4 {
                app.update();
            }
            geometry::tests::assert_frame_all(app.world_mut());
            let world = app.world_mut();
            let document = GraphDocumentKey {
                project: world.resource::<ProjectEffectCatalog>().root().to_owned(),
                asset: crate::document::DocumentKey::MaterialFunction(function.id),
            };
            let measured = world
                .resource::<geometry::GraphGeometryRegistry>()
                .snapshot(&document)
                .unwrap_or_else(|| {
                    panic!(
                        "Missing function geometry on rebuild {index}: {:#?}",
                        world.resource::<geometry::GraphGeometryRegistry>()
                    )
                });
            assert_eq!(
                measured.measured_in.view,
                Some(crate::docking::EditorViewId(index))
            );
            assert_eq!(measured.nodes.len(), positions.len() + 1);
            assert_eq!(
                measured.nodes[&GraphNodeKey::FunctionOutputs].ports.len(),
                function.outputs.len()
            );
            let mut sources = BTreeMap::new();
            let mut outputs = BTreeSet::new();
            for socket in world.query::<&Socket>().iter(world) {
                assert_eq!(socket.owner, function.id);
                match socket.kind {
                    SocketKind::Source(expression) => {
                        let node = world.get::<FeathersGraphNode>(socket.node).unwrap();
                        sources.insert(expression, node.position());
                    }
                    SocketKind::Target(Target::Output(output)) => {
                        outputs.insert(output);
                    }
                    _ => {}
                }
            }
            assert_eq!(sources, positions);
            assert_eq!(
                outputs,
                function.outputs.iter().map(|output| output.id).collect()
            );
            assert_eq!(
                world.query::<&FeathersGraphNode>().iter(world).count(),
                positions.len() + 1
            );
            assert_eq!(world.resource::<EditorSession>().effect, effect_before);
            assert_eq!(
                world
                    .resource::<EditorSession>()
                    .graph_function_for(&editing_target, world.resource::<ProjectEffectCatalog>())
                    .unwrap(),
                function_before
            );
            world.despawn(host);
        }
    }

    #[test]
    fn connection_preview_starts_at_pointer_and_clears_on_release() {
        use bevy::picking::{
            backend::HitData,
            pointer::{Location, PointerButton, PointerId},
        };
        let mut app = App::new();
        app.init_resource::<ConnectionPreview>()
            .add_observer(start_connection)
            .add_observer(move_connection)
            .add_observer(end_connection);
        let socket = app
            .world_mut()
            .spawn(Socket {
                owner: MaterialFunctionId::new(),
                kind: SocketKind::Source(MaterialExpressionId::new()),
                node: Entity::PLACEHOLDER,
            })
            .id();
        let location = Location {
            target: bevy::camera::NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::new(100.0, 120.0),
        };
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location.clone(),
            DragStart {
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            socket,
        ));
        assert_eq!(
            app.world().resource::<ConnectionPreview>().0,
            Some((socket, location.position))
        );
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location,
            DragEnd {
                button: PointerButton::Primary,
                distance: Vec2::ZERO,
            },
            socket,
        ));
        assert!(app.world().resource::<ConnectionPreview>().0.is_none());
    }

    #[test]
    fn component_edits_preserve_literal_type_and_other_channels() {
        for mut value in [
            MaterialValue::Vec2([1.0, 2.0]),
            MaterialValue::Vec3([1.0, 2.0, 3.0]),
            MaterialValue::Vec4([1.0, 2.0, 3.0, 4.0]),
            MaterialValue::ColorSrgb([1.0, 2.0, 3.0, 4.0]),
        ] {
            let before = value.clone();
            set_component(&mut value, 1, 0.75).unwrap();
            assert_eq!(
                std::mem::discriminant(&value),
                std::mem::discriminant(&before)
            );
            let channels = components(&value);
            assert_eq!(channels[0].1, 1.0);
            assert_eq!(channels[1].1, 0.75);
            assert_eq!(&channels[2..], &components(&before)[2..]);
            let saved = value.clone();
            assert!(set_component(&mut value, 0, f32::NAN).is_err());
            assert!(set_component(&mut value, 8, 1.0).is_err());
            assert_eq!(value, saved);
        }
    }

    #[test]
    fn feather_add_and_socket_drop_edit_the_function_not_the_effect() {
        use bevy::{
            camera::NormalizedRenderTarget,
            picking::{
                backend::HitData,
                pointer::{Location, PointerButton, PointerId},
            },
        };
        let root = tempfile::tempdir().unwrap();
        let first = MaterialExpressionId::new();
        let second = MaterialExpressionId::new();
        let output = MaterialFunctionOutputId::new();
        let function = MaterialFunction {
            id: MaterialFunctionId::new(),
            name: "Body".into(),
            schema_version: aestra_core::material::MaterialSchemaVersion::CURRENT,
            inputs: vec![],
            outputs: vec![MaterialFunctionOutput {
                id: output,
                name: "Value".into(),
                value_type: MaterialValueType::Float,
                expression: first,
            }],
            expressions: vec![
                MaterialExpression {
                    id: first,
                    kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
                },
                MaterialExpression {
                    id: second,
                    kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
                },
            ],
            custom_wesl: None,
        };
        function
            .save_ron(root.path().join("body.aestra.material-function.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        let effect = session.effect.clone();
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        let original_function = session.graph_function(&catalog).unwrap();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<GraphViewportMemory>()
            .init_resource::<crate::history::MaterialProgramEditHistory>()
            .init_resource::<crate::history::EditorHistoryLedger>()
            .add_observer(crate::history::execute_history_action);
        super::super::register(&mut app);
        let button = app
            .world_mut()
            .spawn(BodyAction {
                owner: function.id,
                scope: None,
                kind: BodyActionKind::Create(aestra_compiler::MaterialGraphCreateKind::Function(
                    aestra_compiler::MaterialGraphFunction::Multiply,
                )),
            })
            .id();
        app.world_mut().trigger(Activate { entity: button });
        let graph_key = crate::material_graph::function_graph_memory_key(root.path(), function.id);
        let placed = app
            .world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph_key);
        assert!(!placed.is_empty());
        let positions = placed
            .values()
            .map(|(position, _)| *position)
            .collect::<Vec<_>>();
        for (index, position) in positions.iter().enumerate() {
            assert!(!positions[index + 1..].contains(position));
        }
        app.world_mut().trigger(crate::history::HistoryAction::Undo);
        app.world_mut().flush();
        assert!(
            app.world()
                .resource::<GraphViewportMemory>()
                .base_nodes(&graph_key)
                .is_empty()
        );
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .graph_function(app.world().resource::<ProjectEffectCatalog>())
                .unwrap(),
            original_function
        );
        app.world_mut().trigger(crate::history::HistoryAction::Redo);
        app.world_mut().flush();
        assert_eq!(
            app.world()
                .resource::<GraphViewportMemory>()
                .base_nodes(&graph_key),
            placed
        );
        let source = app
            .world_mut()
            .spawn(Socket {
                owner: function.id,
                kind: SocketKind::Source(second),
                node: Entity::PLACEHOLDER,
            })
            .id();
        let destination = app
            .world_mut()
            .spawn(Socket {
                owner: function.id,
                kind: SocketKind::Target(Target::Output(output)),
                node: Entity::PLACEHOLDER,
            })
            .id();
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 800,
                    height: 600,
                },
                position: Vec2::ZERO,
            },
            DragStart {
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            source,
        ));
        assert!(
            app.world()
                .resource::<ConnectionPreview>()
                .1
                .contains(&destination)
        );
        assert!(
            !app.world()
                .resource::<ConnectionPreview>()
                .1
                .contains(&source)
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, effect);
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 800,
                    height: 600,
                },
                position: Vec2::ZERO,
            },
            DragDrop {
                button: PointerButton::Primary,
                dropped: source,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            destination,
        ));
        let session = app.world().resource::<EditorSession>();
        let current = session
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap();
        assert_eq!(current.outputs[0].expression, second);
        assert_eq!(current.expressions.len(), 5);
        assert_eq!(session.effect, effect);
        assert!(
            app.world()
                .resource::<FunctionEditor>()
                .available(session, true)
        );

        // Real picking can start on socket descendants. A near-socket release must
        // resolve the same endpoint, even without a DragDrop or a preview frame.
        app.world_mut()
            .resource_scope(|world, mut editor: Mut<FunctionEditor>| {
                world.resource_scope(|world, mut session: Mut<EditorSession>| {
                    editor
                        .step(
                            &mut session,
                            &mut world.resource_mut::<ProjectEffectCatalog>(),
                            true,
                        )
                        .unwrap();
                });
            });
        let source_child = app.world_mut().spawn(ChildOf(source)).id();
        app.world_mut().entity_mut(destination).insert((
            ComputedNode {
                inverse_scale_factor: 0.5,
                ..default()
            },
            UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::splat(200.0))),
        ));
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::new(110.0, 100.0),
        };
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location.clone(),
            DragStart {
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            source_child,
        ));
        assert_eq!(
            app.world().resource::<ConnectionPreview>().0.unwrap().0,
            source
        );
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location,
            DragEnd {
                button: PointerButton::Primary,
                distance: Vec2::ZERO,
            },
            source_child,
        ));
        assert!(app.world().resource::<ConnectionPreview>().0.is_none());
        let session = app.world().resource::<EditorSession>();
        assert_eq!(
            session
                .graph_function(app.world().resource::<ProjectEffectCatalog>())
                .unwrap()
                .outputs[0]
                .expression,
            second
        );
        assert_eq!(session.effect, effect);
    }
}
