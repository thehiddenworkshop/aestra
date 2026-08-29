//! Editor document I/O, recovery, autosave, and application-exit lifecycle.

use crate::recovery::{RecoveryCandidate, RecoveryPersistence};
use crate::*;
use aestra_bevy::{EffectAssetLoad, EffectAssetMigration, EffectCompiler, prepare_effect_asset};
use bevy::ui_widgets::Activate;
use fluent_bundle::FluentArgs;
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const RECOVERY_CLEANUP_RETRY_DELAY: Duration = Duration::from_secs(1);

pub(crate) struct EditorPersistencePlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PersistenceSet {
    Startup,
    Actions,
    Lifecycle,
}

impl Plugin for EditorPersistencePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(queue_document_action_activation)
            .add_observer(execute_document_action)
            .add_systems(
                Startup,
                initialize_document_persistence.in_set(PersistenceSet::Startup),
            )
            .add_systems(
                Update,
                handle_document_action_buttons.in_set(PersistenceSet::Actions),
            )
            .add_systems(
                Update,
                (handle_window_close_requests, autosave_recovery)
                    .chain()
                    .in_set(PersistenceSet::Lifecycle),
            );
    }
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentAction {
    New,
    Open,
    OpenCatalog(ProjectEffectEntryId),
    Save,
    SaveAs,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PersistenceStatus {
    CreatedUntitled,
    NewCancelled,
    Opened(String),
    OpenCancelled,
    OpenFailed(String),
    MigrationCancelled,
    Migrated {
        path: String,
        backup: String,
        from: u32,
        to: u32,
    },
    Saved(String),
    SaveCancelled,
    SaveFailed(String),
    ExitCancelled,
    RecoveryRestored(String),
    RecoveryDiscarded,
    RecoveryDiscardFailed(String),
    RecoveryAutosaveFailed(String),
    RecoveryDiagnostic(String),
    SettingsSaved,
    SettingsSaveFailed(String),
    SettingsDiagnostic(String),
}

fn set_persistence_status(
    session: &mut EditorSession,
    localizer: &Localizer,
    status: PersistenceStatus,
) {
    session.status = localize_persistence_status(status, localizer);
}

fn localize_persistence_status(status: PersistenceStatus, localizer: &Localizer) -> String {
    if let PersistenceStatus::Migrated {
        path,
        backup,
        from,
        to,
    } = status
    {
        let mut args = FluentArgs::new();
        args.set("path", path);
        args.set("backup", backup);
        args.set("from", i64::from(from));
        args.set("to", i64::from(to));
        return localizer.text_with("persistence-status-migrated", &args);
    }
    let (message_id, argument) = match status {
        PersistenceStatus::CreatedUntitled => {
            return localizer.text("persistence-status-created-untitled");
        }
        PersistenceStatus::NewCancelled => {
            return localizer.text("persistence-status-new-cancelled");
        }
        PersistenceStatus::Opened(path) => ("persistence-status-opened", ("path", path)),
        PersistenceStatus::OpenCancelled => {
            return localizer.text("persistence-status-open-cancelled");
        }
        PersistenceStatus::OpenFailed(error) => {
            ("persistence-status-open-failed", ("error", error))
        }
        PersistenceStatus::MigrationCancelled => {
            return localizer.text("persistence-status-migration-cancelled");
        }
        PersistenceStatus::Saved(path) => ("persistence-status-saved", ("path", path)),
        PersistenceStatus::SaveCancelled => {
            return localizer.text("persistence-status-save-cancelled");
        }
        PersistenceStatus::SaveFailed(error) => {
            ("persistence-status-save-failed", ("error", error))
        }
        PersistenceStatus::ExitCancelled => {
            return localizer.text("persistence-status-exit-cancelled");
        }
        PersistenceStatus::RecoveryRestored(effect) => {
            ("persistence-status-recovery-restored", ("effect", effect))
        }
        PersistenceStatus::RecoveryDiscarded => {
            return localizer.text("persistence-status-recovery-discarded");
        }
        PersistenceStatus::RecoveryDiscardFailed(error) => (
            "persistence-status-recovery-discard-failed",
            ("error", error),
        ),
        PersistenceStatus::RecoveryAutosaveFailed(error) => (
            "persistence-status-recovery-autosave-failed",
            ("error", error),
        ),
        PersistenceStatus::RecoveryDiagnostic(detail) => {
            ("persistence-status-recovery-diagnostic", ("detail", detail))
        }
        PersistenceStatus::SettingsSaved => {
            return localizer.text("persistence-status-settings-saved");
        }
        PersistenceStatus::SettingsSaveFailed(error) => {
            ("persistence-status-settings-save-failed", ("error", error))
        }
        PersistenceStatus::SettingsDiagnostic(detail) => {
            ("persistence-status-settings-diagnostic", ("detail", detail))
        }
        PersistenceStatus::Migrated { .. } => unreachable!("handled above"),
    };
    let mut args = FluentArgs::new();
    args.set(argument.0, argument.1);
    localizer.text_with(message_id, &args)
}

#[derive(Resource)]
struct AutosaveState {
    document_key: String,
    observed_revision: u64,
    written_revision: Option<u64>,
    write_after: Instant,
    cleanup_after: Instant,
    enabled: bool,
    suspended: bool,
}

impl AutosaveState {
    fn new(session: &EditorSession, enabled: bool) -> Self {
        let now = Instant::now();
        Self {
            document_key: recovery_document_key(session),
            observed_revision: session.document_revision(),
            written_revision: session.dirty.then_some(session.document_revision()),
            write_after: now,
            cleanup_after: now,
            enabled,
            suspended: false,
        }
    }
}

fn initialize_document_persistence(
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
    settings: Res<EditorSettings>,
    settings_persistence: Res<SettingsPersistence>,
    localizer: Res<Localizer>,
) {
    let (mut recovery, candidate, recovery_diagnostic) = RecoveryPersistence::discover();
    if let Some(candidate) = candidate {
        recover_startup_session(&mut session, &mut recovery, candidate, &localizer);
    } else if let Some(diagnostic) = recovery_diagnostic {
        set_persistence_status(
            &mut session,
            &localizer,
            PersistenceStatus::RecoveryDiagnostic(diagnostic),
        );
    }
    session.playing = settings.preview.play_on_open;
    if let Some(diagnostic) = settings_persistence.diagnostic() {
        set_persistence_status(
            &mut session,
            &localizer,
            PersistenceStatus::SettingsDiagnostic(diagnostic.into()),
        );
    }
    let autosave = AutosaveState::new(&session, settings.general.autosave_enabled);
    commands.insert_resource(recovery);
    commands.insert_resource(autosave);
}

fn queue_document_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<DocumentAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

#[allow(clippy::type_complexity)]
fn handle_document_action_buttons(
    mut interactions: Query<
        (
            Entity,
            &Interaction,
            &DocumentAction,
            Option<&FeathersActionButton>,
            Option<&PendingFeathersActivation>,
            Has<ListItem>,
            &mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<Button>, With<FeathersActionButton>)>,
        ),
    >,
    mut commands: Commands,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, interaction, action, feathers, pending, list_item, mut background) in
        &mut interactions
    {
        match *interaction {
            Interaction::Hovered if feathers.is_none() => background.0 = theme::BUTTON_HOVER,
            Interaction::None if feathers.is_none() => background.0 = theme::PANEL_DARK,
            Interaction::Pressed => {
                // Library list rows activate through the ListBox ValueChange contract so mouse
                // and keyboard input take the same semantic route exactly once.
                if list_item {
                    background.0 = theme::ACCENT_DIM;
                    continue;
                }
                if feathers.is_some() {
                    if pending.is_none() {
                        continue;
                    }
                    commands
                        .entity(entity)
                        .remove::<PendingFeathersActivation>()
                        .insert(Interaction::None);
                } else {
                    background.0 = theme::ACCENT_DIM;
                }
                menu.open = None;
                menu.panels_open = false;
                if menu.tab_context.take().is_some() {
                    session.ui_revision += 1;
                }
                commands.trigger(*action);
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_document_action(
    action: On<DocumentAction>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
    settings: Res<EditorSettings>,
    catalog: Res<ProjectEffectCatalog>,
    mut workspace: ResMut<CurvesState>,
    mut recovery: ResMut<RecoveryPersistence>,
    mut autosave: ResMut<AutosaveState>,
    localizer: Res<Localizer>,
) {
    match *action {
        DocumentAction::New => {
            if confirm_discard(&session, &settings, &localizer) {
                session.new_effect();
                session.playing = settings.preview.play_on_open;
                workspace.clear();
                set_persistence_status(
                    &mut session,
                    &localizer,
                    PersistenceStatus::CreatedUntitled,
                );
            } else {
                set_persistence_status(&mut session, &localizer, PersistenceStatus::NewCancelled);
            }
        }
        DocumentAction::Open => {
            open_effect_dialog(&mut session, &settings, &localizer);
            workspace.clear();
        }
        DocumentAction::OpenCatalog(id) => {
            if let Some(path) = catalog.openable_path(id) {
                if confirm_discard(&session, &settings, &localizer) {
                    open_effect_path(&mut session, path, &settings, &localizer);
                    workspace.clear();
                } else {
                    set_persistence_status(
                        &mut session,
                        &localizer,
                        PersistenceStatus::OpenCancelled,
                    );
                }
            }
        }
        DocumentAction::Save => save_session(&mut session, false, &localizer),
        DocumentAction::SaveAs => save_session(&mut session, true, &localizer),
        DocumentAction::Exit => {
            if confirm_discard(&session, &settings, &localizer) {
                autosave.suspended = true;
                discard_active_recovery(&mut recovery);
                commands.write_message(AppExit::Success);
            } else {
                set_persistence_status(&mut session, &localizer, PersistenceStatus::ExitCancelled);
            }
        }
    }
}

fn recover_startup_session(
    session: &mut EditorSession,
    persistence: &mut RecoveryPersistence,
    candidate: RecoveryCandidate,
    localizer: &Localizer,
) {
    let source = candidate.source_path().map_or_else(
        || localizer.text("persistence-dialog-recovery-unsaved-source"),
        |path| path.display().to_string(),
    );
    let mut args = FluentArgs::new();
    args.set("source", source);
    let restore = matches!(
        MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title(localizer.text("persistence-dialog-recovery-title"))
            .set_description(localizer.text_with("persistence-dialog-recovery-description", &args),)
            .set_buttons(MessageButtons::YesNo)
            .show(),
        MessageDialogResult::Yes
    );
    if restore {
        session.restore_recovery(
            candidate.effect().clone(),
            candidate.source_path().map(Path::to_owned),
        );
        persistence.activate(&candidate);
        set_persistence_status(
            session,
            localizer,
            PersistenceStatus::RecoveryRestored(session.effect.name.clone()),
        );
    } else {
        match persistence.discard_candidate(&candidate) {
            Ok(()) => {
                set_persistence_status(session, localizer, PersistenceStatus::RecoveryDiscarded)
            }
            Err(error) => set_persistence_status(
                session,
                localizer,
                PersistenceStatus::RecoveryDiscardFailed(error.to_string()),
            ),
        }
    }
}

fn recovery_document_key(session: &EditorSession) -> String {
    format!(
        "{}|{}",
        session.effect.id,
        session.source_path.as_deref().map_or_else(
            || "<untitled>".to_string(),
            |path| path.display().to_string()
        )
    )
}

fn autosave_recovery(
    mut session: ResMut<EditorSession>,
    settings: Res<EditorSettings>,
    mut persistence: ResMut<RecoveryPersistence>,
    mut state: ResMut<AutosaveState>,
    localizer: Res<Localizer>,
) {
    autosave_recovery_at(
        &mut session,
        &settings,
        &mut persistence,
        &mut state,
        Instant::now(),
        &localizer,
    );
}

fn autosave_recovery_at(
    session: &mut EditorSession,
    settings: &EditorSettings,
    persistence: &mut RecoveryPersistence,
    state: &mut AutosaveState,
    now: Instant,
    localizer: &Localizer,
) {
    if state.suspended {
        return;
    }
    let interval = Duration::from_secs(u64::from(settings.general.autosave_interval_seconds));
    if state.enabled != settings.general.autosave_enabled {
        state.enabled = settings.general.autosave_enabled;
        state.write_after = now + interval;
        state.cleanup_after = now;
        if state.enabled {
            state.written_revision = None;
        }
    }
    if !state.enabled {
        try_clear_tracked_recovery(persistence, state, now, "disabled recovery snapshot");
        return;
    }
    let document_key = recovery_document_key(session);
    if document_key != state.document_key {
        if !try_clear_tracked_recovery(
            persistence,
            state,
            now,
            "previous document recovery snapshot",
        ) {
            return;
        }
        state.document_key = document_key;
        state.observed_revision = session.document_revision();
        state.written_revision = None;
        state.write_after = now + interval;
    }

    if !session.dirty {
        try_clear_tracked_recovery(persistence, state, now, "saved effect recovery snapshot");
        return;
    }

    let revision = session.document_revision();
    if revision != state.observed_revision {
        state.observed_revision = revision;
        state.written_revision = None;
        state.write_after = now + interval;
        return;
    }
    if state.written_revision == Some(revision) || now < state.write_after {
        return;
    }

    match persistence.persist(&session.effect, session.source_path.as_deref()) {
        Ok(_) => {
            state.written_revision = Some(revision);
            state.cleanup_after = now;
        }
        Err(error) => {
            error!("failed to write recovery snapshot: {error}");
            set_persistence_status(
                session,
                localizer,
                PersistenceStatus::RecoveryAutosaveFailed(error.to_string()),
            );
            state.write_after = now + interval;
        }
    }
}

fn try_clear_tracked_recovery(
    persistence: &mut RecoveryPersistence,
    state: &mut AutosaveState,
    now: Instant,
    context: &str,
) -> bool {
    if !persistence.has_active() {
        state.written_revision = None;
        state.cleanup_after = now;
        return true;
    }
    if now < state.cleanup_after {
        return false;
    }
    match persistence.clear_active() {
        Ok(()) => {
            state.written_revision = None;
            state.cleanup_after = now;
            true
        }
        Err(error) => {
            warn!("failed to clear the {context}: {error}");
            state.cleanup_after = now + RECOVERY_CLEANUP_RETRY_DELAY;
            false
        }
    }
}

fn discard_active_recovery(persistence: &mut RecoveryPersistence) {
    if let Err(error) = persistence.clear_active() {
        warn!("failed to discard recovery snapshot: {error}");
    }
}

fn open_effect_dialog(
    session: &mut EditorSession,
    settings: &EditorSettings,
    localizer: &Localizer,
) {
    if !confirm_discard(session, settings, localizer) {
        set_persistence_status(session, localizer, PersistenceStatus::OpenCancelled);
        return;
    }
    let mut dialog =
        FileDialog::new().add_filter(localizer.text("persistence-file-filter-effect"), &["ron"]);
    if let Some(directory) = session.source_path.as_ref().and_then(|path| path.parent()) {
        dialog = dialog.set_directory(directory);
    }
    let Some(path) = dialog.pick_file() else {
        set_persistence_status(session, localizer, PersistenceStatus::OpenCancelled);
        return;
    };
    open_effect_path(session, &path, settings, localizer);
}

fn open_effect_path(
    session: &mut EditorSession,
    path: &Path,
    settings: &EditorSettings,
    localizer: &Localizer,
) {
    let result = fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|source| prepare_effect_asset(&source).map_err(|error| error.to_string()));
    match result {
        Ok(EffectAssetLoad::Current(_)) => match session.open(path) {
            Ok(()) => {
                session.playing = settings.preview.play_on_open;
                set_persistence_status(
                    session,
                    localizer,
                    PersistenceStatus::Opened(path.display().to_string()),
                );
            }
            Err(error) => set_persistence_status(
                session,
                localizer,
                PersistenceStatus::OpenFailed(error.to_string()),
            ),
        },
        Ok(EffectAssetLoad::MigrationRequired(migration)) => {
            if !confirm_asset_migration(path, &migration, localizer) {
                set_persistence_status(session, localizer, PersistenceStatus::MigrationCancelled);
                return;
            }
            match persist_asset_migration(path, &migration) {
                Ok(backup) => match session.open(path) {
                    Ok(()) => {
                        session.playing = settings.preview.play_on_open;
                        set_persistence_status(
                            session,
                            localizer,
                            PersistenceStatus::Migrated {
                                path: path.display().to_string(),
                                backup: backup.display().to_string(),
                                from: migration.source_version,
                                to: migration.target_version,
                            },
                        );
                    }
                    Err(error) => set_persistence_status(
                        session,
                        localizer,
                        PersistenceStatus::OpenFailed(error.to_string()),
                    ),
                },
                Err(error) => {
                    set_persistence_status(session, localizer, PersistenceStatus::OpenFailed(error))
                }
            }
        }
        Err(error) => {
            set_persistence_status(session, localizer, PersistenceStatus::OpenFailed(error))
        }
    }
}

fn confirm_asset_migration(
    path: &Path,
    migration: &EffectAssetMigration,
    localizer: &Localizer,
) -> bool {
    let mut args = FluentArgs::new();
    args.set("path", path.display().to_string());
    args.set("from", i64::from(migration.source_version));
    args.set("to", i64::from(migration.target_version));
    matches!(
        MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title(localizer.text("persistence-dialog-migration-title"))
            .set_description(
                localizer.text_with("persistence-dialog-migration-description", &args),
            )
            .set_buttons(MessageButtons::YesNo)
            .show(),
        MessageDialogResult::Yes
    )
}

fn persist_asset_migration(
    path: &Path,
    migration: &EffectAssetMigration,
) -> Result<PathBuf, String> {
    EffectCompiler::default()
        .compile(&migration.asset)
        .map_err(|error| format!("migrated effect does not compile: {error}"))?;
    let backup = unique_migration_backup(path, migration.source_version);
    fs::copy(path, &backup)
        .and_then(|_| fs::OpenOptions::new().write(true).open(&backup)?.sync_all())
        .map_err(|error| format!("could not back up the original effect: {error}"))?;
    if let Err(error) = migration.asset.save_ron(path) {
        if let Err(cleanup_error) = fs::remove_file(&backup) {
            warn!(
                "failed to remove unused migration backup after replacement failure: {cleanup_error}"
            );
        }
        return Err(format!(
            "could not replace the effect after backup: {error}"
        ));
    }
    Ok(backup)
}

fn unique_migration_backup(path: &Path, source_version: u32) -> PathBuf {
    let mut index = 0_u32;
    loop {
        let suffix = if index == 0 {
            format!("v{source_version}.backup")
        } else {
            format!("v{source_version}.backup-{index}")
        };
        let mut name = path.as_os_str().to_os_string();
        name.push(format!(".{suffix}"));
        let candidate = PathBuf::from(name);
        if !candidate.exists() {
            return candidate;
        }
        index += 1;
    }
}

fn confirm_discard(
    session: &EditorSession,
    settings: &EditorSettings,
    localizer: &Localizer,
) -> bool {
    if !session.dirty || !settings.general.confirm_unsaved_changes {
        return true;
    }
    matches!(
        MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title(localizer.text("persistence-dialog-unsaved-title"))
            .set_description(localizer.text("persistence-dialog-unsaved-description"))
            .set_buttons(MessageButtons::YesNo)
            .show(),
        MessageDialogResult::Yes
    )
}

pub(crate) fn persist_editor_settings(
    settings: &EditorSettings,
    persistence: &mut SettingsPersistence,
    session: &mut EditorSession,
    localizer: &Localizer,
) {
    match persistence.persist(settings) {
        Ok(()) => set_persistence_status(session, localizer, PersistenceStatus::SettingsSaved),
        Err(error) => set_persistence_status(
            session,
            localizer,
            PersistenceStatus::SettingsSaveFailed(error.to_string()),
        ),
    }
}

fn save_session(session: &mut EditorSession, save_as: bool, localizer: &Localizer) {
    if !save_as && session.source_path.is_some() {
        let path = session
            .source_path
            .as_deref()
            .unwrap()
            .display()
            .to_string();
        match session.save() {
            Ok(()) => set_persistence_status(session, localizer, PersistenceStatus::Saved(path)),
            Err(error) => set_persistence_status(
                session,
                localizer,
                PersistenceStatus::SaveFailed(error.to_string()),
            ),
        }
        return;
    }

    let file_name = format!("{}.aestra.ron", session.effect.id);
    let mut dialog = FileDialog::new()
        .add_filter(localizer.text("persistence-file-filter-effect"), &["ron"])
        .set_file_name(file_name);
    if let Some(directory) = session.source_path.as_ref().and_then(|path| path.parent()) {
        dialog = dialog.set_directory(directory);
    }
    let Some(path) = dialog.save_file() else {
        set_persistence_status(session, localizer, PersistenceStatus::SaveCancelled);
        return;
    };
    let display_path = path.display().to_string();
    match session.save_as(path) {
        Ok(()) => {
            set_persistence_status(session, localizer, PersistenceStatus::Saved(display_path))
        }
        Err(error) => set_persistence_status(
            session,
            localizer,
            PersistenceStatus::SaveFailed(error.to_string()),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_window_close_requests(
    mut close_requests: MessageReader<WindowCloseRequested>,
    primary: Single<Entity, With<PrimaryWindow>>,
    floating_windows: Query<&NativeFloatingWindow>,
    mut layout: ResMut<WorkspaceLayout>,
    mut session: ResMut<EditorSession>,
    settings: Res<EditorSettings>,
    mut recovery: ResMut<RecoveryPersistence>,
    mut autosave: ResMut<AutosaveState>,
    mut commands: Commands,
    localizer: Res<Localizer>,
) {
    for request in close_requests.read() {
        if request.window == *primary {
            if confirm_discard(&session, &settings, &localizer) {
                autosave.suspended = true;
                discard_active_recovery(&mut recovery);
                commands.write_message(AppExit::Success);
            } else {
                set_persistence_status(&mut session, &localizer, PersistenceStatus::ExitCancelled);
            }
            continue;
        }
        let Ok(floating) = floating_windows.get(request.window) else {
            continue;
        };
        if layout.redock(floating.0) {
            if let Err(error) = layout.save() {
                warn!("failed to save editor workspace layout: {error}");
            }
            session.ui_revision += 1;
            let mut args = FluentArgs::new();
            args.set("panel", localizer.text(floating.0.message_id()));
            session.status = localizer.text_with("dock-status-redocked-after-close", &args);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_open_action_uses_stable_id_and_document_protection_path() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("catalog-effect.aestra.ron");
        let mut effect = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        effect.name = "Catalog Effect".into();
        effect.save_ron(&path).unwrap();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let id = catalog.entries()[0].id;
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let autosave = AutosaveState::new(&session, true);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(EditorSettings::default())
            .insert_resource(catalog)
            .init_resource::<CurvesState>()
            .insert_resource(RecoveryPersistence::for_test(
                temporary.path().join("recovery"),
                None,
            ))
            .insert_resource(autosave)
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(execute_document_action);

        app.world_mut().trigger(DocumentAction::OpenCatalog(id));
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.name, "Catalog Effect");
        assert_eq!(session.source_path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn invalid_catalog_action_cannot_replace_the_document() {
        let temporary = tempfile::tempdir().unwrap();
        fs::write(temporary.path().join("broken.aestra.ron"), "not RON").unwrap();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let id = catalog.entries()[0].id;
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let original_id = session.effect.id;
        let autosave = AutosaveState::new(&session, true);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(EditorSettings::default())
            .insert_resource(catalog)
            .init_resource::<CurvesState>()
            .insert_resource(RecoveryPersistence::for_test(
                temporary.path().join("recovery"),
                None,
            ))
            .insert_resource(autosave)
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(execute_document_action);

        app.world_mut().trigger(DocumentAction::OpenCatalog(id));
        app.update();

        assert_eq!(
            app.world().resource::<EditorSession>().effect.id,
            original_id
        );
    }

    #[test]
    fn persistence_outcomes_are_localized_with_technical_details_preserved() {
        let english = Localizer::new("en-US").unwrap();
        assert_eq!(
            localize_persistence_status(PersistenceStatus::OpenCancelled, &english),
            "Open cancelled"
        );
        assert_eq!(
            localize_persistence_status(PersistenceStatus::NewCancelled, &english),
            "New effect cancelled"
        );
        let opened = localize_persistence_status(
            PersistenceStatus::Opened("C:\\effects\\spark.aestra.ron".into()),
            &english,
        );
        assert!(opened.starts_with("Opened "));
        assert!(opened.contains("C:\\effects\\spark.aestra.ron"));
        let failed = localize_persistence_status(
            PersistenceStatus::SaveFailed("access denied".into()),
            &english,
        );
        assert!(failed.starts_with("Save failed: "));
        assert!(failed.contains("access denied"));
        let migrated = localize_persistence_status(
            PersistenceStatus::Migrated {
                path: "spark.aestra.ron".into(),
                backup: "spark.aestra.ron.v2.backup".into(),
                from: 2,
                to: 3,
            },
            &english,
        );
        assert!(migrated.contains("spark.aestra.ron"));
        assert!(migrated.contains("Migrated "));
        assert!(migrated.contains("spark.aestra.ron.v2.backup"));

        let french = Localizer::new("fr-FR").unwrap();
        assert_eq!(
            localize_persistence_status(PersistenceStatus::ExitCancelled, &french),
            "Fermeture annulée"
        );
        let restored = localize_persistence_status(
            PersistenceStatus::RecoveryRestored("Prisme".into()),
            &french,
        );
        assert!(restored.contains("Prisme"));
        assert!(restored.ends_with(" non enregistré récupéré"));
    }

    #[test]
    fn document_domain_operations_leave_status_presentation_to_the_plugin() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("effect.aestra.ron");
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.status = "sentinel".into();

        session.new_effect();
        assert_eq!(session.status, "sentinel");
        session.save_as(&path).unwrap();
        assert_eq!(session.status, "sentinel");
        session.open(&path).unwrap();
        assert_eq!(session.status, "sentinel");
    }

    #[test]
    fn saving_a_document_clears_its_tracked_recovery_snapshot() {
        let temporary = tempfile::tempdir().unwrap();
        let source_path = temporary.path().join("saved-effect.aestra.ron");
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.save_as(&source_path).unwrap();
        session.adjust_effect_duration(0.25);
        let mut persistence = RecoveryPersistence::for_test(temporary.path().into(), None);
        let recovery_path = persistence
            .persist(&session.effect, session.source_path.as_deref())
            .unwrap();
        let mut state = AutosaveState::new(&session, true);
        session.save().unwrap();

        autosave_recovery_at(
            &mut session,
            &EditorSettings::default(),
            &mut persistence,
            &mut state,
            Instant::now(),
            &Localizer::new("en-US").unwrap(),
        );

        assert!(!recovery_path.exists());
        assert!(!persistence.has_active());
        assert!(state.written_revision.is_none());
    }

    #[test]
    fn disabling_autosave_clears_a_snapshot_even_without_a_written_revision_marker() {
        let temporary = tempfile::tempdir().unwrap();
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let mut persistence = RecoveryPersistence::for_test(temporary.path().into(), None);
        let recovery_path = persistence
            .persist(&session.effect, session.source_path.as_deref())
            .unwrap();
        let mut state = AutosaveState::new(&session, true);
        assert!(state.written_revision.is_none());
        let settings = EditorSettings {
            general: settings::GeneralSettings {
                autosave_enabled: false,
                ..default()
            },
            ..default()
        };

        autosave_recovery_at(
            &mut session,
            &settings,
            &mut persistence,
            &mut state,
            Instant::now(),
            &Localizer::new("en-US").unwrap(),
        );

        assert!(!recovery_path.exists());
        assert!(!persistence.has_active());
        assert!(!state.enabled);
    }

    #[test]
    fn document_switch_waits_for_failed_cleanup_and_retries() {
        let temporary = tempfile::tempdir().unwrap();
        let blocked_path = temporary.path().join("blocked.recovery.ron");
        fs::create_dir(&blocked_path).unwrap();
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let mut state = AutosaveState::new(&session, true);
        let previous_document_key = state.document_key.clone();
        let mut persistence =
            RecoveryPersistence::for_test(temporary.path().into(), Some(blocked_path.clone()));
        session.new_effect();
        let next_document_key = recovery_document_key(&session);
        let now = Instant::now();

        autosave_recovery_at(
            &mut session,
            &EditorSettings::default(),
            &mut persistence,
            &mut state,
            now,
            &Localizer::new("en-US").unwrap(),
        );

        assert!(persistence.has_active());
        assert_eq!(state.document_key, previous_document_key);

        fs::remove_dir(&blocked_path).unwrap();
        fs::write(&blocked_path, "pending snapshot").unwrap();
        autosave_recovery_at(
            &mut session,
            &EditorSettings::default(),
            &mut persistence,
            &mut state,
            now + RECOVERY_CLEANUP_RETRY_DELAY,
            &Localizer::new("en-US").unwrap(),
        );

        assert!(!blocked_path.exists());
        assert!(!persistence.has_active());
        assert_eq!(state.document_key, next_document_key);
    }

    #[test]
    fn document_action_activation_uses_the_feathers_contract() {
        let mut app = App::new();
        app.add_observer(queue_document_action_activation);
        let action = app
            .world_mut()
            .spawn((
                DocumentAction::Save,
                FeathersActionButton,
                Interaction::None,
            ))
            .id();

        app.world_mut().trigger(Activate { entity: action });
        app.update();

        let action = app.world().entity(action);
        assert!(action.contains::<PendingFeathersActivation>());
        assert_eq!(action.get::<Interaction>(), Some(&Interaction::Pressed));
    }

    #[test]
    fn migration_preserves_the_original_and_atomically_replaces_the_effect() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("legacy.aestra.ron");
        let original = "legacy format source";
        fs::write(&path, original).unwrap();
        let asset = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        let migration = EffectAssetMigration {
            source_version: 2,
            target_version: 3,
            asset: asset.clone(),
        };

        let backup = persist_asset_migration(&path, &migration).unwrap();

        assert_eq!(fs::read_to_string(&backup).unwrap(), original);
        assert_eq!(EffectAsset::load_ron(&path).unwrap(), asset);
        assert_eq!(
            backup.file_name().unwrap().to_string_lossy(),
            "legacy.aestra.ron.v2.backup"
        );
    }

    #[test]
    fn failed_migration_leaves_the_source_untouched() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("legacy.aestra.ron");
        let original = "legacy format source";
        fs::write(&path, original).unwrap();
        let mut asset = EffectAsset::from_ron(EFFECT_SOURCE).unwrap();
        asset.duration = 0.0;
        let migration = EffectAssetMigration {
            source_version: 2,
            target_version: 3,
            asset,
        };

        assert!(persist_asset_migration(&path, &migration).is_err());

        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert!(
            !temporary
                .path()
                .join("legacy.aestra.ron.v2.backup")
                .exists()
        );
    }

    #[test]
    fn migration_backups_never_overwrite_an_existing_backup() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("legacy.aestra.ron");
        fs::write(&path, "legacy format source").unwrap();
        fs::write(
            temporary.path().join("legacy.aestra.ron.v2.backup"),
            "older backup",
        )
        .unwrap();
        let migration = EffectAssetMigration {
            source_version: 2,
            target_version: 3,
            asset: EffectAsset::from_ron(EFFECT_SOURCE).unwrap(),
        };

        let backup = persist_asset_migration(&path, &migration).unwrap();

        assert_eq!(
            backup.file_name().unwrap().to_string_lossy(),
            "legacy.aestra.ron.v2.backup-1"
        );
        assert_eq!(
            fs::read_to_string(temporary.path().join("legacy.aestra.ron.v2.backup")).unwrap(),
            "older backup"
        );
    }
}
