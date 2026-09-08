//! Function-native canvas: no surrogate material program or effect is created.
use super::*;
use crate::feathers::{
    combo_box::{spawn_compact_action_menu, spawn_searchable_icon_action_menu},
    icon::load_svg_icon,
    node_graph::*,
};
use aestra_authoring::{
    MaterialConnectionTarget, MaterialExpressionInput, MaterialFunctionBodyCommand as Edit,
};
use aestra_compiler::{
    MaterialCompiler, MaterialFunctionBodyProjection, MaterialFunctionGraphTarget,
};

#[derive(Component, Clone, Copy)]
struct View(MaterialFunctionId);
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Input(MaterialExpressionId, MaterialExpressionInput),
    Output(MaterialFunctionOutputId),
}
#[derive(Clone, Copy)]
enum SocketKind {
    Source(MaterialExpressionId),
    Target(Target),
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
struct ConnectionPreview(Option<(Entity, Vec2)>);
#[derive(Component)]
struct PreviewWire(MaterialFunctionId);
#[derive(Component)]
struct Wire {
    owner: MaterialFunctionId,
    source: MaterialExpressionId,
    target: Target,
}
#[derive(Component, Clone, Copy)]
struct BodyAction {
    owner: MaterialFunctionId,
    kind: BodyActionKind,
}
#[derive(Clone, Copy)]
enum BodyActionKind {
    Back,
    Locate,
    Float,
    Literal(MaterialValueType),
    Boolean(MaterialExpressionId, bool),
    Input(MaterialFunctionInputId),
    Add,
    Multiply,
    Smoothstep,
    Remove(MaterialExpressionId),
}
#[derive(Component)]
struct Constant {
    owner: MaterialFunctionId,
    expression: MaterialExpressionId,
    component: usize,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<ConnectionPreview>()
        .add_observer(start_connection)
        .add_observer(move_connection)
        .add_observer(end_connection)
        .add_observer(action)
        .add_observer(drop_socket)
        .add_observer(constant_text)
        .add_observer(constant_number)
        .add_systems(Update, attach_wires)
        .add_systems(PostUpdate, update_wires.after(bevy::ui::UiSystems::Layout));
}

fn start_connection(
    mut event: On<Pointer<DragStart>>,
    sockets: Query<(), With<Socket>>,
    mut preview: ResMut<ConnectionPreview>,
) {
    if event.button == bevy::picking::pointer::PointerButton::Primary
        && sockets.contains(event.entity)
    {
        preview.0 = Some((event.entity, event.pointer_location.position));
        event.propagate(false);
    }
}
fn move_connection(mut event: On<Pointer<Drag>>, mut preview: ResMut<ConnectionPreview>) {
    if let Some((entity, cursor)) = &mut preview.0
        && *entity == event.entity
    {
        *cursor = event.pointer_location.position;
        event.propagate(false);
    }
}
fn end_connection(mut event: On<Pointer<DragEnd>>, mut preview: ResMut<ConnectionPreview>) {
    if preview.0.is_some_and(|(entity, _)| entity == event.entity) {
        preview.0 = None;
        event.propagate(false);
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
    sockets: Query<&Socket>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
) {
    if event.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    let (Ok(from), Ok(to)) = (sockets.get(event.dropped), sockets.get(event.entity)) else {
        return;
    };
    if from.owner != to.owner || session.standalone_function() != Some(from.owner) {
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
    let result = editor.edit_body(
        &mut session,
        &mut catalog,
        vec![connection_edit(source, target)],
    );
    finish(&mut session, result);
}

fn create_edits(function: &MaterialFunction, action: BodyActionKind) -> Vec<Edit> {
    if let BodyActionKind::Remove(expression) = action {
        return vec![Edit::Remove { expression }];
    }
    if let BodyActionKind::Boolean(expression, value) = action {
        return vec![Edit::Replace {
            expression,
            replacement: MaterialExpression {
                id: expression,
                kind: MaterialExpressionKind::Constant(MaterialValue::Bool(value)),
            },
        }];
    }
    let mut expressions = Vec::new();
    let mut constant = |value| {
        let id = MaterialExpressionId::new();
        expressions.push(MaterialExpression {
            id,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(value)),
        });
        id
    };
    let kind = match action {
        BodyActionKind::Float => MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        BodyActionKind::Literal(kind) => MaterialExpressionKind::Constant(match kind {
            MaterialValueType::Float => MaterialValue::Float(0.0),
            MaterialValueType::Vec2 => MaterialValue::Vec2([0.0; 2]),
            MaterialValueType::Vec3 => MaterialValue::Vec3([0.0; 3]),
            MaterialValueType::Vec4 => MaterialValue::Vec4([0.0; 4]),
            MaterialValueType::Color => MaterialValue::ColorSrgb([1.0; 4]),
            MaterialValueType::Bool => MaterialValue::Bool(false),
            MaterialValueType::Texture2D(_) => return Vec::new(),
        }),
        BodyActionKind::Input(id) => MaterialExpressionKind::FunctionInput(id),
        BodyActionKind::Add => MaterialExpressionKind::Add(constant(0.0), constant(0.0)),
        BodyActionKind::Multiply => MaterialExpressionKind::Multiply(constant(1.0), constant(1.0)),
        BodyActionKind::Smoothstep => MaterialExpressionKind::Smoothstep {
            edge_min: constant(0.0),
            edge_max: constant(1.0),
            value: constant(0.5),
        },
        BodyActionKind::Remove(_)
        | BodyActionKind::Back
        | BodyActionKind::Locate
        | BodyActionKind::Boolean(..) => unreachable!(),
    };
    expressions.push(MaterialExpression {
        id: MaterialExpressionId::new(),
        kind,
    });
    expressions
        .into_iter()
        .enumerate()
        .map(|(index, expression)| Edit::Add {
            expression,
            index: function.expressions.len() + index,
        })
        .collect()
}

fn action(
    event: On<Activate>,
    actions: Query<&BodyAction>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut commands: Commands,
) {
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    if session.standalone_function() != Some(action.owner) {
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
    let result = session.graph_function(&catalog).and_then(|function| {
        editor.edit_body(
            &mut session,
            &mut catalog,
            create_edits(&function, action.kind),
        )
    });
    finish(&mut session, result);
}

fn replace_constant(
    control: &Constant,
    value: f32,
    editor: &mut FunctionEditor,
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
) {
    if session.standalone_function() != Some(control.owner) {
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

pub(crate) fn spawn(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    assets: &AssetServer,
    memory: &GraphViewportMemory,
) {
    let Ok(function) = session.graph_function(catalog) else {
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
    let graph_key = format!("function:{}:{}", catalog.root().display(), function.id);
    let mut inputs = nodes
        .iter()
        .map(|node| (node.id, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in &edges {
        if let Some(Target::Input(expression, _)) = target(&edge.target) {
            inputs.entry(expression).or_default().push(edge.source);
        }
    }
    let mut depths = BTreeMap::new();
    for node in &nodes {
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
            let position = memory
                .node_position(&graph_key, &expression.id.to_string())
                .unwrap_or(initial);
            (expression.id, position)
        })
        .collect::<BTreeMap<_, _>>();
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
                },
            );
            let mut options = vec![
                ("Float", BodyActionKind::Float),
                ("Vector 2", BodyActionKind::Literal(MaterialValueType::Vec2)),
                ("Vector 3", BodyActionKind::Literal(MaterialValueType::Vec3)),
                ("Vector 4", BodyActionKind::Literal(MaterialValueType::Vec4)),
                ("Color", BodyActionKind::Literal(MaterialValueType::Color)),
                ("Boolean", BodyActionKind::Literal(MaterialValueType::Bool)),
                ("Add", BodyActionKind::Add),
                ("Multiply", BodyActionKind::Multiply),
                ("Smoothstep", BodyActionKind::Smoothstep),
            ]
            .into_iter()
            .map(|(name, kind)| ComboOption {
                label: name.into(),
                selected: false,
                action: BodyAction {
                    owner: function.id,
                    kind,
                },
            })
            .collect::<Vec<_>>();
            options.extend(function.inputs.iter().map(|input| ComboOption {
                label: format!("Input: {}", input.name),
                selected: false,
                action: BodyAction {
                    owner: function.id,
                    kind: BodyActionKind::Input(input.id),
                },
            }));
            spawn_searchable_icon_action_menu(
                toolbar,
                assets,
                "icons/plus.svg",
                "Add node",
                "Add a function node",
                &options,
            );
            spawn_graph_frame_button(
                toolbar,
                assets,
                "icons/frame-all.svg",
                "Frame all".into(),
                GraphFrameAction::new(&graph_key, GraphFrameTarget::All),
            );
            spawn_graph_tool_button(
                toolbar,
                assets,
                "icons/folder.svg",
                "Locate in Assets".into(),
                BodyAction {
                    owner: function.id,
                    kind: BodyActionKind::Locate,
                },
            );
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
    let output_position = Vec2::new(
        34.0 + (depths.values().copied().max().unwrap_or_default() + 1) as f32 * 282.0,
        122.0,
    );
    let extent = Vec2::new(
        (output_position.x + NODE_WIDTH + 34.0).max(720.0),
        (columns.values().copied().reduce(f32::max).unwrap_or(0.0) + 34.0).max(420.0),
    );
    let viewport = spawn_graph_viewport(
        parent,
        GraphViewportProps {
            key: graph_key.clone(),
            content_size: extent,
            selection_bounds: None,
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
                    assets,
                );
                spawn_graph_node(canvas, props, (), |node, body| {
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
                    } else if inputs[&expression.id].is_empty() {
                        body.spawn((
                            Node {
                                min_height: Val::Px(26.0),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                    }
                    body.commands().entity(node).with_children(|node| {
                        node.spawn(Node {
                            position_type: PositionType::Absolute,
                            right: Val::Px(28.0),
                            top: Val::Px(2.0),
                            ..default()
                        })
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
                                    },
                                }],
                            )
                        });
                    });
                });
            }
            spawn_graph_node(
                canvas,
                props(
                    &graph_key,
                    "outputs",
                    "Function outputs".into(),
                    output_position,
                    assets,
                ),
                (),
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
    parent.commands().entity(viewport).insert(View(function.id));
}

fn props(
    graph: &str,
    node: &str,
    title: String,
    position: Vec2,
    assets: &AssetServer,
) -> GraphNodeProps {
    GraphNodeProps {
        graph_key: graph.into(),
        node_key: node.into(),
        title,
        position,
        selected: false,
        muted: false,
        collapse_icon: load_svg_icon(assets, "icons/chevron-down.svg"),
        expand_icon: load_svg_icon(assets, "icons/chevron-right.svg"),
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
        (Socket { owner, kind, node }, Anchor::default()),
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
    wires: Query<(&Wire, &MaterialNode<GraphWireMaterial>)>,
    mut sockets: Query<(&Socket, &UiGlobalTransform, &mut Anchor)>,
    nodes: Query<(&FeathersGraphNode, &ComputedNode, &UiGlobalTransform)>,
    viewports: Query<(&View, &FeathersGraphViewport)>,
    previews: Query<(&PreviewWire, &MaterialNode<GraphWireMaterial>)>,
    mut preview: ResMut<ConnectionPreview>,
    preview_sockets: Query<(&Socket, &UiGlobalTransform)>,
    viewport_geometry: Query<(&View, &ComputedNode, &UiGlobalTransform)>,
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
        let offset = *anchor.0.get_or_insert_with(|| {
            node_transform.try_inverse().map_or(Vec2::ZERO, |inverse| {
                inverse.transform_point2(world) + computed.size() * 0.5
            })
        });
        positions.push((socket, node.position() + offset));
    }
    for (wire, handle) in &wires {
        let Some((_, viewport)) = viewports.iter().find(|(view, _)| view.0 == wire.owner) else {
            continue;
        };
        let source = positions.iter().find(|(socket, _)| {
            socket.owner == wire.owner
                && matches!(socket.kind, SocketKind::Source(id) if id == wire.source)
        });
        let target = positions.iter().find(|(socket, _)| {
            socket.owner == wire.owner
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
            Vec4::new(0.4, 0.8, 1.0, 1.0),
            2.0,
        );
    }
    if preview
        .0
        .is_some_and(|(entity, _)| !preview_sockets.contains(entity))
    {
        preview.0 = None;
    }
    for (ghost, handle) in &previews {
        let endpoints = preview.0.and_then(|(entity, cursor)| {
            let (socket, transform) = preview_sockets.get(entity).ok()?;
            if socket.owner != ghost.0 {
                return None;
            }
            let (_, computed, viewport_transform) = viewport_geometry
                .iter()
                .find(|(view, _, _)| view.0 == ghost.0)?;
            let (_, _, origin) = transform.to_scale_angle_translation();
            let start = crate::feathers::context_menu::pointer_position_in_node(
                origin,
                computed,
                viewport_transform,
            );
            let end = crate::feathers::context_menu::pointer_position_in_node(
                cursor / computed.inverse_scale_factor,
                computed,
                viewport_transform,
            );
            Some(if matches!(socket.kind, SocketKind::Source(_)) {
                (start, end)
            } else {
                (end, start)
            })
        });
        let (start, end) = endpoints.unwrap_or((Vec2::ZERO, Vec2::ZERO));
        crate::material_graph::update_wire_material(
            &mut materials,
            &handle.0,
            start,
            end,
            Vec4::new(0.4, 0.8, 1.0, if endpoints.is_some() { 1.0 } else { 0.0 }),
            2.0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut app = App::new();
        app.insert_resource(session).insert_resource(catalog);
        super::super::register(&mut app);
        let button = app
            .world_mut()
            .spawn(BodyAction {
                owner: function.id,
                kind: BodyActionKind::Multiply,
            })
            .id();
        app.world_mut().trigger(Activate { entity: button });
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
    }
}
