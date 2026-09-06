//! Source operations retain the existing disk preflight/rollback helpers, executed off-thread.
use super::*;
use crate::project_content::io::{self, IoGuard};
use std::path::PathBuf;

#[cfg(test)]
mod tests;

pub(super) enum SourceAction {
    Move {
        source: ProjectEffectEntryId,
        destination: PathBuf,
        current: bool,
    },
    Rename {
        rename: LibraryRenameState,
        current: bool,
    },
    InspectDeletion(ProjectEffectEntryId),
    Delete(LibraryEffectDeletionState),
    Extract(ReusableEffectExtractionState),
}

struct SourceResult {
    action: SourceAction,
    catalog: ProjectEffectCatalog,
    session: EditorSession,
    result: Result<(), String>,
    graph: Option<ProjectEffectUsageGraph>,
}

pub(super) fn queue(
    commands: &mut Commands,
    action: SourceAction,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
    localizer: &Localizer,
) {
    let guard = IoGuard::capture(catalog, session);
    let candidate = catalog.clone();
    let detached = session.fork_for_io();
    let locale = localizer.locale();
    let previous_name = session.effect.name.clone();
    io::enqueue(commands, guard.clone(), move || {
        let result = prepare(
            action,
            candidate,
            detached,
            &Localizer::new(locale).expect("supported locale"),
        );
        io::completion(move |world| apply(world, guard, previous_name, result))
    });
}

fn prepare(
    action: SourceAction,
    mut catalog: ProjectEffectCatalog,
    mut session: EditorSession,
    localizer: &Localizer,
) -> SourceResult {
    // A new owner/duplicate may have appeared since the published UI snapshot. In particular,
    // deletion confirmation must inspect the current project, not only previously known files.
    catalog.refresh();
    let mut graph = None;
    let result = (|| {
        match &action {
            SourceAction::Move {
                source,
                destination,
                current,
            } => {
                let moved = catalog
                    .move_effect_source(*source, destination)
                    .map_err(|e| e.to_string())?;
                if *current {
                    session.source_path = Some(moved.path.clone());
                }
                let mut args = FluentArgs::new();
                args.set("path", moved.path.display().to_string());
                session.status = localizer.text_with("library-status-effect-moved", &args);
            }
            SourceAction::Rename { rename, current } => {
                let renamed = catalog
                    .rename_effect_source(rename.source, &rename.draft)
                    .map_err(|e| e.to_string())?;
                if *current {
                    session.accept_external_source_rename(
                        renamed.path.clone(),
                        renamed.display_name.clone(),
                    );
                }
                let mut args = FluentArgs::new();
                args.set("name", renamed.display_name);
                session.status = localizer.text_with("library-status-effect-renamed", &args);
            }
            SourceAction::InspectDeletion(source) => {
                let reference = catalog
                    .entry(*source)
                    .and_then(|entry| entry.reference)
                    .ok_or("Source is unavailable")?;
                graph = Some(catalog.effect_usage_graph(reference)?);
            }
            SourceAction::Delete(deletion) => {
                let reference = catalog
                    .entry(deletion.source)
                    .and_then(|entry| entry.reference)
                    .ok_or("Source is unavailable")?;
                let current = catalog.effect_usage_graph(reference)?;
                if current != deletion.graph {
                    graph = Some(current);
                    return Err(localizer.text("library-delete-usages-changed"));
                }
                let entry = catalog
                    .delete_effect_source(deletion.source)
                    .map_err(|e| e.to_string())?;
                let mut args = FluentArgs::new();
                args.set("name", entry.display_name);
                session.status = localizer.text_with("library-status-effect-deleted", &args);
            }
            SourceAction::Extract(extraction) => create_reusable_effect_from_emitters(
                extraction,
                &mut catalog,
                &mut session,
                localizer,
            )?,
        }
        Ok(())
    })();
    // Even failures may follow a completed write or failed rollback. Reconcile on the worker.
    catalog.refresh();
    if result.is_ok() && matches!(action, SourceAction::Extract(_)) {
        // Extraction can introduce a project clip. Prepare its dependencies as well as the
        // source snapshot; the UI must not install a detached session with no preview.
        if let Ok(compiled) = catalog.compile_project(&session.effect) {
            let _ = session.install_compiled_project_root(compiled.root);
        }
    }
    SourceResult {
        action,
        catalog,
        session,
        result,
        graph,
    }
}

