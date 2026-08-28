//! Editor document I/O, recovery, autosave, and application-exit lifecycle.

use crate::recovery::{RecoveryCandidate, RecoveryPersistence};
use crate::*;
use bevy::ui_widgets::Activate;
use fluent_bundle::FluentArgs;
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
#[cfg(test)]
use std::fs;
use std::{
    path::Path,
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
    OpenCatalog(usize),
    Save,
    SaveAs,
    Exit,
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
) {
    let (mut recovery, candidate, recovery_diagnostic) = RecoveryPersistence::discover();
    if let Some(candidate) = candidate {
        recover_startup_session(&mut session, &mut recovery, candidate);
    } else if let Some(diagnostic) = recovery_diagnostic {
        session.status = diagnostic;
    }
    session.playing = settings.preview.play_on_open;
    if let Some(diagnostic) = settings_persistence.diagnostic() {
        session.status = diagnostic.into();
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
    for (entity, interaction, action, feathers, pending, mut background) in &mut interactions {
        match *interaction {
            Interaction::Hovered if feathers.is_none() => background.0 = theme::BUTTON_HOVER,
            Interaction::None if feathers.is_none() => background.0 = theme::PANEL_DARK,
            Interaction::Pressed => {
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
    catalog: Res<EffectCatalog>,
    mut workspace: ResMut<CurvesState>,
    mut recovery: ResMut<RecoveryPersistence>,
    mut autosave: ResMut<AutosaveState>,
) {
    match *action {
        DocumentAction::New => {
            if confirm_discard(&session, &settings) {
                session.new_effect();
                session.playing = settings.preview.play_on_open;
                workspace.clear();
            }
        }
        DocumentAction::Open => {
            open_effect_dialog(&mut session, &settings);
            workspace.clear();
        }
        DocumentAction::OpenCatalog(index) => {
            if confirm_discard(&session, &settings) {
                if let Some(path) = catalog.path(index) {
                    match session.open(path) {
                        Ok(()) => session.playing = settings.preview.play_on_open,
                        Err(error) => session.status = format!("Open failed: {error}"),
                    }
                }
                workspace.clear();
            } else {
                session.status = "Open cancelled".into();
            }
        }
        DocumentAction::Save => save_session(&mut session, false),
        DocumentAction::SaveAs => save_session(&mut session, true),
        DocumentAction::Exit => {
            if confirm_discard(&session, &settings) {
                autosave.suspended = true;
                discard_active_recovery(&mut recovery);
                commands.write_message(AppExit::Success);
            } else {
                session.status = "Exit cancelled".into();
            }
        }
    }
}

fn recover_startup_session(
    session: &mut EditorSession,
    persistence: &mut RecoveryPersistence,
    candidate: RecoveryCandidate,
) {
    let source = candidate.source_path().map_or_else(
        || "an unsaved effect".to_string(),
        |path| path.display().to_string(),
    );
    let restore = matches!(
        MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("Recover unsaved effect")
            .set_description(format!(
                "A newer recovery snapshot was found for {source}.\n\nRestore it? Yes restores the unsaved work; No discards the snapshot."
            ))
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
    } else {
        match persistence.discard_candidate(&candidate) {
            Ok(()) => session.status = "Discarded recovery snapshot".into(),
            Err(error) => session.status = format!("Recovery discard failed: {error}"),
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
) {
    autosave_recovery_at(
        &mut session,
        &settings,
        &mut persistence,
        &mut state,
        Instant::now(),
    );
}

fn autosave_recovery_at(
    session: &mut EditorSession,
    settings: &EditorSettings,
    persistence: &mut RecoveryPersistence,
    state: &mut AutosaveState,
    now: Instant,
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
            session.status = format!("Recovery autosave failed: {error}");
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

fn open_effect_dialog(session: &mut EditorSession, settings: &EditorSettings) {
    if !confirm_discard(session, settings) {
        session.status = "Open cancelled".into();
        return;
    }
    let mut dialog = FileDialog::new().add_filter("Aestra effect", &["ron"]);
    if let Some(directory) = session.source_path.as_ref().and_then(|path| path.parent()) {
        dialog = dialog.set_directory(directory);
    }
    let Some(path) = dialog.pick_file() else {
        session.status = "Open cancelled".into();
        return;
    };
    match session.open(&path) {
        Ok(()) => session.playing = settings.preview.play_on_open,
        Err(error) => session.status = format!("Open failed: {error}"),
    }
}

fn confirm_discard(session: &EditorSession, settings: &EditorSettings) -> bool {
    if !session.dirty || !settings.general.confirm_unsaved_changes {
        return true;
    }
    matches!(
        MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("Unsaved changes")
            .set_description("Discard the unsaved changes to the current effect?")
            .set_buttons(MessageButtons::YesNo)
            .show(),
        MessageDialogResult::Yes
    )
}

pub(crate) fn persist_editor_settings(
    settings: &EditorSettings,
    persistence: &mut SettingsPersistence,
    session: &mut EditorSession,
) {
    match persistence.persist(settings) {
        Ok(()) => session.status = "Editor settings saved".into(),
        Err(error) => session.status = format!("Settings save failed: {error}"),
    }
}

fn save_session(session: &mut EditorSession, save_as: bool) {
    if !save_as && session.source_path.is_some() {
        if let Err(error) = session.save() {
            session.status = format!("Save failed: {error}");
        }
        return;
    }

    let file_name = format!("{}.aestra.ron", session.effect.id);
    let mut dialog = FileDialog::new()
        .add_filter("Aestra effect", &["ron"])
        .set_file_name(file_name);
    if let Some(directory) = session.source_path.as_ref().and_then(|path| path.parent()) {
        dialog = dialog.set_directory(directory);
    }
    let Some(path) = dialog.save_file() else {
        session.status = "Save cancelled".into();
        return;
    };
    if let Err(error) = session.save_as(path) {
        session.status = format!("Save failed: {error}");
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
            if confirm_discard(&session, &settings) {
                autosave.suspended = true;
                discard_active_recovery(&mut recovery);
                commands.write_message(AppExit::Success);
            } else {
                session.status = "Exit cancelled".into();
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
}
