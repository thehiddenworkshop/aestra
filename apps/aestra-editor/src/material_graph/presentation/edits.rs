//! Production presentation edit consumers and atomic arrangement installation.
use crate::feathers::node_graph::{GraphPinEdit, GraphPresentationEdit, GraphViewportMemory};
use crate::history::asset_order::Context;
use crate::material_document::MaterialEditingTarget;
use crate::material_graph::presentation::{
    Snapshot, Transaction,
    batch_input::{batch_edit, record},
    preview::record_in_context,
};
use crate::{EditorSession, ProjectEffectCatalog};
use bevy::prelude::*;
use std::collections::BTreeMap;

pub(crate) fn register(app: &mut App) {
    app.add_observer(node_edit)
        .add_observer(batch_edit)
        .add_observer(pin_edit);
}

/// Atomically installs a validated asynchronous arrangement and records one presentation-only
/// history entry. Any semantic or manual-placement change since dispatch rejects the result.
pub(crate) fn arrange(
    world: &mut World,
    before: Snapshot,
    expected_placement_revision: u64,
    positions: BTreeMap<String, Vec2>,
    editing_target: MaterialEditingTarget,
) -> Result<(), String> {
    if !before.is_current(
        world.resource::<ProjectEffectCatalog>(),
        world.resource::<EditorSession>(),
    ) {
        return Err("Arrange result is stale: the graph changed while layout was running".into());
    }
    if positions.keys().ne(before.keys.iter())
        || positions.values().any(|position| !position.is_finite())
    {
        return Err("Arrange result does not match the current graph".into());
    }
    let graph = before.graph.clone();
    if world
        .resource::<GraphViewportMemory>()
        .placement_revision(&graph)
        != expected_placement_revision
    {
        return Err("Arrange result was ignored because node placement changed".into());
    }

    {
        let mut memory = world.resource_mut::<GraphViewportMemory>();
        for (node, position) in positions {
            let collapsed = memory
                .node(&graph, &node)
                .map(|(_, collapsed)| collapsed)
                .unwrap_or(false);
            memory.set_node(&graph, node, position, collapsed);
        }
        memory.clear_offsets(&graph);
    }
    let after = Snapshot::capture(
        &graph,
        world.resource::<ProjectEffectCatalog>(),
        world.resource::<EditorSession>(),
        world.resource::<GraphViewportMemory>(),
    )
    .ok_or("Arrange result became stale before it could be applied")?;
    if before.nodes != after.nodes || before.pinned != after.pinned {
        record_in_context(
            world,
            match editing_target {
                MaterialEditingTarget::EffectInstance => Context::Effect,
                target => Context::Material(target),
            },
            Transaction {
                before,
                after,
                previews: None,
                invalidated: false,
            },
        );
    }
    Ok(())
}

pub(crate) fn pin_edit(event: On<GraphPinEdit>, mut commands: Commands) {
    let edit = event.event().clone();
    commands.queue(move |world: &mut World| {
        let Some(mut after) = Snapshot::capture(
            &edit.graph,
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
            world.resource::<GraphViewportMemory>(),
        ) else {
            return;
        };
        if !after.keys.contains(&edit.node) || edit.before == edit.after {
            return;
        }
        if edit.after {
            after.pinned.insert(edit.node.clone());
        } else {
            after.pinned.remove(&edit.node);
        }
        let mut before = after.clone();
        if edit.before {
            before.pinned.insert(edit.node);
        } else {
            before.pinned.remove(&edit.node);
        }
        record(
            world,
            Transaction {
                before,
                after,
                previews: None,
                invalidated: false,
            },
            edit.origin,
        );
    });
}

pub(crate) fn node_edit(event: On<GraphPresentationEdit>, mut commands: Commands) {
    let edit = event.event().clone();
    commands.queue(move |world: &mut World| {
        let Some(mut after) = Snapshot::capture(
            &edit.graph,
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
            world.resource::<GraphViewportMemory>(),
        ) else {
            return;
        };
        if !after.keys.contains(&edit.node) || edit.before == edit.after {
            return;
        }
        after.nodes.insert(edit.node.clone(), edit.after);
        let mut before = after.clone();
        before.nodes.insert(edit.node, edit.before);
        record(
            world,
            Transaction {
                before,
                after,
                previews: None,
                invalidated: false,
            },
            edit.origin,
        );
    });
}

#[cfg(test)]
#[path = "edits/tests.rs"]
mod tests;
