//! Actual preview presentation recording, shared without the panel/picking host.
use crate::{
    EditorSession, ProjectEffectCatalog,
    feathers::node_graph::GraphViewportMemory,
    history::asset_order::Context,
    material_document::MaterialEditingTarget,
    material_graph::{
        MaterialGraphPreviewTarget,
        keys::material_graph_view_key,
        presentation::{Snapshot, Transaction},
    },
};
use aestra_core::MaterialProgramId;
use bevy::prelude::*;
use std::collections::BTreeSet;

pub(crate) fn record_in_context(world: &mut World, context: Context, transaction: Transaction) {
    // Fork the existing redo branch as well as the presentation branch. Layout does not execute
    // a material command, install compiled data, or change a document content revision.
    crate::history::clear_presentation_redo(world, &context);
    let mut session = world.resource_mut::<EditorSession>();
    context.select(&mut session);
    session.operation_order.record_layout(context, transaction);
    session.status = "Changed graph layout".into();
}

pub(crate) fn preview_edit(
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
