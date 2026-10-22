//! Production function-native canvas composition shared by both engine qualification roots.
#[cfg(test)]
#[path = "composition/tests.rs"]
mod tests;
use crate::feathers::node_graph::geometry::{
    GraphDocumentKey, GraphGeometryNode, GraphGeometryPort, GraphGeometryView, GraphNodeKey,
    GraphViewKey,
};
use crate::feathers::{
    combo_box::{
        ComboOption, spawn_combo_control, spawn_compact_action_menu,
        spawn_searchable_icon_action_menu,
    },
    context_menu::{
        spawn_pointer_context_menu_item, spawn_pointer_context_menu_shortcut_item,
        spawn_pointer_context_menu_sized, spawn_pointer_context_submenu,
    },
    icon::load_svg_icon,
    node_graph::*,
    text_input::spawn_text_input,
};
use crate::material_function_editor::graph::{
    clipboard_context::{FunctionGraphNodeAction, View, ViewScope},
    menu_state::{FunctionGraphMenuKind, FunctionGraphMenuOpen, FunctionGraphMenuState, Target},
    model::*,
};
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_authoring::{MaterialConnectionTarget, MaterialExpressionInput};
use aestra_compiler::{
    MaterialCompiler, MaterialFunctionBodyProjection, MaterialFunctionGraphTarget,
};
use aestra_core::{
    MaterialExpressionId, MaterialFunctionId,
    material::{MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialValue},
};
use bevy::prelude::*;
use std::collections::BTreeMap;

pub(super) fn components(value: &MaterialValue) -> Vec<(&'static str, f32)> {
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

pub(super) fn target(value: &MaterialFunctionGraphTarget) -> Option<Target> {
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

#[allow(
    clippy::too_many_arguments,
    reason = "Preserve the explicit function-canvas composition contract"
)]
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
