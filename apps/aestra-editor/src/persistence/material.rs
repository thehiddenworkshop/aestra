//! Standalone material persistence. Never writes or replaces the active effect.
use super::*;
use crate::material_drafts::MaterialDrafts;
use crate::project_content::io::{self, IoGuard};
use aestra_core::{MaterialFunctionId, MaterialProgramId, material::*};
use std::collections::BTreeSet;

/// Save the selected program and only its transitive function dependencies.
/// In particular, an unrelated dirty program/function must not be silently committed.
fn save_scope(
    catalog: &ProjectEffectCatalog,
    id: MaterialProgramId,
) -> Result<MaterialDrafts, String> {
    fn calls(expressions: &[MaterialExpression]) -> impl Iterator<Item = MaterialFunctionId> + '_ {
        expressions
            .iter()
            .filter_map(|expression| match expression.kind {
                MaterialExpressionKind::FunctionCall {
                    function: MaterialFunctionRef::Project(id),
                    ..
                }
                | MaterialExpressionKind::CustomWeslCall { function: id, .. } => Some(id),
                _ => None,
            })
    }
    let program = catalog.material_program(id)?;
    let functions = catalog.material_functions()?;
    let mut pending = calls(&program.expressions).collect::<Vec<_>>();
    let mut dependencies = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !dependencies.insert(id) {
            continue;
        }
        let function = functions
            .iter()
            .find(|function| function.id == id)
            .ok_or_else(|| format!("Material function {id} is unavailable"))?;
        pending.extend(calls(&function.expressions));
    }
    aestra_compiler::MaterialCompiler
        .compile_with_functions(
            &program,
            &aestra_compiler::MaterialFunctionLibrary::new(functions),
        )
        .map_err(|error| error.to_string())?;
    let mut scope = catalog.material_drafts.clone();
    scope.programs.retain(|candidate, _| *candidate == id);
    scope
        .functions
        .retain(|candidate, _| dependencies.contains(candidate));
    if !scope.validate_root(catalog.root()) {
        return Err("Material save destination is outside the active project".into());
    }
    Ok(scope)
}

pub(super) fn queue_save(
    commands: &mut Commands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    reload: bool,
) {
    let Some(id) = session.standalone_material() else {
        return;
    };
    let guard = IoGuard::capture(catalog, session);
    let target = session.material_target.clone();
    let mut prepared = catalog.clone();
    io::enqueue(commands, guard.clone(), move || {
        // Refresh identity before writing: duplicate IDs introduced after browsing must block.
        prepared.refresh();
        let mut before = MaterialDrafts::default();
        let mut remaining = MaterialDrafts::default();
        let result: Result<(), String> = (|| {
            if target
                != (crate::material_document::MaterialEditingTarget::Program {
                    root: prepared.root().to_owned(),
                    id,
                })
            {
                return Err("The material belongs to another project".into());
            }
            before = save_scope(&prepared, id)?;
            remaining = before.clone();
            if !prepared.snapshot_is_current() {
                return Err("Project sources changed while preparing the save; retry".into());
            }
            remaining.save()
        })();
        let receipts = MaterialDrafts::saved_baselines(&before, &remaining);
        prepared.refresh();
        io::completion(move |world| {
            if !guard.same_document(
                world.resource::<ProjectEffectCatalog>(),
                world.resource::<EditorSession>(),
            ) {
                io::set_status(world, "project-operation-save-stale");
                return;
            }
            // Written bytes must be acknowledged even if the graph target changed. Merge only
            // receipts, preserving unrelated drafts and edits/undo that occurred during I/O.
            world
                .resource_mut::<ProjectEffectCatalog>()
                .material_drafts
                .accept_saved_baselines(&before, receipts);
            let drafts = world
                .resource::<ProjectEffectCatalog>()
                .material_drafts
                .clone();
            let success = result.is_ok();
            let localizer = world.resource::<Localizer>();
            let status = match result {
                Ok(()) => localizer.text("material-save-complete"),
                Err(error) => {
                    let mut args = FluentArgs::new();
                    args.set("error", error);
                    localizer.text_with("material-save-failed", &args)
                }
            };
            {
                let mut session = world.resource_mut::<EditorSession>();
                session.set_material_drafts(drafts);
                session.status = status;
                session.ui_revision += 1;
            }
            io::publish_catalog(world, prepared);
            if success
                && reload
                && world.resource::<EditorSession>().material_target == target
                && world.resource::<DocumentProtectionState>().pending
                    == Some(DocumentAction::ReloadMaterial)
            {
                world.resource_mut::<DocumentProtectionState>().pending = None;
                // A concurrent edit needs a fresh confirmation, not silent discard.
                world.trigger(DocumentAction::ReloadMaterial);
            }
        })
    });
}

pub(super) fn queue_reload(
    commands: &mut Commands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
) {
    let Some(id) = session.standalone_material() else {
        return;
    };
    let guard = IoGuard::capture(catalog, session);
    let target = session.material_target.clone();
    let mut prepared = catalog.clone();
    io::enqueue(commands, guard.clone(), move || {
        prepared.material_drafts.programs.remove(&id);
        prepared.refresh();
        let result: Result<(), String> = (|| {
            if target
                != (crate::material_document::MaterialEditingTarget::Program {
                    root: prepared.root().to_owned(),
                    id,
                })
            {
                return Err("The material belongs to another project".into());
            }
            prepared.material_program(id)?;
            if !prepared.snapshot_is_current() {
                return Err("Project sources changed while reloading; retry".into());
            }
            Ok(())
        })();
        io::completion(move |world| {
            if !guard.matches_material_reload(
                world.resource::<ProjectEffectCatalog>(),
                world.resource::<EditorSession>(),
            ) {
                io::set_status(world, "project-operation-open-stale");
                return;
            }
            if let Err(error) = result {
                let mut args = FluentArgs::new();
                args.set("error", error);
                let status = world
                    .resource::<Localizer>()
                    .text_with("material-reload-failed", &args);
                world.resource_mut::<EditorSession>().status = status;
                return;
            }
            // Discard only after successful fresh-source resolution; errors retain the draft.
            world
                .resource_mut::<ProjectEffectCatalog>()
                .material_drafts
                .programs
                .remove(&id);
            if let Some(mut history) =
                world.get_resource_mut::<crate::history::MaterialProgramEditHistory>()
            {
                history.clear_program(prepared.root(), id);
            }
            io::publish_catalog(world, prepared);
            let drafts = world
                .resource::<ProjectEffectCatalog>()
                .material_drafts
                .clone();
            let status = world
                .resource::<Localizer>()
                .text("material-reload-complete");
            crate::material_graph::clear_document_transients(world);
            let mut session = world.resource_mut::<EditorSession>();
            session.set_material_drafts(drafts);
            session.status = status;
            session.ui_revision += 1;
        })
    });
}

#[cfg(test)]
mod tests;
