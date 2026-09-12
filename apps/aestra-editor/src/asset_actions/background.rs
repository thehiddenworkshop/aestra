//! Reusable-effect extraction runs off-thread with guarded publication and rollback.
use super::*;
use crate::project_content::io::{self, IoGuard};

#[cfg(test)]
mod tests;

pub(super) enum SourceAction {
    Extract(ReusableEffectExtractionState),
}

struct SourceResult {
    action: SourceAction,
    catalog: ProjectEffectCatalog,
    session: EditorSession,
    result: Result<(), String>,
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
    io::enqueue(commands, guard.clone(), move || {
        let result = prepare(
            action,
            candidate,
            detached,
            &Localizer::new(locale).expect("supported locale"),
        );
        io::completion(move |world| apply(world, guard, result))
    });
}

fn prepare(
    action: SourceAction,
    mut catalog: ProjectEffectCatalog,
    mut session: EditorSession,
    localizer: &Localizer,
) -> SourceResult {
    catalog.refresh();
    let SourceAction::Extract(extraction) = &action;
    let result = create_reusable_effect_from_emitters(
        &extraction.emitters,
        &extraction.draft,
        extraction.replace_selection,
        &mut catalog,
        &mut session,
        localizer,
    );
    // Even failures can follow a write or failed rollback. Reconcile on the worker.
    catalog.refresh();
    if result.is_ok()
        && let Ok(compiled) = catalog.compile_project(&session.effect)
    {
        let _ = session.install_compiled_project_root(compiled.root);
    }
    SourceResult {
        action,
        catalog,
        session,
        result,
    }
}

fn apply(world: &mut World, guard: IoGuard, mut result: SourceResult) {
    let unchanged = guard.matches(
        world.resource::<ProjectEffectCatalog>(),
        world.resource::<EditorSession>(),
    );
    if !guard.same_project(world.resource::<ProjectEffectCatalog>()) {
        io::set_status(world, "project-operation-source-stale");
        return;
    }
    io::publish_catalog(world, result.catalog);
    let SourceAction::Extract(extraction) = &result.action;
    if result.result.is_ok() {
        world.resource_mut::<AssetOperationState>().extraction = None;
        if !extraction.replace_selection {
            let mut session = world.resource_mut::<EditorSession>();
            session.status = result.session.status;
            session.ui_revision += 1;
        } else if unchanged {
            let live = world.resource::<EditorSession>();
            result.session.clock = live.clock;
            result.session.playing = live.playing;
            result.session.speed = live.speed;
            result.session.ui_revision = live.ui_revision + 1;
            world.insert_resource(result.session);
            if let Some(mut timeline) = world.get_resource_mut::<TimelineState>() {
                timeline.clear_emitter_selection();
            }
        } else {
            io::set_status(world, "project-operation-extract-stale");
        }
    } else if let Err(error) = result.result {
        if let Some(extraction) = &mut world.resource_mut::<AssetOperationState>().extraction {
            extraction.error = Some(error.clone());
        }
        let mut session = world.resource_mut::<EditorSession>();
        session.status = error;
        session.ui_revision += 1;
    }
}
