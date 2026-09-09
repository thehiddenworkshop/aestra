//! Explicit recovery of journaled relocations. Inspection is read-only; publication
//! uses the serialized project queue and rechecks document guards on the main thread.
use crate::project_content::io::{self, IoGuard};
use crate::*;
use aestra_project::content::operations::PendingAssetMoveBatch;
use bevy::ui_widgets::Activate;

mod ui;
pub(crate) use ui::spawn;
pub(super) use ui::spawn_reopen_button;
#[cfg(test)]
mod tests;

#[derive(Resource, Default)]
struct RecoveryState {
    generation: Option<u64>,
    pending: Option<PendingAssetMoveBatch>,
    error: Option<String>,
    available: bool,
    open: bool,
    requested: bool,
    busy: bool,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Open,
    Restore,
    Retry,
    Later,
}

#[derive(Event)]
pub(super) struct CheckRecovery;

pub(super) fn register(app: &mut App) {
    app.init_resource::<RecoveryState>()
        .init_resource::<DocumentProtectionState>()
        .add_observer(activate)
        .add_observer(|_: On<CheckRecovery>, mut state: ResMut<RecoveryState>| {
            state.requested = true;
        })
        .add_systems(Update, (inspect, ui::sync).chain());
}

fn drafts_clear(catalog: &ProjectEffectCatalog, session: &EditorSession) -> bool {
    let (drafts, complete) = super::inspection::draft_inventory(catalog, session);
    complete && drafts.is_empty()
}

fn inspect(
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    mut state: ResMut<RecoveryState>,
    mut protection: ResMut<DocumentProtectionState>,
    tasks: Option<Res<io::ProjectIoTasks>>,
    mut commands: Commands,
) {
    let generation = catalog.content_revision().generation;
    if state.generation != Some(generation) {
        *state = RecoveryState {
            generation: Some(generation),
            requested: true,
            ..default()
        };
        protection.asset_recovery_open = false;
    }
    if !state.requested || state.busy || !io::idle(tasks) || protection.is_open() && !state.open {
        return;
    }
    state.requested = false;
    if !journal_present(catalog.root()) {
        state.pending = None;
        state.available = false;
        state.open = false;
        state.error = None;
        protection.asset_recovery_open = false;
        return;
    }
    state.busy = true;
    let prepared = catalog.clone();
    let guard = IoGuard::capture(&catalog, &session);
    let status = session.status.clone();
    io::enqueue(&mut commands, guard.clone(), move || {
        let result = prepared.content().pending_asset_move_batch();
        io::completion(move |world| {
            if !guard.same_project(world.resource::<ProjectEffectCatalog>()) {
                return;
            }
            let mut state = world.resource_mut::<RecoveryState>();
            state.busy = false;
            state.pending = None;
            state.error = None;
            match result {
                Ok(pending) => {
                    state.available = pending.is_some();
                    state.pending = pending;
                }
                Err(error) => {
                    state.available = true;
                    state.error = Some(error.to_string());
                }
            }
            // A document dialog may have opened while read-only inspection ran.
            // Leave the Recovery button available rather than stacking modals.
            let open = state.available;
            if !open || !world.resource::<DocumentProtectionState>().is_open() {
                world.resource_mut::<RecoveryState>().open = open;
                world
                    .resource_mut::<DocumentProtectionState>()
                    .asset_recovery_open = open;
            }
            let running = world
                .resource::<Localizer>()
                .text("project-operation-running");
            if world.resource::<EditorSession>().status == running {
                world.resource_mut::<EditorSession>().status = status;
            }
        })
    });
}

#[allow(clippy::too_many_arguments)]
fn activate(
    event: On<Activate>,
    choices: Query<&Choice>,
    mut state: ResMut<RecoveryState>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    mut protection: ResMut<DocumentProtectionState>,
    tasks: Option<Res<io::ProjectIoTasks>>,
    mut commands: Commands,
) {
    let Ok(choice) = choices.get(event.entity) else {
        return;
    };
    if state.busy || !io::idle(tasks) {
        return;
    }
    match choice {
        Choice::Open if !protection.is_open() => {
            state.open = state.available;
            protection.asset_recovery_open = state.open;
        }
        Choice::Later => {
            state.open = false;
            protection.asset_recovery_open = false;
        }
        Choice::Retry if state.open => state.requested = true,
        Choice::Restore if state.open && drafts_clear(&catalog, &session) => {
            let Some(pending) = state.pending.clone() else {
                return;
            };
            state.busy = true;
            let guard = IoGuard::capture(&catalog, &session);
            // Retain the exact inspected journal, not a newly discovered transaction
            // at the same filename. rollback verifies both its contents and every path.
            io::enqueue(&mut commands, guard.clone(), move || {
                io::completion(move |world| restore(world, guard, pending))
            });
        }
        _ => {}
    }
}

