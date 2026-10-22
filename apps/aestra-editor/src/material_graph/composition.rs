//! Actual material canvas assembly, semantic nodes, typed ports and wires.
#[cfg(test)]
#[path = "composition/tests.rs"]
mod tests;
use crate::material_graph::{
    MaterialGraphAction, MaterialGraphPreviewState, MaterialGraphPreviewTarget,
    MaterialGraphSelectionState, MaterialGraphViewport, MaterialSelectionScope,
    appearance_input::NodeButton,
    connections::{edge_target, input_target},
    controls::material_graph_value_component,
    keys::{
        MATERIAL_GRAPH_OUTPUT_NODE_KEY, material_graph_expression_node_key, material_graph_view_key,
    },
    menu_actions::MaterialGraphPreviewToggle,
    node_controls::*,
    ports::*,
    preview_surface::{self, MaterialGraphPreviewRaster},
    sockets::*,
    workspace_layout::{CANVAS_PADDING, layout_graph},
};
use crate::{
    Localizer,
    feathers::{
        icon::load_svg_icon,
        node_graph::{
            GraphModifiedDragTarget, GraphNodePreviewToggleProps, GraphNodeProps, GraphPortProps,
            GraphSocketSide, GraphViewportMemory, GraphViewportProps, PORT_ROW_HEIGHT,
            geometry::{
                GraphDocumentKey, GraphGeometryNode, GraphGeometryPort, GraphGeometryView,
                GraphNodeKey, GraphViewKey,
            },
            spawn_graph_node, spawn_graph_node_preview_toggle, spawn_graph_port,
            spawn_graph_port_with, spawn_graph_viewport,
        },
        number_input::ScrubbableNumber,
        scenes as ui_shell,
    },
    theme,
};
use aestra_authoring::{MaterialConnectionTarget, MaterialExpressionInput, MaterialOutputSocket};
use aestra_compiler::{
    MaterialGraphNode, MaterialGraphNodeKind, MaterialGraphOutput, MaterialGraphOutputKind,
    MaterialGraphProjection,
};
use aestra_core::{
    MaterialExpressionId, MaterialId, MaterialProgramId,
    material::{MaterialExpressionKind, MaterialProgram, MaterialValue, MaterialValueType},
};
use bevy::{prelude::*, ui::Checked};
use std::collections::BTreeSet;

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct MaterialGraphCanvas;

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_material_graph_canvas(
    panel: &mut ChildSpawnerCommands,
    project_root: &std::path::Path,
    projection: &MaterialGraphProjection,
    program_definition: &MaterialProgram,
    target: &crate::material_document::MaterialEditingTarget,
    view: MaterialSelectionScope,
    viewport_key: &str,
    instance: Option<MaterialId>,
    selection: &MaterialGraphSelectionState,
    previews: &MaterialGraphPreviewState,
    graph_memory: &GraphViewportMemory,
    localizer: &Localizer,
    asset_server: &AssetServer,
) -> Entity {
    let layout = layout_graph(projection, previews);
    let graph_key = material_graph_view_key(projection.program);
    // Seed this view's camera from its own remembered pan/zoom, else the document camera
    // (the last-saved / tool-panel camera), so a reopened or split view starts where the
    // document was left rather than always re-framing.
    let initial_view = graph_memory
        .view(viewport_key)
        .or_else(|| graph_memory.view(&graph_key));
    let viewport = spawn_graph_viewport(
        panel,
        GraphViewportProps {
            key: viewport_key.to_owned(),
            content_size: layout.size,
            selection_bounds: None,
            initial_view,
        },
        MaterialGraphCanvas,
        |overlay| spawn_graph_wires(overlay, projection),
        |canvas| {
            let inline_constants = program_definition.inline_constants();
            for node in &projection.nodes {
                let position = layout
                    .nodes
                    .get(&node.expression)
                    .copied()
                    .unwrap_or(Vec2::splat(CANVAS_PADDING));
                spawn_expression_node(
                    canvas,
                    projection.program,
                    target,
                    node,
                    program_definition,
                    &inline_constants,
                    position,
                    selection,
                    view,
                    previews,
                    instance,
                    localizer,
                    asset_server,
                    &graph_key,
                    graph_memory,
                );
            }
            spawn_output_node(
                canvas,
                projection.program,
                target,
                &projection.outputs,
                previews,
                instance,
                layout.output,
                localizer,
                asset_server,
                &graph_key,
                graph_memory,
            );
        },
    );
    panel.commands().entity(viewport).insert((
        GraphGeometryView {
            key: GraphViewKey {
                document: GraphDocumentKey {
                    project: project_root.to_owned(),
                    asset: crate::document::DocumentKey::MaterialProgram(projection.program),
                },
                view,
            },
            nodes: projection
                .nodes
                .iter()
                .map(|node| GraphNodeKey::Expression(node.expression))
                .chain([GraphNodeKey::MaterialOutputs])
                .collect(),
        },
        MaterialGraphViewport {
            program: projection.program,
            editing_target: target.clone(),
            scope: view,
        },
    ));

    viewport
}

