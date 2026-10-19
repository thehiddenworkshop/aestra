//! Original material socket identities and geometry used by picking, wires and connections.
use crate::feathers::node_graph::{FeathersGraphNode, GraphSocketSide};
use aestra_authoring::MaterialConnectionTarget;
use aestra_core::{MaterialExpressionId, MaterialProgramId};
use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MaterialGraphSocketKind {
    ExpressionOutput(MaterialExpressionId),
    ConnectionInput(MaterialConnectionTarget),
}

impl MaterialGraphSocketKind {
    pub(super) fn side(self) -> GraphSocketSide {
        match self {
            Self::ExpressionOutput(_) => GraphSocketSide::Output,
            Self::ConnectionInput(_) => GraphSocketSide::Input,
        }
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct MaterialGraphSocket {
    pub(super) program: MaterialProgramId,
    pub(super) kind: MaterialGraphSocketKind,
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct MaterialGraphSocketAnchor {
    pub(super) node: Entity,
    pub(super) offset: Option<Vec2>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MaterialGraphSocketPosition {
    pub(super) program: MaterialProgramId,
    pub(super) kind: MaterialGraphSocketKind,
    pub(super) graph: Vec2,
    pub(super) world: Vec2,
    pub(super) interactive: bool,
}

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct MaterialGraphWire {
    pub(super) program: MaterialProgramId,
    pub(super) source: MaterialExpressionId,
    pub(super) target: MaterialConnectionTarget,
    pub(super) color: Vec4,
}

pub(crate) fn viewport_local_position(
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
    world_position: Vec2,
) -> Vec2 {
    transform.try_inverse().map_or(world_position, |inverse| {
        inverse.transform_point2(world_position) + computed.size() * 0.5
    }) * computed.inverse_scale_factor
}

pub(super) fn socket_graph_position(
    sockets: &[MaterialGraphSocketPosition],
    program: MaterialProgramId,
    kind: MaterialGraphSocketKind,
) -> Option<Vec2> {
    sockets
        .iter()
        .find(|socket| socket.program == program && socket.kind == kind)
        .map(|socket| socket.graph)
}

pub(super) fn collect_socket_positions(
    sockets: &Query<(
        &MaterialGraphSocket,
        &UiGlobalTransform,
        &MaterialGraphSocketAnchor,
    )>,
    graph_nodes: &Query<(&FeathersGraphNode, &ComputedNode, &UiGlobalTransform)>,
) -> Vec<MaterialGraphSocketPosition> {
    sockets
        .iter()
        .filter_map(|(socket, transform, anchor)| {
            let (node, computed, node_transform) = graph_nodes.get(anchor.node).ok()?;
            let (_, _, world) = transform.to_scale_angle_translation();
            let compact_offset = crate::feathers::node_graph::compact_graph_socket_offset(
                node,
                computed,
                socket.kind.side(),
            );
            let offset = compact_offset.unwrap_or_else(|| {
                anchor
                    .offset
                    .unwrap_or_else(|| viewport_local_position(computed, node_transform, world))
            });
            Some(MaterialGraphSocketPosition {
                program: socket.program,
                kind: socket.kind,
                graph: node.position() + offset,
                world,
                interactive: compact_offset.is_none(),
            })
        })
        .collect()
}

pub(crate) fn distance_to_graph_wire(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    crate::feathers::node_graph::insertion::distance_to_wire(point, start, end)
}