fn restore(world: &mut World, guard: IoGuard, pending: PendingAssetMoveBatch) {
    if !guard.same_project(world.resource::<ProjectEffectCatalog>()) {
        return;
    }
    world.resource_mut::<RecoveryState>().busy = false;
    if !world.resource::<RecoveryState>().open
        || !guard.matches_material_reload(
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
        )
        || !drafts_clear(
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
        )
    {
        let message = world
            .resource::<Localizer>()
            .text("browser-recovery-changed");
        world.resource_mut::<RecoveryState>().error = Some(message);
        return;
    }
    let (drafts, complete) = super::inspection::draft_inventory(
        world.resource::<ProjectEffectCatalog>(),
        world.resource::<EditorSession>(),
    );
    match pending.rollback(&drafts, complete) {
        Err(error) => {
            let mut state = world.resource_mut::<RecoveryState>();
            state.pending = None;
            state.error = Some(error.to_string());
        }
        Ok(journal) => {
            let mut catalog = world.resource::<ProjectEffectCatalog>().clone();
            catalog.refresh();
            let result = reconcile_document(&catalog, &mut world.resource_mut::<EditorSession>());
            io::publish_catalog(world, catalog);
            let mut args = fluent_bundle::FluentArgs::new();
            args.set("path", journal.display().to_string());
            let message = world
                .resource::<Localizer>()
                .text_with("browser-recovery-restored", &args);
            let mut state = world.resource_mut::<RecoveryState>();
            state.pending = None;
            state.available = false;
            state.open = false;
            state.error = None;
            world
                .resource_mut::<DocumentProtectionState>()
                .asset_recovery_open = false;
            world.resource_mut::<EditorSession>().status = match result {
                Ok(()) => message,
                Err(error) => format!("{message}\n{error}"),
            };
        }
    }
}

/// Material/function targets are ID-based and read the refreshed catalog. Effects
/// additionally retain a path and exact-byte save baseline, both of which must agree
/// with the restored source. Never resolve an ambiguous ID or adopt an unrelated file.
fn reconcile_document(
    catalog: &ProjectEffectCatalog,
    session: &mut EditorSession,
) -> Result<(), String> {
    let Some(source) = &session.source_path else {
        return Ok(());
    };
    // A same-ID document opened outside this project is not owned by recovery.
    if !inside_project(source, catalog.root()) {
        return Ok(());
    }
    let path = catalog.openable_path(session.effect.id.into())
        .ok_or("Locations restored, but the open effect is unavailable or ambiguous. Reopen it before saving.")?;
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let effect =
        EffectAsset::from_ron(std::str::from_utf8(&bytes).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    if effect.id != session.effect.id {
        return Err("Restored effect identity changed; reopen it before saving.".into());
    }
    if effect == session.effect {
        session.accept_restored_source(path.to_owned(), bytes);
    } else {
        let compiled = catalog.compile_project(&effect)
            .map_err(|error| format!("Locations restored, but the open effect could not reload: {error}. Reopen it before saving."))?;
        let target = session.material_target.clone();
        let material_history_active = session.material_history_active;
        session.open_refreshed_effect(path, effect, compiled.root, bytes);
        session.material_target = target;
        session.material_history_active = material_history_active;
    }
    Ok(())
}

pub(super) fn journal_present(root: &std::path::Path) -> bool {
    !matches!(std::fs::symlink_metadata(root.join(".aestra/asset-transactions/active.pending")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

fn inside_project(path: &std::path::Path, root: &std::path::Path) -> bool {
    if path
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return false;
    }
    fn normalized(path: &std::path::Path) -> String {
        let path = path.to_string_lossy().replace('\\', "/");
        let path = path.strip_prefix("//?/").unwrap_or(&path);
        if cfg!(windows) {
            path.to_lowercase()
        } else {
            path.to_owned()
        }
    }
    normalized(path).starts_with(&format!("{}/", normalized(root).trim_end_matches('/')))
}
