//! Guarded filesystem history execution, independent of browser/dialog composition.
use super::{
    documents::ClosedDocuments,
    reference_checks::{active_block, deletion_draft_inventory},
};
use crate::{
    ProjectEffectCatalog,
    persistence::DocumentProtectionState,
    project_content::io::{self, IoGuard},
    session::EditorSession,
};
use aestra_project::content::operations::{DeletePlan, DeletedSource};
use bevy::prelude::*;
use std::path::Path;

/// UI adapters run around actual catalog publication, after successful guarded I/O.
#[derive(Clone, Copy)]
pub(super) struct Publication {
    pub(super) reconcile: fn(&mut World, &ProjectEffectCatalog, &Path, bool),
    pub(super) finish: fn(&mut World, &Path, bool),
}

pub(super) fn publish(
    world: &mut World,
    mut prepared: ProjectEffectCatalog,
    relative: &Path,
    deleting: bool,
    publication: Publication,
) {
    prepared.refresh();
    (publication.reconcile)(world, &prepared, relative, deleting);
    io::publish_catalog(world, prepared);
    (publication.finish)(world, relative, deleting);
}

/// Undo/Redo keeps its stack entry until guarded, serialized publication succeeds.
pub(super) fn step(
    world: &mut World,
    item: DeletedSource,
    undo: bool,
    revision: u64,
    publication: Publication,
) {
    use crate::history::asset_order::AssetOrder;
    if !io::idle_world(world) {
        return;
    }
    // Callers queue this after the observer; compare ordering again before capturing I/O.
    world.resource_scope(|world, mut order: Mut<AssetOrder>| {
        order.sync(
            world.resource::<EditorSession>(),
            world.resource::<ProjectEffectCatalog>(),
        );
    });
    if !world
        .resource::<AssetOrder>()
        .matches_delete(undo, revision)
    {
        return;
    }
    if world.resource::<DocumentProtectionState>().is_open() {
        return;
    }
    let catalog = world.resource::<ProjectEffectCatalog>();
    let session = world.resource::<EditorSession>();
    let closed = match ClosedDocuments::read(&item).and_then(|closed| {
        if undo {
            closed.validate_restore(catalog, &item.original)?;
        } else {
            let source = catalog
                .content()
                .source_tree()
                .at_relative_path(&item.original)
                .ok_or("Deleted source disappeared")?
                .id;
            let current = ClosedDocuments::capture(catalog, session, source)?;
            closed.validate_redo(&current)?;
            return Ok(current);
        }
        Ok(closed)
    }) {
        Ok(closed) => closed,
        Err(error) => {
            world.resource_mut::<EditorSession>().status = error;
            return;
        }
    };
    let for_checks = if undo {
        catalog.clone()
    } else {
        closed.without_drafts(catalog)
    };
    let (drafts, complete) = deletion_draft_inventory(&for_checks, session);
    if !complete {
        world.resource_mut::<EditorSession>().status =
            "Asset Undo/Redo blocked: draft inventory is incomplete".into();
        return;
    }
    if !undo
        && catalog
            .content()
            .source_tree()
            .at_relative_path(&item.original)
            .is_none_or(|source| active_block(catalog, session, source.id))
    {
        world.resource_mut::<EditorSession>().status =
            "Redo deletion blocked: source is missing or referenced by the active document".into();
        return;
    }
    let prepared = catalog.clone();
    let guard = IoGuard::capture(catalog, session);
    io::enqueue(&mut world.commands(), guard.clone(), move || {
        let planned: Result<Option<DeletePlan>, String> = if undo {
            Ok(None)
        } else {
            item.plan_redo(&drafts, complete)
                .map(Some)
                .map_err(|error| error.to_string())
        };
        io::completion(move |world| {
            if !guard.matches(
                world.resource::<ProjectEffectCatalog>(),
                world.resource::<EditorSession>(),
            ) || !world
                .resource::<AssetOrder>()
                .matches_delete(undo, revision)
                || world.resource::<DocumentProtectionState>().is_open()
            {
                world.resource_mut::<EditorSession>().status =
                    "Asset Undo/Redo cancelled: project or document changed".into();
                return;
            }
            let result = planned.and_then(|plan| match plan {
                Some(plan) => plan.apply().map_err(|e| e.to_string()),
                None => item
                    .clone()
                    .restore(&drafts, complete)
                    .map(|_| item)
                    .map_err(|e| e.to_string()),
            });
            match result {
                Ok(item) => {
                    let relative = item.original.clone();
                    world
                        .resource_mut::<AssetOrder>()
                        .finish_delete(undo, revision, item);
                    if undo {
                        closed.restore(world);
                    } else {
                        closed.close(world);
                    }
                    publish(world, prepared, &relative, !undo, publication);
                }
                Err(error) => {
                    world.resource_mut::<EditorSession>().status =
                        format!("Asset Undo/Redo blocked: {error}");
                }
            }
        })
    });
}

#[cfg(test)]
#[path = "../tests/deletion_history.rs"]
mod tests;