fn apply(world: &mut World, guard: IoGuard, previous_name: String, mut result: SourceResult) {
    let same_document = guard.same_document(
        world.resource::<ProjectEffectCatalog>(),
        world.resource::<EditorSession>(),
    );
    let unchanged = guard.matches(
        world.resource::<ProjectEffectCatalog>(),
        world.resource::<EditorSession>(),
    );
    if !guard.same_project(world.resource::<ProjectEffectCatalog>()) {
        io::set_status(world, "project-operation-source-stale");
        return;
    }
    io::publish_catalog(world, result.catalog);
    match &result.action {
        SourceAction::Extract(extraction)
            if result.result.is_ok() && !extraction.replace_selection =>
        {
            world
                .resource_mut::<LibraryAssetOperationState>()
                .extraction = None;
            let mut session = world.resource_mut::<EditorSession>();
            session.status = result.session.status;
            session.ui_revision += 1;
            return;
        }
        SourceAction::InspectDeletion(source) => {
            if !unchanged {
                return;
            }
            if let Some(graph) = result.graph.take() {
                let mut state = world.resource_mut::<LibraryAssetOperationState>();
                state.close_all();
                state.deletion = Some(LibraryEffectDeletionState {
                    source: *source,
                    graph,
                    error: None,
                });
                world.resource_mut::<EditorSession>().ui_revision += 1;
            }
        }
        SourceAction::Extract(_) if result.result.is_ok() && unchanged => {
            let live = world.resource::<EditorSession>();
            result.session.clock = live.clock;
            result.session.playing = live.playing;
            result.session.speed = live.speed;
            result.session.ui_revision = live.ui_revision + 1;
            world.insert_resource(result.session);
            world
                .resource_mut::<LibraryAssetOperationState>()
                .extraction = None;
            if let Some(mut timeline) = world.get_resource_mut::<TimelineState>() {
                timeline.clear_emitter_selection();
            }
            return;
        }
        SourceAction::Extract(_) if result.result.is_ok() => {
            world
                .resource_mut::<LibraryAssetOperationState>()
                .extraction = None;
            io::set_status(world, "project-operation-extract-stale");
            return;
        }
        SourceAction::Move { current: true, .. } | SourceAction::Rename { current: true, .. }
            if result.result.is_ok() && same_document =>
        {
            let mut live = world.resource_mut::<EditorSession>();
            if matches!(result.action, SourceAction::Rename { .. })
                && live.effect.name == previous_name
            {
                live.effect.name.clone_from(&result.session.effect.name);
            }
            live.accept_io_save(&result.session);
        }
        _ => {}
    }
    let mut state = world.resource_mut::<LibraryAssetOperationState>();
    match result.result {
        Ok(()) => {
            match result.action {
                SourceAction::Rename { .. } => state.rename = None,
                SourceAction::Delete(_) => state.close_all(),
                _ => {}
            }
            world.resource_mut::<EditorSession>().status = result.session.status;
        }
        Err(error) => {
            match result.action {
                SourceAction::Rename { .. } => {
                    if let Some(rename) = &mut state.rename {
                        rename.error = Some(error.clone());
                    }
                }
                SourceAction::Delete(_) => {
                    if let Some(deletion) = &mut state.deletion {
                        deletion.error = Some(error.clone());
                        if let Some(graph) = result.graph {
                            deletion.graph = graph;
                        }
                    }
                }
                SourceAction::Extract(_) => {
                    if let Some(extraction) = &mut state.extraction {
                        extraction.error = Some(error.clone());
                    }
                }
                _ => {}
            }
            world.resource_mut::<EditorSession>().status = error;
        }
    }
    world.resource_mut::<EditorSession>().ui_revision += 1;
}
