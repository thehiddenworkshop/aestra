//! Production function wire attachment and post-layout presentation.
use crate::feathers::node_graph::{
    self as graph, FeathersGraphNode, FeathersGraphViewport, GraphWireMaterial,
};
use crate::material_function_editor::graph::{
    clipboard_context::View,
    connection::ConnectionPreview,
    model::{Anchor, PreviewWire, Socket, SocketKind, Wire},
};
use bevy::{prelude::*, ui_render::ui_material::MaterialNode};
use graph::insertion;

pub(super) fn register(app: &mut App) {
    app.add_systems(Update, attach_wires).add_systems(
        PostUpdate,
        update_wires.after(bevy::ui::UiSystems::PostLayout),
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
#[path = "../tests/graph_wires.rs"]
mod tests;