pub(super) fn spawn_graph_wires(
    parent: &mut ChildSpawnerCommands,
    graph: &MaterialGraphProjection,
) {
    for edge in &graph.edges {
        let Some(target) = edge_target(&edge.target) else {
            continue;
        };
        parent.spawn((
            MaterialGraphWire {
                program: graph.program,
                source: edge.source,
                target,
                color: wire_color(edge.value_type),
            },
            crate::feathers::node_graph::insertion::Wire::material(edge.source, target),
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

pub(super) fn inline_material_graph_default<'a>(
    program: &'a MaterialProgram,
    inline_constants: &BTreeSet<MaterialExpressionId>,
    source: MaterialExpressionId,
) -> Option<&'a MaterialValue> {
    if !inline_constants.contains(&source) {
        return None;
    }
    program
        .expressions
        .iter()
        .find(|expression| expression.id == source)
        .and_then(|expression| match &expression.kind {
            MaterialExpressionKind::Constant(value) => Some(value),
            _ => None,
        })
}

pub(super) fn spawn_material_graph_default_control(
    parent: &mut ChildSpawnerCommands,
    program: MaterialProgramId,
    editing_target: &crate::material_document::MaterialEditingTarget,
    expression: MaterialExpressionId,
    value: &MaterialValue,
) {
    // The control sits immediately after the port label; no flex spacer pushing it to the far
    // right of the node.
    match value {
        MaterialValue::Float(number) => {
            parent
                .spawn(Node {
                    width: Val::Px(66.0),
                    height: Val::Px(20.0),
                    ..default()
                })
                .with_children(|container| {
                    container
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_scalar_input())
                        .insert((
                            MaterialGraphDefaultNumberControl {
                                program,
                                editing_target: editing_target.clone(),
                                expression,
                                value: value.clone(),
                                component: 0,
                            },
                            ScrubbableNumber::new(*number, -f32::MAX, f32::MAX, 0.01),
                            AccessibleLabel("Material input default".into()),
                        ));
                });
        }
        MaterialValue::Bool(enabled) => {
            let mut checkbox = parent.spawn_empty();
            checkbox.apply_scene(ui_shell::feathers_checkbox()).insert((
                MaterialGraphDefaultToggleControl {
                    program,
                    editing_target: editing_target.clone(),
                    expression,
                    value: *enabled,
                },
                AccessibleLabel("Material input default".into()),
            ));
            if *enabled {
                checkbox.insert(Checked);
            }
        }
        MaterialValue::Vec2(_)
        | MaterialValue::Vec3(_)
        | MaterialValue::Vec4(_)
        | MaterialValue::ColorSrgb(_)
        | MaterialValue::Texture2D(_) => {}
    }
}

pub(super) fn spawn_material_graph_constant_editor(
    parent: &mut ChildSpawnerCommands,
    program: MaterialProgramId,
    editing_target: &crate::material_document::MaterialEditingTarget,
    expression: MaterialExpressionId,
    value: &MaterialValue,
) {
    let labels: &[&str] = match value {
        MaterialValue::Float(_) => &["Value"],
        MaterialValue::Vec2(_) => &["X", "Y"],
        MaterialValue::Vec3(_) => &["X", "Y", "Z"],
        MaterialValue::Vec4(_) => &["X", "Y", "Z", "W"],
        MaterialValue::ColorSrgb(_) => &["R", "G", "B", "A"],
        MaterialValue::Bool(enabled) => {
            parent
                .spawn(Node {
                    height: Val::Px(PORT_ROW_HEIGHT),
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        Text::new("Value"),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        Pickable::IGNORE,
                    ));
                    row.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    let mut checkbox = row.spawn_empty();
                    checkbox.apply_scene(ui_shell::feathers_checkbox()).insert((
                        MaterialGraphDefaultToggleControl {
                            program,
                            editing_target: editing_target.clone(),
                            expression,
                            value: *enabled,
                        },
                        AccessibleLabel("Material constant value".into()),
                    ));
                    if *enabled {
                        checkbox.insert(Checked);
                    }
                });
            return;
        }
        MaterialValue::Texture2D(_) => return,
    };
    for (component, label) in labels.iter().enumerate() {
        let Some(number) = material_graph_value_component(value, component as u8) else {
            continue;
        };
        let (min, max, step) = if matches!(value, MaterialValue::ColorSrgb(_)) {
            (0.0, 1.0, 0.01)
        } else {
            (-f32::MAX, f32::MAX, 0.01)
        };
        parent
            .spawn(Node {
                height: Val::Px(PORT_ROW_HEIGHT),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(5.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Text::new(*label),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                    Pickable::IGNORE,
                ));
                row.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                row.spawn(Node {
                    width: Val::Px(88.0),
                    height: Val::Px(20.0),
                    ..default()
                })
                .with_children(|container| {
                    container
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_scalar_input())
                        .insert((
                            MaterialGraphDefaultNumberControl {
                                program,
                                editing_target: editing_target.clone(),
                                expression,
                                value: value.clone(),
                                component: component as u8,
                            },
                            ScrubbableNumber::new(number, min, max, step),
                            AccessibleLabel(format!("Material constant {label}")),
                        ));
                });
            });
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_expression_node(
    parent: &mut ChildSpawnerCommands,
    program: MaterialProgramId,
    editing_target: &crate::material_document::MaterialEditingTarget,
    node: &MaterialGraphNode,
    program_definition: &MaterialProgram,
    inline_constants: &BTreeSet<MaterialExpressionId>,
    position: Vec2,
    selection: &MaterialGraphSelectionState,
    scope: MaterialSelectionScope,
    previews: &MaterialGraphPreviewState,
    instance: Option<MaterialId>,
    localizer: &Localizer,
    asset_server: &AssetServer,
    graph_key: &str,
    graph_memory: &GraphViewportMemory,
) {
    let selected = selection.is_expression_selected(scope, program, node.expression);
    let target = MaterialGraphPreviewTarget::Expression(node.expression);
    let preview_visible = previews.is_visible(program, target);
    let graph_node = spawn_graph_node(
        parent,
        GraphNodeProps {
            graph_key: graph_key.to_owned(),
            node_key: material_graph_expression_node_key(node.expression),
            title: if node.kind == MaterialGraphNodeKind::Constant {
                "Constant".to_owned()
            } else {
                node.label.clone()
            },
            position,
            selected,
            pinned: graph_memory.is_pinned(
                graph_key,
                &material_graph_expression_node_key(node.expression),
            ),
            muted: !node.reachable || node.disabled || node.validation_message.is_some(),
            collapse_icon: load_svg_icon(asset_server, "icons/chevron-down.svg"),
            expand_icon: load_svg_icon(asset_server, "icons/chevron-right.svg"),
            pin_icon: load_svg_icon(asset_server, "icons/pin.svg"),
            collapse_label: localizer.text("material-graph-collapse-node"),
            expand_label: localizer.text("material-graph-expand-node"),
        },
        (
            NodeButton,
            GraphGeometryNode::new(
                GraphNodeKey::Expression(node.expression),
                node,
                preview_visible,
            ),
            MaterialGraphAction {
                program,
                expression: node.expression,
            },
            MaterialGraphNodeScope(scope),
            GraphModifiedDragTarget,
        ),
        |graph_node, body| {
            if node.kind == MaterialGraphNodeKind::Constant
                && let Some(value) = program_definition
                    .expressions
                    .iter()
                    .find(|expression| expression.id == node.expression)
                    .and_then(|expression| match &expression.kind {
                        MaterialExpressionKind::Constant(value) => Some(value),
                        _ => None,
                    })
            {
                spawn_material_graph_constant_editor(
                    body,
                    program,
                    editing_target,
                    node.expression,
                    value,
                );
            }
            if node.disabled || !node.reachable || node.validation_message.is_some() {
                let state = node.validation_message.clone().unwrap_or_else(|| {
                    match (node.disabled, node.reachable) {
                        (true, false) => format!(
                            "{} · {}",
                            localizer.text("material-graph-disabled"),
                            localizer.text("material-graph-unreachable")
                        ),
                        (true, true) => localizer.text("material-graph-disabled"),
                        (false, false) => localizer.text("material-graph-unreachable"),
                        (false, true) => String::new(),
                    }
                });
                body.spawn((
                    Text::new(state),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.58, 0.30)),
                    Node {
                        margin: UiRect::horizontal(Val::Px(9.0)),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
            }
            for port in &node.inputs {
                let target = port.function_input.map_or_else(
                    || input_target(node.expression, &port.name),
                    |input| {
                        Some(MaterialConnectionTarget::ExpressionInput {
                            expression: node.expression,
                            input: MaterialExpressionInput::FunctionArgument(input),
                        })
                    },
                );
                let Some(target) = target else {
                    continue;
                };
                let presentation = input_port_presentation(&port.name);
                let inline_default = inline_material_graph_default(
                    program_definition,
                    inline_constants,
                    port.source,
                );
                spawn_graph_port_with(
                    body,
                    GraphPortProps {
                        label: Some(presentation.label.to_owned()),
                        tooltip_title: material_slot_title(
                            &format!("{} input", presentation.label),
                            port.value_type,
                        ),
                        tooltip_description: material_slot_description(
                            &presentation.description,
                            port.evaluation_domain,
                        ),
                        side: GraphSocketSide::Input,
                        color: socket_color(port.value_type),
                    },
                    (
                        GraphGeometryPort::from(target),
                        MaterialGraphSocket {
                            program,
                            kind: MaterialGraphSocketKind::ConnectionInput(target),
                        },
                        MaterialGraphSocketAnchor {
                            node: graph_node,
                            offset: None,
                        },
                    ),
                    |row| {
                        if let Some(value) = inline_default {
                            spawn_material_graph_default_control(
                                row,
                                program,
                                editing_target,
                                port.source,
                                value,
                            );
                        }
                    },
                );
            }
            spawn_graph_port(
                body,
                GraphPortProps {
                    label: None,
                    tooltip_title: material_slot_title("Output", node.value_type),
                    tooltip_description: material_slot_description(
                        &format!("Value produced by the {} node.", node.label),
                        node.evaluation_domain,
                    ),
                    side: GraphSocketSide::Output,
                    color: socket_color(node.value_type),
                },
                (
                    GraphGeometryPort::Output,
                    MaterialGraphSocket {
                        program,
                        kind: MaterialGraphSocketKind::ExpressionOutput(node.expression),
                    },
                    MaterialGraphSocketAnchor {
                        node: graph_node,
                        offset: None,
                    },
                ),
            );
            if preview_visible {
                preview_surface::spawn(
                    body,
                    previews,
                    MaterialGraphPreviewRaster {
                        program,
                        editing_target: editing_target.clone(),
                        instance,
                        target,
                        value_type: node.value_type,
                    },
                );
            }
        },
    );
    parent
        .commands()
        .entity(graph_node)
        .with_children(|graph_node| {
            spawn_graph_node_preview_toggle(
                graph_node,
                preview_toggle_props(preview_visible, localizer, asset_server),
                MaterialGraphPreviewToggle {
                    program,
                    editing_target: editing_target.clone(),
                    target,
                },
            );
        });
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_output_node(
    parent: &mut ChildSpawnerCommands,
    program: MaterialProgramId,
    editing_target: &crate::material_document::MaterialEditingTarget,
    outputs: &[MaterialGraphOutput],
    previews: &MaterialGraphPreviewState,
    instance: Option<MaterialId>,
    position: Vec2,
    localizer: &Localizer,
    asset_server: &AssetServer,
    graph_key: &str,
    graph_memory: &GraphViewportMemory,
) {
    let target = MaterialGraphPreviewTarget::Output;
    let preview_visible = previews.is_visible(program, target);
    let graph_node = spawn_graph_node(
        parent,
        GraphNodeProps {
            graph_key: graph_key.to_owned(),
            node_key: MATERIAL_GRAPH_OUTPUT_NODE_KEY.to_owned(),
            title: localizer.text("material-graph-outputs"),
            position,
            selected: false,
            pinned: graph_memory.is_pinned(graph_key, MATERIAL_GRAPH_OUTPUT_NODE_KEY),
            muted: false,
            collapse_icon: load_svg_icon(asset_server, "icons/chevron-down.svg"),
            expand_icon: load_svg_icon(asset_server, "icons/chevron-right.svg"),
            pin_icon: load_svg_icon(asset_server, "icons/pin.svg"),
            collapse_label: localizer.text("material-graph-collapse-node"),
            expand_label: localizer.text("material-graph-expand-node"),
        },
        GraphGeometryNode::new(GraphNodeKey::MaterialOutputs, &outputs, preview_visible),
        |graph_node, body| {
            for output in outputs {
                let (label, description) = match output.kind {
                    MaterialGraphOutputKind::Color => {
                        ("Color", "Final color written by the material.")
                    }
                    MaterialGraphOutputKind::Alpha => {
                        ("Alpha", "Final opacity written by the material.")
                    }
                    MaterialGraphOutputKind::VertexOffset => (
                        "Vertex Offset",
                        "Mesh-local displacement before particle rotation and scale. Unconnected means zero. Fragment-only operations cannot be used here.",
                    ),
                };
                let target = MaterialConnectionTarget::ProgramOutput(match output.kind {
                    MaterialGraphOutputKind::Color => MaterialOutputSocket::Color,
                    MaterialGraphOutputKind::Alpha => MaterialOutputSocket::Alpha,
                    MaterialGraphOutputKind::VertexOffset => MaterialOutputSocket::VertexOffset,
                });
                spawn_graph_port(
                    body,
                    GraphPortProps {
                        label: Some(label.into()),
                        tooltip_title: material_slot_title(
                            &format!("{label} input"),
                            output.value_type,
                        ),
                        tooltip_description: material_slot_description(
                            description,
                            output.evaluation_domain,
                        ),
                        side: GraphSocketSide::Input,
                        color: socket_color(output.value_type),
                    },
                    (
                        GraphGeometryPort::from(target),
                        MaterialGraphSocket {
                            program,
                            kind: MaterialGraphSocketKind::ConnectionInput(target),
                        },
                        MaterialGraphSocketAnchor {
                            node: graph_node,
                            offset: None,
                        },
                    ),
                );
            }
            if preview_visible {
                preview_surface::spawn(
                    body,
                    previews,
                    MaterialGraphPreviewRaster {
                        program,
                        editing_target: editing_target.clone(),
                        instance,
                        target,
                        value_type: Some(MaterialValueType::Color),
                    },
                );
            }
        },
    );
    parent
        .commands()
        .entity(graph_node)
        .with_children(|graph_node| {
            spawn_graph_node_preview_toggle(
                graph_node,
                preview_toggle_props(preview_visible, localizer, asset_server),
                MaterialGraphPreviewToggle {
                    program,
                    editing_target: editing_target.clone(),
                    target,
                },
            );
        });
}

pub(super) fn preview_toggle_props(
    visible: bool,
    localizer: &Localizer,
    asset_server: &AssetServer,
) -> GraphNodePreviewToggleProps {
    GraphNodePreviewToggleProps {
        visible,
        show_icon: load_svg_icon(asset_server, "icons/show.svg"),
        hide_icon: load_svg_icon(asset_server, "icons/hide.svg"),
        show_label: localizer.text("material-graph-show-preview"),
        hide_label: localizer.text("material-graph-hide-preview"),
    }
}
