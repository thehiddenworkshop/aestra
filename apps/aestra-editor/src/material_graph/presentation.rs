//! Presentation deltas share the existing document chronology, but never shader state.
use super::*;
use crate::feathers::node_graph::{
    GraphPinEdit, GraphPresentationBatchEdit, GraphPresentationEdit,
};
use crate::history::asset_order::Context;
use crate::material_document::MaterialEditingTarget;

mod semantic;
mod transaction;
pub(crate) use transaction::{Snapshot, Transaction};

mod state;
pub(crate) use state::State;
mod context;
pub(super) use context::visible;

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

pub(super) fn pin_edit(event: On<GraphPinEdit>, mut commands: Commands) {
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

fn context(graph: &str, catalog: &ProjectEffectCatalog, session: &EditorSession) -> Context {
    if let Some(id) = session.standalone_function()
        && let Ok(functions) = catalog.material_functions()
        && let Some(function) = functions
            .iter()
            .find(|f| f.id == id && function_graph_memory_key(catalog.root(), f.id) == graph)
    {
        return Context::Material(MaterialEditingTarget::Function {
            root: catalog.root().to_owned(),
            id: function.id,
        });
    }
    if let Some(id) = session.standalone_material()
        && material_graph_view_key(id) == graph
        && catalog.material_program(id).is_ok()
    {
        Context::Material(MaterialEditingTarget::Program {
            root: catalog.root().to_owned(),
            id,
        })
    } else {
        Context::Effect
    }
}

fn context_for_origin(world: &World, mut origin: Entity) -> Option<Context> {
    loop {
        if let Some(marker) = world.get::<super::asset_drop::GraphDropTarget>(origin) {
            return Some(match marker.editing_target() {
                MaterialEditingTarget::EffectInstance => Context::Effect,
                target => Context::Material(target.clone()),
            });
        }
        origin = world.get::<ChildOf>(origin)?.parent();
    }
}

fn record(world: &mut World, transaction: Transaction, origin: Option<Entity>) {
    let context = origin
        .and_then(|entity| context_for_origin(world, entity))
        .unwrap_or_else(|| {
            context(
                &transaction.before.graph,
                world.resource::<ProjectEffectCatalog>(),
                world.resource::<EditorSession>(),
            )
        });
    record_in_context(world, context, transaction);
}

fn record_in_context(world: &mut World, context: Context, transaction: Transaction) {
    // Fork the existing redo branch as well as the presentation branch. Layout does not execute
    // a material command, install compiled data, or change a document content revision.
    crate::history::clear_presentation_redo(world, &context);
    let mut session = world.resource_mut::<EditorSession>();
    context.select(&mut session);
    session.operation_order.record_layout(context, transaction);
    session.status = "Changed graph layout".into();
}

pub(super) fn node_edit(event: On<GraphPresentationEdit>, mut commands: Commands) {
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

pub(super) fn batch_edit(event: On<GraphPresentationBatchEdit>, mut commands: Commands) {
    let edit = event.event().clone();
    commands.queue(move |world: &mut World| {
        let Some(after) = Snapshot::capture(
            &edit.graph,
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
            world.resource::<GraphViewportMemory>(),
        ) else {
            return;
        };
        let mut before = after.clone();
        for (node, state) in edit.before {
            if before.keys.contains(&node) {
                before.nodes.insert(node, state);
            }
        }
        if before.nodes != after.nodes {
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
        }
    });
}

pub(super) fn preview_edit(
    commands: &mut Commands,
    program: MaterialProgramId,
    editing_target: MaterialEditingTarget,
    before: BTreeSet<MaterialGraphPreviewTarget>,
    after: BTreeSet<MaterialGraphPreviewTarget>,
) {
    if before == after {
        return;
    }
    commands.queue(move |world: &mut World| {
        let Some(snapshot) = Snapshot::capture(
            &material_graph_view_key(program),
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
            world.resource::<GraphViewportMemory>(),
        ) else {
            return;
        };
        record_in_context(
            world,
            match editing_target {
                MaterialEditingTarget::EffectInstance => Context::Effect,
                target => Context::Material(target),
            },
            Transaction {
                before: snapshot.clone(),
                after: snapshot,
                previews: Some((program, before, after)),
                invalidated: false,
            },
        );
    });
}

#[cfg(test)]
mod tests;
