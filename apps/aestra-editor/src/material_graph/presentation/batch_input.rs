//! Actual graph presentation batch history and context resolution.
use crate::feathers::node_graph::{GraphPresentationBatchEdit, GraphViewportMemory};
use crate::material_graph::{
    keys::{function_graph_memory_key, material_graph_view_key},
    presentation::{Snapshot, Transaction, preview::record_in_context},
};
use crate::{EditorSession, ProjectEffectCatalog};
use crate::{history::asset_order::Context, material_document::MaterialEditingTarget};
use bevy::prelude::*;

pub(super) fn context(
    graph: &str,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Context {
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

pub(super) fn context_for_origin(world: &World, mut origin: Entity) -> Option<Context> {
    loop {
        if let Some(marker) =
            world.get::<crate::material_graph::asset_drop::GraphDropTarget>(origin)
        {
            return Some(match marker.editing_target() {
                MaterialEditingTarget::EffectInstance => Context::Effect,
                target => Context::Material(target.clone()),
            });
        }
        origin = world.get::<ChildOf>(origin)?.parent();
    }
}

pub(super) fn record(world: &mut World, transaction: Transaction, origin: Option<Entity>) {
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

pub(crate) fn batch_edit(event: On<GraphPresentationBatchEdit>, mut commands: Commands) {
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
