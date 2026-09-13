//! Adapter from a validated drag snapshot to the shared bounded solver. Planning is read-only.
use super::*;
use crate::feathers::node_graph::resize;

#[derive(Clone, Debug, PartialEq)]
struct Move {
    key: String,
    stored: Option<(Vec2, bool)>,
    before: (Vec2, bool),
    after: Vec2,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Spacing {
    moves: Vec<Move>,
    // Rewiring can change bootstrap depth columns even for untouched nodes. Freeze their
    // displayed bases so only the planned conflict chain moves after the UI rebuild.
    seeds: BTreeMap<String, (Vec2, bool)>,
}

impl Spacing {
    pub(crate) fn count(&self) -> usize {
        self.moves.len()
    }

    pub(crate) fn validate(&self, graph: &str, memory: &GraphViewportMemory) -> Result<(), String> {
        if self
            .seeds
            .keys()
            .any(|key| memory.node(graph, key).is_some())
        {
            return Err("Neighbor placement changed; try again".into());
        }
        for item in &self.moves {
            if memory.node(graph, &item.key) != item.stored
                || memory.node_position(graph, &item.key) != item.stored.map(|node| node.0)
            {
                return Err("Neighbor placement changed; try again".into());
            }
        }
        Ok(())
    }

    /// Freeze bootstrap positions before history capture, without baking preview offsets.
    pub(crate) fn preserve(&self, graph: &str, memory: &mut GraphViewportMemory) {
        for (key, (position, collapsed)) in &self.seeds {
            memory.set_node(graph, key, *position, *collapsed);
        }
    }

    pub(crate) fn rollback(&self, graph: &str, memory: &mut GraphViewportMemory) {
        for key in self.seeds.keys() {
            memory.remove_node(graph, key);
        }
    }

    pub(crate) fn apply(&self, graph: &str, memory: &mut GraphViewportMemory) {
        for item in &self.moves {
            memory.set_node(graph, &item.key, item.after, item.before.1);
        }
    }
}

fn limits() -> resize::Limits {
    resize::Limits {
        spacing: Vec2::splat(24.0),
        max_nodes: 512,
        max_affected: 16,
        max_node_displacement: 1024.0,
        max_total_displacement: 4096.0,
        ..default()
    }
}

fn solve(
    snapshot: &resize::Snapshot<GraphNodeKey, GraphViewKey>,
    inserted: GraphNodeKey,
) -> Result<resize::Snapshot<GraphNodeKey, GraphViewKey>, String> {
    let result = resize::solve_insertion(snapshot, &inserted, limits()).map_err(|error| {
        match error.conflict {
            resize::Conflict::Anchors(..) => {
                "No room beside a protected node; move the node or use Alt"
            }
            resize::Conflict::Budget(_) => "Local spacing limit reached; move the node or use Alt",
            _ => "Graph geometry changed; try again",
        }
        .to_string()
    })?;
    let mut after = snapshot.clone();
    result
        .apply(&mut after)
        .map_err(|_| "Graph geometry changed; try again".to_string())?;
    Ok(after)
}

pub(super) fn capture(
    world: &World,
    gesture: &Gesture,
    inserted: GraphNodeKey,
    wire: Wire,
) -> Result<Spacing, String> {
    let memory = world
        .get_resource::<GraphViewportMemory>()
        .ok_or("Graph placement unavailable")?;
    let dragged = world
        .get::<FeathersGraphNode>(gesture.node)
        .ok_or("Dragged node disappeared")?;
    let mut nodes = BTreeMap::new();
    let mut identities = BTreeMap::new();
    let mut seeds = BTreeMap::new();
    for (entity, key) in &gesture.entities {
        let node = world
            .get::<FeathersGraphNode>(*entity)
            .ok_or("Node disappeared")?;
        let measured = gesture
            .snapshot
            .nodes
            .get(key)
            .ok_or("Graph geometry changed")?;
        if node.graph_key != dragged.graph_key {
            return Err("Graph placement identity changed".into());
        }
        let stored = memory.node(&node.graph_key, &node.node_key);
        let effective = memory
            .node_position(&node.graph_key, &node.node_key)
            .unwrap_or(node.position);
        if *key != inserted && effective.distance(measured.effective_position) > 0.5 {
            return Err("Neighbor placement changed; try again".into());
        }
        let position = if *key == inserted {
            dragged.position
        } else {
            effective
        };
        if *key != inserted && stored.is_none() {
            seeds.insert(node.node_key.clone(), (position, node.collapsed));
        }
        // Preview offsets are session-only. Do not persist them as part of an insertion.
        // Nodes left of the drop and outside the local radius are hard boundaries.
        let frozen = node.dragging
            || *key == wire.source
            || position.x < dragged.position.x
            || position.distance(dragged.position) > 1024.0
            || stored.is_some_and(|base| base.0 != effective);
        nodes.insert(
            *key,
            resize::Node {
                position,
                size: measured.size,
                frozen,
            },
        );
        identities.insert(
            *key,
            (node.node_key.clone(), stored, (position, node.collapsed)),
        );
    }
    let snapshot = resize::Snapshot {
        identity: gesture.snapshot.measured_in.clone(),
        revision: default(),
        nodes,
    };
    let after = solve(&snapshot, inserted)?;
    let moves = after
        .nodes
        .iter()
        .filter(|(key, node)| node.position != snapshot.nodes[key].position)
        .map(|(key, node)| {
            let (key, stored, before) = &identities[key];
            Move {
                key: key.clone(),
                stored: *stored,
                before: *before,
                after: node.position,
            }
        })
        .collect();
    Ok(Spacing { moves, seeds })
}

#[cfg(test)]
mod tests;
