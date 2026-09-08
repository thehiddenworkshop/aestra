//! Function-native canvas: no surrogate material program or effect is created.
use super::*;
use crate::feathers::{icon::load_svg_icon, node_graph::*};
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
    Float,
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
}

pub(super) fn register(app: &mut App) {
    app.add_observer(action)
        .add_observer(drop_socket)
        .add_observer(constant_text)
        .add_observer(constant_number)
        .add_systems(Update, attach_wires)
        .add_systems(PostUpdate, update_wires.after(bevy::ui::UiSystems::Layout));
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
        BodyActionKind::Input(id) => MaterialExpressionKind::FunctionInput(id),
        BodyActionKind::Add => MaterialExpressionKind::Add(constant(0.0), constant(0.0)),
        BodyActionKind::Multiply => MaterialExpressionKind::Multiply(constant(1.0), constant(1.0)),
        BodyActionKind::Smoothstep => MaterialExpressionKind::Smoothstep {
            edge_min: constant(0.0),
            edge_max: constant(1.0),
            value: constant(0.5),
        },
        BodyActionKind::Remove(_) => unreachable!(),
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
) {
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    if session.standalone_function() != Some(action.owner) {
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
    let result = if value.is_finite() {
        editor.edit_body(
            session,
            catalog,
            vec![Edit::Replace {
                expression: control.expression,
                replacement: MaterialExpression {
                    id: control.expression,
                    kind: MaterialExpressionKind::Constant(MaterialValue::Float(value)),
                },
            }],
        )
    } else {
        Err("Constant must be finite".into())
    };
    finish(session, result);
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
    let saved_bottom = nodes
        .iter()
        .filter_map(|node| memory.node_position(&graph_key, &node.id.to_string()))
        .map(|position| position.y + 240.0)
        .reduce(f32::max);
    let mut new_index = 0;
    let positions = nodes
        .iter()
        .map(|expression| {
            let position = memory
                .node_position(&graph_key, &expression.id.to_string())
                .unwrap_or_else(|| {
                    let position = Vec2::new(
                        30.0 + (new_index % 3) as f32 * 290.0,
                        saved_bottom.unwrap_or(30.0) + (new_index / 3) as f32 * 240.0,
                    );
                    new_index += 1;
                    position
                });
            (expression.id, position)
        })
        .collect::<BTreeMap<_, _>>();
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|toolbar| {
            let mut options = vec![
                ("Float", BodyActionKind::Float),
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
            spawn_combo_control(toolbar, "+ Add node", "Add function node", &options, 144.0);
            spawn_graph_frame_button(
                toolbar,
                assets,
                "icons/frame-all.svg",
                "Frame all".into(),
                GraphFrameAction::new(&graph_key, GraphFrameTarget::All),
            );
            label(
                toolbar,
                "Drag between sockets to connect · Edit signature in Properties",
            );
        });
    let extent = Vec2::new(1180.0, (nodes.len().div_ceil(3) as f32 * 240.0).max(320.0));
    let viewport = spawn_graph_viewport(
        parent,
        GraphViewportProps {
            key: graph_key.clone(),
            content_size: extent,
            selection_bounds: None,
        },
        (),
        |wires| {
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
                                    port.replace('_', " ")
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
                    if let MaterialExpressionKind::Constant(MaterialValue::Float(value)) =
                        expression.kind
                    {
                        body.spawn(Node {
                            width: Val::Px(170.0),
                            min_height: Val::Px(28.0),
                            ..default()
                        })
                        .with_children(|row| {
                            let control = spawn_text_input(
                                row,
                                &value.to_string(),
                                "Constant value",
                                Constant {
                                    owner: function.id,
                                    expression: expression.id,
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
                    } else {
                        body.spawn((
                            Node {
                                min_height: Val::Px(26.0),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                    }
                    spawn_action_button(
                        body,
                        "Delete node",
                        BodyAction {
                            owner: function.id,
                            kind: BodyActionKind::Remove(expression.id),
                        },
                        false,
                    );
                });
            }
            spawn_graph_node(
                canvas,
                props(
                    &graph_key,
                    "outputs",
                    "Function outputs".into(),
                    Vec2::new(910.0, 30.0),
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
                            Some(format!("{} [{:?}]", output.name, output.value_type)),
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
    wires: Query<Entity, (With<Wire>, Without<MaterialNode<GraphWireMaterial>>)>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
