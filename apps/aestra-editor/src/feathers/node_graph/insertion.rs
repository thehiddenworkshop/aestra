//! Shared drag-on-wire hit testing. Adapters validate and commit semantics.
use super::{geometry::*, *};
use std::collections::BTreeMap;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Wire {
    pub source: GraphNodeKey,
    pub target: GraphNodeKey,
    pub port: GraphGeometryPort,
}

impl Wire {
    pub(crate) fn material(
        source: aestra_core::MaterialExpressionId,
        target: aestra_authoring::MaterialConnectionTarget,
    ) -> Self {
        use aestra_authoring::MaterialConnectionTarget::*;
        Self {
            source: GraphNodeKey::Expression(source),
            target: match target {
                ExpressionInput { expression, .. } => GraphNodeKey::Expression(expression),
                ProgramOutput(_) => GraphNodeKey::MaterialOutputs,
            },
            port: target.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub view: GraphViewKey,
    pub node: GraphNodeKey,
    pub wire: Wire,
    pub entity: Entity,
    pub inputs: Vec<aestra_authoring::MaterialExpressionInput>,
}

#[derive(Event)]
pub(crate) struct Probe(pub Option<Candidate>);
#[derive(Event)]
pub(crate) struct Drop {
    pub candidate: Candidate,
    pub edit: GraphPresentationEdit,
    pub before_offset: Vec2,
}

struct Gesture {
    node: Entity,
    viewport: Entity,
    snapshot: GraphGeometrySnapshot,
    entities: BTreeMap<Entity, GraphNodeKey>,
}

#[derive(Resource, Default)]
pub(crate) struct State {
    gesture: Option<Gesture>,
    candidate: Option<Candidate>,
    pub allowed: bool,
}

impl State {
    pub(crate) fn color(&self, wire: Entity) -> Option<Vec4> {
        self.candidate
            .as_ref()
            .filter(|c| c.entity == wire)
            .map(|_| {
                if self.allowed {
                    Vec4::new(0.3, 1.0, 0.5, 1.0)
                } else {
                    Vec4::new(1.0, 0.3, 0.25, 1.0)
                }
            })
    }
}

fn belongs(world: &World, mut entity: Entity, viewport: Entity) -> bool {
    while let Some(parent) = world.get::<ChildOf>(entity) {
        entity = parent.parent();
        if entity == viewport {
            return true;
        }
    }
    false
}

pub(super) fn begin(world: &mut World, node: Entity) {
    if !world.contains_resource::<State>() {
        return;
    }
    *world.resource_mut::<State>() = default();
    let mut ancestor = node;
    let viewport = loop {
        let Some(parent) = world.get::<ChildOf>(ancestor) else {
            return;
        };
        ancestor = parent.parent();
        if world.get::<GraphGeometryView>(ancestor).is_some() {
            break ancestor;
        }
    };
    let key = &world.get::<GraphGeometryView>(viewport).unwrap().key;
    let Some(snapshot) = world
        .get_resource::<GraphGeometryRegistry>()
        .and_then(|registry| registry.mounted_view_snapshot(key, viewport))
        .cloned()
        .filter(|s| s.nodes.len() <= 512)
    else {
        return;
    };
    let entities = world
        .query::<(Entity, &GraphGeometryNode)>()
        .iter(world)
        .filter(|(entity, _)| belongs(world, *entity, viewport))
        .map(|(entity, meta)| (entity, meta.key))
        .collect();
    world.resource_mut::<State>().gesture = Some(Gesture {
        node,
        viewport,
        snapshot,
        entities,
    });
}

fn candidate(world: &mut World, entity: Entity) -> Option<Candidate> {
    if world
        .get_resource::<ButtonInput<KeyCode>>()
        .is_some_and(|keys| {
            keys.pressed(KeyCode::AltLeft)
                || keys.pressed(KeyCode::AltRight)
                || keys.pressed(KeyCode::Escape)
        })
    {
        return None;
    }
    if world.get_resource::<State>()?.gesture.as_ref()?.node != entity {
        return None;
    }
    let mut wires = world
        .query::<(Entity, &Wire)>()
        .iter(world)
        .map(|(entity, wire)| (entity, *wire))
        .collect::<Vec<_>>();
    let gesture = world.get_resource::<State>()?.gesture.as_ref()?;
    if gesture.node != entity {
        return None;
    }
    let meta = world.get::<GraphGeometryView>(gesture.viewport)?;
    if meta.key != gesture.snapshot.measured_in || meta.nodes.len() != gesture.entities.len() {
        return None;
    }
    for (id, key) in &gesture.entities {
        let node = world.get::<FeathersGraphNode>(*id)?;
        let meta = world.get::<GraphGeometryNode>(*id)?;
        let measured = gesture.snapshot.nodes.get(key)?;
        let computed = world.get::<ComputedNode>(*id)?;
        if !belongs(world, *id, gesture.viewport)
            || !node.position.is_finite()
            || !computed.size().is_finite()
            || !computed.inverse_scale_factor.is_finite()
            || meta.key != *key
            || meta.content_stamp() != measured.content
            || meta.is_preview() != measured.preview
            || node.collapsed != measured.collapsed
            || (computed.size() * computed.inverse_scale_factor - measured.size)
                .abs()
                .max_element()
                > 0.5
            || (*id != entity && node.position.distance(measured.effective_position) > 0.5)
        {
            return None;
        }
    }
    let key = *gesture.entities.get(&entity)?;
    let node = world.get::<FeathersGraphNode>(entity)?;
    let shape = gesture.snapshot.nodes.get(&key)?;
    let view = world.get::<FeathersGraphViewport>(gesture.viewport)?;
    let computed = world.get::<ComputedNode>(gesture.viewport)?;
    let center = view.project_graph_point(node.position + shape.size * 0.5);
    if !center.is_finite()
        || !Rect::from_corners(Vec2::ZERO, computed.size() * computed.inverse_scale_factor)
            .contains(center)
    {
        return None;
    }
    wires.retain(|(id, wire)| {
        belongs(world, *id, gesture.viewport) && wire.source != key && wire.target != key
    });
    if wires.len() > 512 {
        return None;
    }
    let port_position = |node, port| {
        let shape = gesture.snapshot.nodes.get(&node)?;
        let offset = shape.ports.iter().find(|p| p.key == port)?.offset;
        Some(view.project_graph_point(shape.effective_position + offset))
    };
    let (wire_entity, wire, _) = wires
        .into_iter()
        .filter_map(|(id, wire)| {
            let start = port_position(wire.source, GraphGeometryPort::Output)?;
            let end = port_position(wire.target, wire.port)?;
            let distance = distance_to_wire(center, start, end);
            (distance <= 14.0 && center.distance(start) > 18.0 && center.distance(end) > 18.0)
                .then_some((id, wire, distance))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2).then_with(|| a.0.cmp(&b.0)))?;
    Some(Candidate {
        view: meta.key.clone(),
        node: key,
        wire,
        entity: wire_entity,
        inputs: shape
            .ports
            .iter()
            .filter_map(|p| match p.key {
                GraphGeometryPort::Input(input) => Some(input),
                _ => None,
            })
            .collect(),
    })
}

pub(super) fn motion(world: &mut World, entity: Entity) {
    if !world.contains_resource::<State>() {
        return;
    }
    let next = candidate(world, entity);
    if world.resource::<State>().candidate != next {
        let mut state = world.resource_mut::<State>();
        state.candidate = next.clone();
        state.allowed = false;
        world.trigger(Probe(next));
    }
}

pub(crate) fn refresh(world: &mut World) {
    let node = world
        .get_resource::<State>()
        .and_then(|s| s.gesture.as_ref().map(|g| g.node));
    if let Some(node) = node {
        if world
            .get::<FeathersGraphNode>(node)
            .is_none_or(|n| !n.dragging)
        {
            *world.resource_mut::<State>() = default();
        } else {
            motion(world, node);
        }
    }
}

pub(super) fn finish(world: &mut World, entity: Entity, edit: Option<GraphPresentationEdit>) {
    let target = candidate(world, entity);
    let before_offset = edit
        .as_ref()
        .and_then(|edit| {
            let gesture = world.get_resource::<State>()?.gesture.as_ref()?;
            let key = gesture.entities.get(&entity)?;
            Some(gesture.snapshot.nodes.get(key)?.effective_position - edit.before.0)
        })
        .unwrap_or(Vec2::ZERO);
    if let Some(mut state) = world.get_resource_mut::<State>() {
        *state = default();
    }
    if let Some(edit) = edit {
        if let Some(candidate) = target {
            world.trigger(Drop {
                candidate,
                edit,
                before_offset,
            });
        } else {
            world.trigger(edit);
        }
    }
}

/// Shared cubic convention, identical to rendered wires; distances are logical viewport units.
pub(crate) fn distance_to_wire(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let control = (end.x - start.x).abs().mul_add(0.5, 54.0).min(220.0);
    let a = start + Vec2::new(control, 0.0);
    let b = end - Vec2::new(control, 0.0);
    let mut distance = f32::INFINITY;
    let mut previous = start;
    for index in 1..=32 {
        let t = index as f32 / 32.0;
        let s = 1.0 - t;
        let next = start * s.powi(3)
            + a * (3.0 * s.powi(2) * t)
            + b * (3.0 * s * t.powi(2))
            + end * t.powi(3);
        let segment = next - previous;
        let fraction = if segment.length_squared() <= f32::EPSILON {
            0.0
        } else {
            ((point - previous).dot(segment) / segment.length_squared()).clamp(0.0, 1.0)
        };
        distance = distance.min(point.distance(previous + fraction * segment));
        previous = next;
    }
    distance
}

#[cfg(test)]
mod tests;
