//! Editor document I/O, recovery, autosave, and application-exit lifecycle.
mod background;
mod material;
pub(crate) mod recovery_dialog;
mod recovery_target;

use crate::recovery::{RecoveryCandidate, RecoveryPersistence};
use crate::timeline::{TimelineNavigationSnapshot, TimelineState};
use crate::*;
use aestra_compiler::EffectCompiler;
use aestra_core::{EffectAssetLoad, EffectAssetMigration, EffectClipId, prepare_effect_asset};
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
        app.init_resource::<DocumentProtectionState>()
            .init_resource::<recovery_dialog::RecoveryDialogState>()
            .init_resource::<crate::project_content::io::ProjectIoTasks>()
            .init_resource::<SourceNavigationState>()
            .add_observer(queue_document_action_activation)
            .add_observer(resolve_document_protection)
            .add_observer(execute_document_action)
            .add_observer(recovery_dialog::activate)
            .add_systems(
                Startup,
                initialize_document_persistence.in_set(PersistenceSet::Startup),
            )
            .add_systems(
                Update,
                (
                    recovery_dialog::escape,
                    dismiss_document_protection_with_escape,
                    handle_document_action_buttons,
                    sync_document_protection_overlay,
                    recovery_dialog::sync,
                )
                    .chain()
                    .in_set(PersistenceSet::Actions),
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
    OpenProject,
    OpenCatalog(EffectAssetRef),
    OpenCatalogClip(EffectAssetRef, EffectClipId),
    OpenSource(EffectAssetRef),
    OpenSourceEmitter(EffectAssetRef, EmitterId),
    BackToSource,
    ForwardToSource,
    NavigateSourceAncestor(usize),
    Save,
    SaveAs,
    ReloadMaterial,
    Exit,
}

#[cfg(test)]
pub(crate) fn install_document_open_test_runtime(app: &mut App, recovery_path: PathBuf) {
    let autosave = AutosaveState::new(app.world().resource::<EditorSession>(), true);
    app.insert_resource(EditorSettings::default())
        .init_resource::<CurvesState>()
        .insert_resource(RecoveryPersistence::for_test(recovery_path, None))
        .insert_resource(autosave)
        .init_resource::<DocumentProtectionState>()
        .init_resource::<crate::project_content::io::ProjectIoTasks>()
        .add_observer(execute_document_action)
        .add_systems(Update, crate::project_content::io::poll);
    crate::viewport::install_project_preview_test_runtime(app);
}

#[derive(Clone, Debug)]
struct SourceNavigationEntry {
    path: PathBuf,
    effect: EffectAssetRef,
    name: String,
    playhead_time: f32,
    playing: bool,
    selection: SemanticTarget,
    timeline: TimelineNavigationSnapshot,
}

#[derive(Resource, Debug, Default, Clone)]
pub(crate) struct SourceNavigationState {
    back: Vec<SourceNavigationEntry>,
    forward: Vec<SourceNavigationEntry>,
}

impl SourceNavigationState {
    pub(crate) fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub(crate) fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub(crate) fn depth(&self) -> usize {
        self.back.len()
    }

    pub(crate) fn breadcrumb(&self, current: &str) -> Vec<String> {
        self.back
            .iter()
            .map(|entry| entry.name.clone())
            .chain(std::iter::once(current.to_owned()))
            .collect()
    }

    fn contains(&self, effect: EffectAssetRef) -> bool {
        self.back.iter().any(|entry| entry.effect == effect)
    }

    fn clear(&mut self) {
        self.back.clear();
        self.forward.clear();
    }
}

#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DocumentProtectionState {
    recovery_open: bool,
    pub(crate) asset_recovery_open: bool,
    pending: Option<DocumentAction>,
    reload_target: Option<crate::material_document::MaterialEditingTarget>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentProtectionAction {
    Save,
    Discard,
    Cancel,
}

#[derive(Component)]
struct DocumentProtectionOverlay;

#[derive(Component)]
struct DocumentProtectionDescription;

impl DocumentProtectionState {
    pub(crate) fn is_open(&self) -> bool {
        self.pending.is_some() || self.recovery_open || self.asset_recovery_open
    }
}

pub(crate) fn spawn_document_protection_overlay(
    parent: &mut ChildSpawnerCommands,
    state: &DocumentProtectionState,
    localizer: &Localizer,
) {
    parent
        .spawn((
            DocumentProtectionOverlay,
            GlobalZIndex(300),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Node {
                display: if state.pending.is_some() {
                    Display::Flex
                } else {
                    Display::None
                },
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.005, 0.007, 0.014, 0.82)),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: Val::Px(440.0),
                        max_width: Val::Percent(92.0),
                        padding: UiRect::all(Val::Px(22.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(12.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(7.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                    BorderColor::all(theme::BORDER_BRIGHT),
                ))
                .with_children(|dialog| {
                    dialog.spawn((
                        Text::new(localizer.text("persistence-dialog-unsaved-title")),
                        TextFont {
                            font_size: FontSize::Px(17.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Pickable::IGNORE,
                    ));
                    dialog.spawn((
                        DocumentProtectionDescription,
                        Text::new(localizer.text(
                            if state.pending == Some(DocumentAction::ReloadMaterial) {
                                if matches!(state.reload_target, Some(crate::material_document::MaterialEditingTarget::Function { .. })) {
                                    "function-reload-confirm"
                                } else {
                                    "material-reload-confirm"
                                }
                            } else {
                                "persistence-dialog-unsaved-description"
                            },
                        )),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        Pickable::IGNORE,
                    ));
                    dialog
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::End,
                            column_gap: Val::Px(8.0),
                            margin: UiRect::top(Val::Px(6.0)),
                            ..default()
                        })
                        .with_children(|buttons| {
                            for (message, action) in [
                                ("common-cancel", DocumentProtectionAction::Cancel),
                                ("common-discard", DocumentProtectionAction::Discard),
                                ("common-save", DocumentProtectionAction::Save),
                            ] {
                                let label = localizer.text(message);
                                buttons
                                    .spawn_empty()
                                    .apply_scene(ui_shell::feathers_button())
                                    .insert((
                                        action,
                                        FeathersActionButton,
                                        AccessibleLabel(label.clone()),
                                        Node {
                                            min_width: Val::Px(82.0),
                                            height: Val::Px(30.0),
                                            padding: UiRect::horizontal(Val::Px(12.0)),
                                            align_items: AlignItems::Center,
                                            justify_content: JustifyContent::Center,
                                            ..default()
                                        },
                                    ))
                                    .with_child((Text::new(label), ThemedText, Pickable::IGNORE));
                            }
                        });
                });
        });
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PersistenceStatus {
    CreatedUntitled,
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
    observed_material_drafts: crate::material_drafts::MaterialDrafts,
    observed_material_target: crate::material_document::MaterialEditingTarget,
    written_revision: Option<u64>,
    write_after: Instant,
    first_unwritten_edit: Option<Instant>,
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
            observed_material_drafts: session.material_drafts.clone(),
            observed_material_target: session.material_target.clone(),
            written_revision: session.dirty.then_some(session.document_revision()),
            write_after: now,
            first_unwritten_edit: None,
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
    mut protection: ResMut<DocumentProtectionState>,
    mut dialog: ResMut<recovery_dialog::RecoveryDialogState>,
) {
    let (recovery, candidate, recovery_diagnostic) = RecoveryPersistence::discover();
    protection.recovery_open = candidate.is_some();
    dialog.candidate = candidate;
    if let Some(diagnostic) = recovery_diagnostic {
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
    let mut autosave = AutosaveState::new(&session, settings.general.autosave_enabled);
    autosave.suspended = protection.recovery_open;
    commands.insert_resource(recovery);
    commands.insert_resource(autosave);
}

fn queue_document_action_activation(
    activate: On<Activate>,
    actions: Query<&DocumentAction, With<FeathersActionButton>>,
    mut commands: Commands,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    menu.open = None;
    menu.panels_open = false;
    if menu.tab_context.take().is_some() {
        session.ui_revision += 1;
    }
    commands.trigger(*action);
}

#[allow(clippy::type_complexity)]
fn handle_document_action_buttons(
    mut interactions: Query<
        (
            Entity,
            &Interaction,
            &DocumentAction,
            Has<ListItem>,
            &mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            With<Button>,
            Without<FeathersActionButton>,
        ),
    >,
    mut commands: Commands,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for (_entity, interaction, action, list_item, mut background) in &mut interactions {
        match *interaction {
            Interaction::Hovered => background.0 = theme::BUTTON_HOVER,
            Interaction::None => background.0 = theme::PANEL_DARK,
            Interaction::Pressed => {
                // Library list rows activate through the ListBox ValueChange contract so mouse
                // and keyboard input take the same semantic route exactly once.
                if list_item {
                    background.0 = theme::ACCENT_DIM;
                    continue;
                }
                background.0 = theme::ACCENT_DIM;
                menu.open = None;
                menu.panels_open = false;
                if menu.tab_context.take().is_some() {
                    session.ui_revision += 1;
                }
                commands.trigger(*action);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_document_action(
    action: On<DocumentAction>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
    settings: Res<EditorSettings>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut workspace: ResMut<CurvesState>,
    mut recovery: ResMut<RecoveryPersistence>,
    mut autosave: ResMut<AutosaveState>,
    localizer: Res<Localizer>,
    mut protection: ResMut<DocumentProtectionState>,
    mut timeline: Option<ResMut<TimelineState>>,
    mut navigation: Option<ResMut<SourceNavigationState>>,
    io_tasks: Option<Res<crate::project_content::io::ProjectIoTasks>>,
) {
    if !crate::project_content::io::idle(io_tasks) || protection.is_open() {
        return;
    }
    if matches!(*action, DocumentAction::Save | DocumentAction::SaveAs) {
        if session.standalone_function().is_some() {
            if *action == DocumentAction::SaveAs {
                session.status = localizer.text("material-save-as-unavailable");
            } else {
                material::queue_save(&mut commands, &session, &catalog, false);
            }
            return;
        }
        if session.standalone_material().is_some() {
            if *action == DocumentAction::SaveAs {
                session.status = localizer.text("material-save-as-unavailable");
            } else {
                material::queue_save(&mut commands, &session, &catalog, false);
            }
            return;
        }
        background::queue_save(
            &mut commands,
            &mut session,
            &catalog,
            matches!(*action, DocumentAction::SaveAs),
            None,
            &localizer,
        );
        return;
    }
    if *action == DocumentAction::ReloadMaterial {
        let dirty = match &session.material_target {
            crate::material_document::MaterialEditingTarget::Program { id, .. } => {
                catalog.material_drafts.programs.contains_key(id)
            }
            crate::material_document::MaterialEditingTarget::Function { id, .. } => {
                catalog.material_drafts.functions.contains_key(id)
            }
            _ => return,
        };
        if dirty {
            protection.pending = Some(*action);
            protection.reload_target = Some(session.material_target.clone());
        } else {
            material::queue_reload(&mut commands, &session, &catalog);
        }
        return;
    }
    if document_action_requires_confirmation(&session, &settings) {
        protection.pending = Some(*action);
        return;
    }
    // Queuing an open only reads the catalog. A mutable dereference would notify the
    // viewport, reinstall the current preview, and invalidate the pending I/O guard.
    if background::queue_open(
        *action,
        &mut commands,
        &mut session,
        &settings,
        &catalog,
        &localizer,
        timeline.as_deref(),
        navigation.as_deref(),
    ) {
        return;
    }
    execute_protected_document_action(
        *action,
        &mut commands,
        &mut session,
        &settings,
        &mut catalog,
        &mut workspace,
        &mut recovery,
        &mut autosave,
        &localizer,
        timeline.as_deref_mut(),
        navigation.as_deref_mut(),
    );
}

fn dismiss_document_protection_with_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut protection: ResMut<DocumentProtectionState>,
) {
    if protection.pending.is_some() && keys.just_pressed(KeyCode::Escape) {
        protection.pending = None;
        protection.reload_target = None;
    }
}

fn sync_document_protection_overlay(
    protection: Res<DocumentProtectionState>,
    localizer: Res<Localizer>,
    mut overlays: Query<&mut Node, With<DocumentProtectionOverlay>>,
    mut descriptions: Query<&mut Text, With<DocumentProtectionDescription>>,
) {
    if !protection.is_changed() {
        return;
    }
    let display = if protection.pending.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut overlays {
        node.display = display;
    }
    for mut text in &mut descriptions {
        text.0 = localizer.text(
            if protection.pending == Some(DocumentAction::ReloadMaterial) {
                if matches!(
                    protection.reload_target,
                    Some(crate::material_document::MaterialEditingTarget::Function { .. })
                ) {
                    "function-reload-confirm"
                } else {
                    "material-reload-confirm"
                }
            } else {
                "persistence-dialog-unsaved-description"
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_protected_document_action(
    action: DocumentAction,
    commands: &mut Commands,
    session: &mut EditorSession,
    settings: &EditorSettings,
    catalog: &mut ProjectEffectCatalog,
    workspace: &mut CurvesState,
    recovery: &mut RecoveryPersistence,
    autosave: &mut AutosaveState,
    localizer: &Localizer,
    timeline: Option<&mut TimelineState>,
    navigation: Option<&mut SourceNavigationState>,
) {
    if background::queue_open(
        action,
        commands,
        session,
        settings,
        catalog,
        localizer,
        timeline.as_deref(),
        navigation.as_deref(),
    ) {
        return;
    }
    let drafts = std::mem::take(&mut catalog.material_drafts);
    let generation = session.history_generation();
    match action {
        DocumentAction::New => {
            if let Some(navigation) = navigation {
                navigation.clear();
            }
            session.new_effect();
            session.playing = settings.preview.play_on_open;
            if let Some(timeline) = timeline {
                *timeline = TimelineState::framed(session.playback_duration());
            }
            workspace.clear();
            set_persistence_status(session, localizer, PersistenceStatus::CreatedUntitled);
        }
        DocumentAction::Exit => {
            autosave.suspended = true;
            discard_active_recovery(recovery);
            commands.write_message(AppExit::Success);
        }
        _ => {}
    }
    if session.history_generation() == generation && action != DocumentAction::Exit {
        catalog.material_drafts = drafts;
    }
    session.set_material_drafts(catalog.material_drafts.clone());
}

fn document_action_requires_confirmation(
    session: &EditorSession,
    settings: &EditorSettings,
) -> bool {
    session.dirty && settings.general.confirm_unsaved_changes
}

#[allow(clippy::too_many_arguments)]
fn resolve_document_protection(
    activate: On<Activate>,
    actions: Query<&DocumentProtectionAction>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
    settings: Res<EditorSettings>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut workspace: ResMut<CurvesState>,
    mut recovery: ResMut<RecoveryPersistence>,
    mut autosave: ResMut<AutosaveState>,
    localizer: Res<Localizer>,
    mut protection: ResMut<DocumentProtectionState>,
    mut timeline: Option<ResMut<TimelineState>>,
    mut navigation: Option<ResMut<SourceNavigationState>>,
    io_tasks: Option<Res<crate::project_content::io::ProjectIoTasks>>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    if *action == DocumentProtectionAction::Cancel {
        protection.pending = None;
        protection.reload_target = None;
        return;
    }
    if !crate::project_content::io::idle(io_tasks) {
        return;
    }
    if protection.pending == Some(DocumentAction::ReloadMaterial) {
        if protection.reload_target.as_ref() != Some(&session.material_target) {
            protection.pending = None;
            protection.reload_target = None;
            session.status = localizer.text("project-operation-open-stale");
            return;
        }
        if *action == DocumentProtectionAction::Save {
            material::queue_save(&mut commands, &session, &catalog, true);
        } else {
            protection.pending = None;
            material::queue_reload(&mut commands, &session, &catalog);
        }
        return;
    }
    if *action == DocumentProtectionAction::Save {
        background::queue_save(
            &mut commands,
            &mut session,
            &catalog,
            false,
            protection.pending,
            &localizer,
        );
        return;
    }
    let Some(pending) = protection.pending.take() else {
        return;
    };
    // Discard authorizes the switch; the catalog itself changes only on publication.
    if background::queue_open(
        pending,
        &mut commands,
        &mut session,
        &settings,
        &catalog,
        &localizer,
        timeline.as_deref(),
        navigation.as_deref(),
    ) {
        return;
    }
    execute_protected_document_action(
        pending,
        &mut commands,
        &mut session,
        &settings,
        &mut catalog,
        &mut workspace,
        &mut recovery,
        &mut autosave,
        &localizer,
        timeline.as_deref_mut(),
        navigation.as_deref_mut(),
    );
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
        state.first_unwritten_edit = None;
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
        state.first_unwritten_edit = None;
        state.observed_revision = session.document_revision();
        state.written_revision = None;
        state.write_after = now + interval;
    }

    if !session.dirty
        && session.standalone_material().is_none()
        && session.standalone_function().is_none()
    {
        state.first_unwritten_edit = None;
        try_clear_tracked_recovery(persistence, state, now, "saved effect recovery snapshot");
        return;
    }

    let revision = session.document_revision();
    if revision != state.observed_revision
        || state.observed_material_drafts != session.material_drafts
        || state.observed_material_target != session.material_target
    {
        state.observed_revision = revision;
        state
            .observed_material_drafts
            .clone_from(&session.material_drafts);
        state
            .observed_material_target
            .clone_from(&session.material_target);
        state.written_revision = None;
        state.write_after = now + interval;
        state.first_unwritten_edit.get_or_insert(now);
    }
    let deadline_due = state
        .first_unwritten_edit
        .is_some_and(|first| now >= first + interval * 4);
    if state.written_revision == Some(revision) || (now < state.write_after && !deadline_due) {
        return;
    }

    match persistence.persist_document(
        &session.effect,
        session.source_path.as_deref(),
        &session.material_drafts,
        &session.material_target,
    ) {
        Ok(_) => {
            state.written_revision = Some(revision);
            state.first_unwritten_edit = None;
            state.cleanup_after = now;
        }
        Err(error) => {
            error!("failed to write recovery snapshot: {error}");
            state.first_unwritten_edit = Some(now);
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

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn open_referenced_source(
    session: &mut EditorSession,
    settings: &EditorSettings,
    catalog: &ProjectEffectCatalog,
    workspace: &mut CurvesState,
    timeline: &mut TimelineState,
    navigation: &mut SourceNavigationState,
    source: EffectAssetRef,
    localizer: &Localizer,
) -> bool {
    let current = EffectAssetRef::new(session.effect.id);
    if source == current || navigation.contains(source) {
        set_persistence_status(
            session,
            localizer,
            PersistenceStatus::OpenFailed(
                "source navigation stopped because the reference is already in the breadcrumb"
                    .into(),
            ),
        );
        session.ui_revision += 1;
        return false;
    }
    let Some(return_path) = session.source_path.clone() else {
        set_persistence_status(
            session,
            localizer,
            PersistenceStatus::OpenFailed(
                "save the current effect before opening a referenced source".into(),
            ),
        );
        session.ui_revision += 1;
        return false;
    };
    let Some(source_path) = catalog.openable_path(source).map(Path::to_owned) else {
        set_persistence_status(
            session,
            localizer,
            PersistenceStatus::OpenFailed("referenced source is unavailable".into()),
        );
        session.ui_revision += 1;
        return false;
    };
    let entry = source_navigation_entry(session, timeline, return_path);
    if open_effect_path(session, &source_path, settings, catalog, localizer) {
        navigation.back.push(entry);
        navigation.forward.clear();
        *timeline = TimelineState::framed(session.playback_duration());
        workspace.clear();
        true
    } else {
        false
    }
}

fn source_navigation_entry(
    session: &EditorSession,
    timeline: &TimelineState,
    path: PathBuf,
) -> SourceNavigationEntry {
    SourceNavigationEntry {
        path,
        effect: EffectAssetRef::new(session.effect.id),
        name: session.effect.name.clone(),
        playhead_time: session.time(),
        playing: session.playing,
        selection: session.selection.primary,
        timeline: timeline.navigation_snapshot(),
    }
}

fn current_source_navigation_entry(
    session: &EditorSession,
    timeline: &TimelineState,
) -> Option<SourceNavigationEntry> {
    session
        .source_path
        .clone()
        .map(|path| source_navigation_entry(session, timeline, path))
}

#[cfg(test)]
fn return_to_source(
    session: &mut EditorSession,
    settings: &EditorSettings,
    catalog: &ProjectEffectCatalog,
    workspace: &mut CurvesState,
    timeline: &mut TimelineState,
    navigation: &mut SourceNavigationState,
    localizer: &Localizer,
) {
    let Some(depth) = navigation.back.len().checked_sub(1) else {
        return;
    };
    return_to_source_at(
        session, settings, catalog, workspace, timeline, navigation, depth, localizer,
    );
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn return_to_source_at(
    session: &mut EditorSession,
    settings: &EditorSettings,
    catalog: &ProjectEffectCatalog,
    workspace: &mut CurvesState,
    timeline: &mut TimelineState,
    navigation: &mut SourceNavigationState,
    depth: usize,
    localizer: &Localizer,
) {
    let Some(entry) = navigation.back.get(depth).cloned() else {
        return;
    };
    let Some(current) = current_source_navigation_entry(session, timeline) else {
        return;
    };
    if !open_effect_path(session, &entry.path, settings, catalog, localizer) {
        return;
    }
    let traversed = navigation.back.split_off(depth + 1);
    navigation.back.truncate(depth);
    navigation.forward.push(current);
    navigation.forward.extend(traversed.into_iter().rev());
    restore_source_navigation_entry(session, workspace, timeline, entry);
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn advance_to_source(
    session: &mut EditorSession,
    settings: &EditorSettings,
    catalog: &ProjectEffectCatalog,
    workspace: &mut CurvesState,
    timeline: &mut TimelineState,
    navigation: &mut SourceNavigationState,
    localizer: &Localizer,
) {
    let Some(entry) = navigation.forward.last().cloned() else {
        return;
    };
    let Some(current) = current_source_navigation_entry(session, timeline) else {
        return;
    };
    if !open_effect_path(session, &entry.path, settings, catalog, localizer) {
        return;
    }
    navigation.forward.pop();
    navigation.back.push(current);
    restore_source_navigation_entry(session, workspace, timeline, entry);
}

#[cfg(test)]
fn restore_source_navigation_entry(
    session: &mut EditorSession,
    workspace: &mut CurvesState,
    timeline: &mut TimelineState,
    entry: SourceNavigationEntry,
) {
    session.selection.primary = entry.selection;
    session.selection.repair(&session.effect);
    timeline.restore_navigation(entry.timeline, session.playback_duration());
    session.seek_time(entry.playhead_time);
    session.playing = entry.playing;
    workspace.clear();
}

/// Stage the new catalog and document together; failed opens leave the current project intact.
fn open_effect_in_project(
    session: &mut EditorSession,
    path: &Path,
    settings: &EditorSettings,
    catalog: &mut ProjectEffectCatalog,
    localizer: &Localizer,
) -> bool {
    if crate::project::contains_source(catalog, path) {
        return open_effect_path(session, path, settings, catalog, localizer);
    }
    let candidate = crate::project::folder_for_source(path)
        .and_then(|folder| crate::project::catalog_for_folder(&folder));
    match candidate {
        Ok(candidate) => {
            if !open_effect_path(session, path, settings, &candidate, localizer) {
                return false;
            }
            *catalog = candidate;
            true
        }
        Err(error) => {
            set_persistence_status(session, localizer, PersistenceStatus::OpenFailed(error));
            false
        }
    }
}

fn open_effect_path(
    session: &mut EditorSession,
    path: &Path,
    settings: &EditorSettings,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) -> bool {
    let result = fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|source| prepare_effect_asset(&source).map_err(|error| error.to_string()));
    match result {
        Ok(EffectAssetLoad::Current(effect)) => {
            open_prepared_effect(session, path, effect, settings, catalog, localizer)
        }
        Ok(EffectAssetLoad::MigrationRequired(migration)) => {
            if !confirm_asset_migration(path, &migration, localizer) {
                set_persistence_status(session, localizer, PersistenceStatus::MigrationCancelled);
                return false;
            }
            match persist_asset_migration(path, &migration) {
                Ok(backup) => match EffectAsset::load_ron(path) {
                    Ok(effect) => {
                        if !open_prepared_effect(
                            session, path, effect, settings, catalog, localizer,
                        ) {
                            return false;
                        }
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
                        true
                    }
                    Err(error) => {
                        set_persistence_status(
                            session,
                            localizer,
                            PersistenceStatus::OpenFailed(error.to_string()),
                        );
                        false
                    }
                },
                Err(error) => {
                    set_persistence_status(
                        session,
                        localizer,
                        PersistenceStatus::OpenFailed(error),
                    );
                    false
                }
            }
        }
        Err(error) => {
            set_persistence_status(session, localizer, PersistenceStatus::OpenFailed(error));
            false
        }
    }
}

fn open_prepared_effect(
    session: &mut EditorSession,
    path: &Path,
    effect: EffectAsset,
    settings: &EditorSettings,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) -> bool {
    match catalog.compile_project(&effect) {
        Ok(project) => {
            session.open_compiled_effect(path, effect, project.root);
            session.playing = settings.preview.play_on_open;
            set_persistence_status(
                session,
                localizer,
                PersistenceStatus::Opened(path.display().to_string()),
            );
            true
        }
        Err(error) => {
            set_persistence_status(session, localizer, PersistenceStatus::OpenFailed(error));
            false
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

fn save_session(
    session: &mut EditorSession,
    save_as: bool,
    localizer: &Localizer,
    catalog: &mut ProjectEffectCatalog,
) -> bool {
    if let Err(error) = catalog.material_drafts.preflight() {
        set_persistence_status(session, localizer, PersistenceStatus::SaveFailed(error));
        return false;
    }
    if !save_as && session.source_path.is_some() {
        let path = session
            .source_path
            .as_deref()
            .unwrap()
            .display()
            .to_string();
        return match if session.effect_is_dirty() {
            session.save()
        } else {
            Ok(())
        } {
            Ok(()) => finish_material_save(session, catalog, localizer, path),
            Err(error) => {
                set_persistence_status(
                    session,
                    localizer,
                    PersistenceStatus::SaveFailed(error.to_string()),
                );
                false
            }
        };
    }

    let file_name = format!("{}.aestra.ron", session.effect.id);
    let mut dialog = FileDialog::new()
        .add_filter(localizer.text("persistence-file-filter-effect"), &["ron"])
        .set_file_name(file_name)
        .set_directory(catalog.effect_root());
    if let Some(directory) = session.source_path.as_ref().and_then(|path| path.parent()) {
        dialog = dialog.set_directory(directory);
    }
    let Some(path) = dialog.save_file() else {
        set_persistence_status(session, localizer, PersistenceStatus::SaveCancelled);
        return false;
    };
    let display_path = path.display().to_string();
    match session.save_as(path) {
        Ok(()) => finish_material_save(session, catalog, localizer, display_path),
        Err(error) => {
            set_persistence_status(
                session,
                localizer,
                PersistenceStatus::SaveFailed(error.to_string()),
            );
            false
        }
    }
}

fn finish_material_save(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    localizer: &Localizer,
    path: String,
) -> bool {
    let result = catalog.save_material_drafts();
    session.set_material_drafts(catalog.material_drafts.clone());
    match result {
        Ok(()) => {
            set_persistence_status(session, localizer, PersistenceStatus::Saved(path));
            true
        }
        Err(error) => {
            set_persistence_status(
                session,
                localizer,
                PersistenceStatus::SaveFailed(format!(
                    "The effect is saved. Some material changes remain unsaved: {error}"
                )),
            );
            false
        }
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
    mut protection: ResMut<DocumentProtectionState>,
    io_tasks: Option<Res<crate::project_content::io::ProjectIoTasks>>,
) {
    let idle = crate::project_content::io::idle(io_tasks);
    for request in close_requests.read() {
        if request.window == *primary {
            // No recovery has been accepted yet. Closing the app must leave its snapshot
            // available next time, not activate or discard it behind the modal.
            if protection.recovery_open {
                autosave.suspended = true;
                commands.write_message(AppExit::Success);
                continue;
            }
            if !idle {
                session.status = localizer.text("project-operation-close-pending");
                continue;
            }
            // Defer asset recovery without touching its journal. An unsaved-document
            // exit prompt must not be hidden behind the asset recovery overlay.
            protection.asset_recovery_open = false;
            if document_action_requires_confirmation(&session, &settings) {
                protection.pending = Some(DocumentAction::Exit);
            } else {
                autosave.suspended = true;
                discard_active_recovery(&mut recovery);
                commands.write_message(AppExit::Success);
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
    use crate::menus::MenuKind;
    use crate::test_support;

    fn pending_material_edit(
        root: &Path,
    ) -> (
        EditorSession,
        ProjectEffectCatalog,
        aestra_core::material::MaterialProgram,
    ) {
        let effect_path = root.join("effect.aestra.ron");
        let program_path = root.join("program.aestra.material.ron");
        fs::write(&effect_path, crate::MATERIAL_GRAPH_LAB_EFFECT_SOURCE).unwrap();
        fs::write(&program_path, crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE).unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root);
        let mut session = test_support::session_with_timing_slack();
        assert!(open_effect_path(
            &mut session,
            &effect_path,
            &EditorSettings::default(),
            &catalog,
            &Localizer::new("en-US").unwrap()
        ));
        let original = aestra_core::material::MaterialProgram::load_ron(&program_path).unwrap();
        let mut replacement = original.clone();
        replacement.name = "Unsaved shared program".into();
        crate::history::MaterialProgramEditHistory::default()
            .execute_replacement(
                &mut session,
                &mut catalog,
                "Rename program",
                original,
                replacement.clone(),
            )
            .unwrap();
        (session, catalog, replacement)
    }

    #[test]
    fn save_commits_material_only_changes_without_rewriting_the_clean_effect() {
        let directory = tempfile::tempdir().unwrap();
        let (mut session, mut catalog, replacement) = pending_material_edit(directory.path());
        assert!(session.dirty);
        assert!(!session.effect_is_dirty());
        assert!(document_action_requires_confirmation(
            &session,
            &EditorSettings::default()
        ));
        let path = directory.path().join("effect.aestra.ron");
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        assert!(save_session(
            &mut session,
            false,
            &Localizer::new("en-US").unwrap(),
            &mut catalog
        ));
        assert!(!session.dirty);
        assert!(catalog.material_drafts.is_empty());
        assert_eq!(
            aestra_core::material::MaterialProgram::load_ron(
                directory.path().join("program.aestra.material.ron")
            )
            .unwrap(),
            replacement
        );
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
    }

    #[test]
    fn failed_navigation_keeps_material_drafts_and_discarded_new_document_drops_them() {
        let directory = tempfile::tempdir().unwrap();
        let (mut session, mut catalog, _) = pending_material_edit(directory.path());
        let original = fs::read(directory.path().join("program.aestra.material.ron")).unwrap();
        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let mut workspace = CurvesState::default();
        let mut recovery = RecoveryPersistence::for_test(directory.path().join("recovery"), None);
        let mut autosave = AutosaveState::new(&session, true);
        for action in [
            DocumentAction::OpenCatalog(EffectAssetRef::new(aestra_core::EffectId::new())),
            DocumentAction::New,
        ] {
            execute_protected_document_action(
                action,
                &mut commands,
                &mut session,
                &EditorSettings::default(),
                &mut catalog,
                &mut workspace,
                &mut recovery,
                &mut autosave,
                &Localizer::new("en-US").unwrap(),
                None,
                None,
            );
            assert_eq!(
                catalog.material_drafts.is_empty(),
                action == DocumentAction::New
            );
            assert_eq!(
                session.material_drafts.is_empty(),
                action == DocumentAction::New
            );
        }
        queue.apply(&mut world);
        assert_eq!(
            fs::read(directory.path().join("program.aestra.material.ron")).unwrap(),
            original
        );
    }

    #[test]
    fn external_open_switches_to_its_material_project_only_after_success() {
        let old = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let effects = external.path().join("assets/effects");
        let materials = external.path().join("assets/materials");
        fs::create_dir_all(&effects).unwrap();
        fs::create_dir_all(&materials).unwrap();
        let path = effects.join("lab.aestra.ron");
        fs::write(&path, crate::MATERIAL_GRAPH_LAB_EFFECT_SOURCE).unwrap();
        let mut catalog = crate::project::catalog_for_folder(old.path()).unwrap();
        let old_root = catalog.root().to_owned();
        let mut session = test_support::session_with_timing_slack();
        let original = session.effect.clone();
        let settings = EditorSettings::default();
        let localizer = Localizer::new("en-US").unwrap();

        assert!(!open_effect_in_project(
            &mut session,
            &path,
            &settings,
            &mut catalog,
            &localizer
        ));
        assert_eq!(catalog.root(), old_root);
        assert_eq!(session.effect, original);
        fs::write(
            materials.join("lab.aestra.material.ron"),
            crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE,
        )
        .unwrap();
        assert!(open_effect_in_project(
            &mut session,
            &path,
            &settings,
            &mut catalog,
            &localizer
        ));
        assert_eq!(
            catalog.root(),
            external.path().join("assets").canonicalize().unwrap()
        );
        assert_eq!(session.source_path.as_deref(), Some(path.as_path()));
        assert!(
            !session
                .preview
                .as_ref()
                .unwrap()
                .effect()
                .material_programs
                .is_empty()
        );
    }

    #[test]
    fn explicit_project_root_is_retained_for_effects_in_nested_folders() {
        let temporary = tempfile::tempdir().unwrap();
        let nested = temporary.path().join("scenes/deep");
        fs::create_dir_all(&nested).unwrap();
        let child = EffectAsset::new("Child", 1.0);
        child
            .save_ron(temporary.path().join("child.aestra.ron"))
            .unwrap();
        let mut parent = EffectAsset::new("Parent", 1.0);
        parent
            .effect_clips
            .push(aestra_core::EffectClip::new(child.id, 0.0, 1.0));
        let path = nested.join("parent.aestra.ron");
        parent.save_ron(&path).unwrap();
        let mut catalog = crate::project::catalog_for_folder(temporary.path()).unwrap();
        let root = catalog.root().to_owned();
        let mut session = test_support::session_with_timing_slack();
        assert!(open_effect_in_project(
            &mut session,
            &path,
            &EditorSettings::default(),
            &mut catalog,
            &Localizer::new("en-US").unwrap()
        ));
        assert_eq!(catalog.root(), root);
        assert_eq!(session.effect.id, parent.id);
    }

    #[test]
    fn document_protection_overlay_visibility_syncs_without_rebuilding_the_editor() {
        let mut app = App::new();
        app.init_resource::<DocumentProtectionState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_systems(Update, sync_document_protection_overlay);
        let description = app
            .world_mut()
            .spawn((DocumentProtectionDescription, Text::default()))
            .id();
        let overlay = app
            .world_mut()
            .spawn((DocumentProtectionOverlay, Node::default()))
            .id();

        app.world_mut()
            .resource_mut::<DocumentProtectionState>()
            .pending = Some(DocumentAction::New);
        app.update();
        assert_eq!(
            app.world().get::<Node>(overlay).unwrap().display,
            Display::Flex
        );
        assert!(
            app.world()
                .get::<Text>(description)
                .unwrap()
                .0
                .contains("all unsaved changes")
        );
        app.world_mut()
            .resource_mut::<DocumentProtectionState>()
            .pending = Some(DocumentAction::ReloadMaterial);
        app.update();
        assert!(
            app.world()
                .get::<Text>(description)
                .unwrap()
                .0
                .contains("only this material")
        );

        app.world_mut()
            .resource_mut::<DocumentProtectionState>()
            .pending = None;
        app.update();
        assert_eq!(
            app.world().get::<Node>(overlay).unwrap().display,
            Display::None
        );
    }

    #[test]
    fn catalog_open_action_uses_stable_id_and_document_protection_path() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("catalog-effect.aestra.ron");
        let mut effect = test_support::effect_with_timing_slack();
        effect.name = "Catalog Effect".into();
        effect.save_ron(&path).unwrap();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let reference = catalog.entries()[0].reference.unwrap();
        let session = test_support::session_with_timing_slack();
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
            .init_resource::<DocumentProtectionState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(execute_document_action);

        app.world_mut()
            .trigger(DocumentAction::OpenCatalog(reference));
        app.update();
        crate::project_content::io::drain(app.world_mut());

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.name, "Catalog Effect");
        assert_eq!(session.source_path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn catalog_open_compiles_an_effect_with_its_project_material_program() {
        let temporary = tempfile::tempdir().unwrap();
        let effects = temporary.path().join("effects");
        let materials = temporary.path().join("materials");
        fs::create_dir_all(&effects).unwrap();
        fs::create_dir_all(&materials).unwrap();
        let effect_path = effects.join("material_graph_lab.aestra.ron");
        fs::write(&effect_path, crate::MATERIAL_GRAPH_LAB_EFFECT_SOURCE).unwrap();
        fs::write(
            materials.join("material_graph_lab.aestra.material.ron"),
            crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE,
        )
        .unwrap();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let reference = catalog
            .entries()
            .iter()
            .find(|entry| entry.display_name == "Material Graph Lab")
            .and_then(|entry| entry.reference)
            .unwrap();
        let effect = EffectAsset::load_ron(&effect_path).unwrap();
        assert!(EffectCompiler::default().compile(&effect).is_err());
        assert!(catalog.compile_project(&effect).is_ok());

        let session = test_support::session_with_timing_slack();
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
            .init_resource::<DocumentProtectionState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(execute_document_action);

        app.world_mut()
            .trigger(DocumentAction::OpenCatalog(reference));
        app.update();
        crate::project_content::io::drain(app.world_mut());

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.name, "Material Graph Lab");
        assert_eq!(session.source_path.as_deref(), Some(effect_path.as_path()));
        assert!(session.preview.is_some());
    }

    #[test]
    fn catalog_clip_navigation_opens_the_owner_with_the_requested_clip_selected() {
        let temporary = tempfile::tempdir().unwrap();
        let child_path = temporary.path().join("child.aestra.ron");
        let mut child = EffectAsset::new("Child", 1.0);
        child.id = aestra_core::EffectId::from_u128(0xC101);
        child.save_ron(&child_path).unwrap();
        let owner_path = temporary.path().join("owner.aestra.ron");
        let mut owner = EffectAsset::new("Owner", 1.0);
        owner.id = aestra_core::EffectId::from_u128(0xC102);
        let clip = aestra_core::EffectClip::new(child.id, 0.0, 1.0);
        let clip_id = clip.id;
        owner.effect_clips.push(clip);
        owner.save_ron(&owner_path).unwrap();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let owner_reference = EffectAssetRef::new(owner.id);
        let session = test_support::session_with_timing_slack();
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
            .init_resource::<DocumentProtectionState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(execute_document_action);

        app.world_mut()
            .trigger(DocumentAction::OpenCatalogClip(owner_reference, clip_id));
        app.update();
        crate::project_content::io::drain(app.world_mut());

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.id, owner.id);
        assert_eq!(
            session.selection.primary,
            SemanticTarget::EffectClip(clip_id)
        );
    }

    #[test]
    fn source_emitter_navigation_opens_the_effect_with_the_requested_selection() {
        let temporary = tempfile::tempdir().unwrap();
        let source_path = temporary.path().join("source.aestra.ron");
        let mut source = test_support::effect_with_timing_slack();
        source.id = aestra_core::EffectId::from_u128(0x50A1CE);
        source.name = "Source".into();
        let target_id = source.emitters[1].id;
        source.save_ron(&source_path).unwrap();

        let parent_path = temporary.path().join("parent.aestra.ron");
        let mut parent = test_support::effect_with_timing_slack();
        parent.id = aestra_core::EffectId::from_u128(0xA11CE);
        parent.name = "Parent".into();
        parent.save_ron(&parent_path).unwrap();

        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let mut session = test_support::session_with_timing_slack();
        session.open(&parent_path).unwrap();
        let autosave = AutosaveState::new(&session, true);
        let duration = session.playback_duration();
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
            .init_resource::<DocumentProtectionState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .insert_resource(TimelineState::framed(duration))
            .init_resource::<SourceNavigationState>()
            .add_observer(execute_document_action);

        app.world_mut().trigger(DocumentAction::OpenSourceEmitter(
            EffectAssetRef::new(source.id),
            target_id,
        ));
        app.update();
        crate::project_content::io::drain(app.world_mut());

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.id, source.id);
        assert_eq!(
            session.selection.primary,
            SemanticTarget::Emitter(target_id)
        );
        assert!(
            app.world()
                .resource::<SourceNavigationState>()
                .can_go_back()
        );
    }

    #[test]
    fn source_navigation_round_trip_restores_playhead_selection_and_parent_context() {
        let temporary = tempfile::tempdir().unwrap();
        let grandchild_path = temporary.path().join("grandchild.aestra.ron");
        let mut grandchild = test_support::effect_with_timing_slack();
        grandchild.id = aestra_core::EffectId::from_u128(0x6A11D);
        grandchild.name = "Grandchild".into();
        grandchild.save_ron(&grandchild_path).unwrap();

        let child_path = temporary.path().join("child.aestra.ron");
        let mut child = test_support::effect_with_timing_slack();
        child.id = aestra_core::EffectId::from_u128(0xC111D);
        child.name = "Child".into();
        child.effect_clips.clear();
        child.effect_clips.push(aestra_core::EffectClip::new(
            EffectAssetRef::new(grandchild.id),
            0.0,
            1.0,
        ));
        child.save_ron(&child_path).unwrap();

        let parent_path = temporary.path().join("parent.aestra.ron");
        let mut parent = test_support::effect_with_timing_slack();
        parent.id = aestra_core::EffectId::from_u128(0xA11CE);
        parent.name = "Parent".into();
        parent.effect_clips.clear();
        let clip = aestra_core::EffectClip::new(EffectAssetRef::new(child.id), 0.0, 1.0);
        let clip_id = clip.id;
        parent.effect_clips.push(clip);
        parent.save_ron(&parent_path).unwrap();

        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let mut session = test_support::session_with_timing_slack();
        session.open(&parent_path).unwrap();
        session.select_effect_clip(clip_id);
        session.seek_time(1.25);
        session.playing = false;
        let mut timeline = TimelineState::framed(session.playback_duration());
        let mut navigation = SourceNavigationState::default();
        let mut workspace = CurvesState::default();
        let settings = EditorSettings::default();
        let localizer = Localizer::new("en-US").unwrap();

        open_referenced_source(
            &mut session,
            &settings,
            &catalog,
            &mut workspace,
            &mut timeline,
            &mut navigation,
            EffectAssetRef::new(child.id),
            &localizer,
        );
        assert_eq!(session.effect.id, child.id);
        assert!(navigation.can_go_back());
        assert!(!navigation.can_go_forward());
        assert_eq!(
            navigation.breadcrumb(&session.effect.name),
            ["Parent", "Child"]
        );

        open_referenced_source(
            &mut session,
            &settings,
            &catalog,
            &mut workspace,
            &mut timeline,
            &mut navigation,
            EffectAssetRef::new(parent.id),
            &localizer,
        );
        assert_eq!(session.effect.id, child.id, "cycles must not navigate");

        return_to_source(
            &mut session,
            &settings,
            &catalog,
            &mut workspace,
            &mut timeline,
            &mut navigation,
            &localizer,
        );
        assert_eq!(session.effect.id, parent.id);
        assert_eq!(
            session.selection.primary,
            SemanticTarget::EffectClip(clip_id)
        );
        assert!((session.time() - 1.25).abs() < 0.02);
        assert!(!session.playing);
        assert!(!navigation.can_go_back());

        open_referenced_source(
            &mut session,
            &settings,
            &catalog,
            &mut workspace,
            &mut timeline,
            &mut navigation,
            EffectAssetRef::new(child.id),
            &localizer,
        );
        open_referenced_source(
            &mut session,
            &settings,
            &catalog,
            &mut workspace,
            &mut timeline,
            &mut navigation,
            EffectAssetRef::new(grandchild.id),
            &localizer,
        );
        assert_eq!(navigation.depth(), 2);
        assert_eq!(
            navigation.breadcrumb(&session.effect.name),
            ["Parent", "Child", "Grandchild"]
        );

        return_to_source_at(
            &mut session,
            &settings,
            &catalog,
            &mut workspace,
            &mut timeline,
            &mut navigation,
            0,
            &localizer,
        );
        assert_eq!(session.effect.id, parent.id);
        assert_eq!(navigation.depth(), 0);
        assert!(navigation.can_go_forward());

        advance_to_source(
            &mut session,
            &settings,
            &catalog,
            &mut workspace,
            &mut timeline,
            &mut navigation,
            &localizer,
        );
        assert_eq!(session.effect.id, child.id);
        assert_eq!(navigation.depth(), 1);
        assert!(navigation.can_go_forward());

        advance_to_source(
            &mut session,
            &settings,
            &catalog,
            &mut workspace,
            &mut timeline,
            &mut navigation,
            &localizer,
        );
        assert_eq!(session.effect.id, grandchild.id);
        assert_eq!(navigation.depth(), 2);
        assert!(!navigation.can_go_forward());
    }

    #[test]
    fn unresolvable_catalog_reference_cannot_replace_the_document() {
        let temporary = tempfile::tempdir().unwrap();
        fs::write(temporary.path().join("broken.aestra.ron"), "not RON").unwrap();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let missing_reference = EffectAssetRef::new(aestra_core::EffectId::from_u128(0xBAD));
        let session = test_support::session_with_timing_slack();
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
            .init_resource::<DocumentProtectionState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(execute_document_action);

        app.world_mut()
            .trigger(DocumentAction::OpenCatalog(missing_reference));
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
        let mut session = test_support::session_with_timing_slack();
        session.status = "sentinel".into();

        session.new_effect();
        assert_eq!(session.status, "sentinel");
        session.save_as(&path).unwrap();
        assert_eq!(session.status, "sentinel");
        session.open(&path).unwrap();
        assert_eq!(session.status, "sentinel");
    }

    #[test]
    fn saving_an_effect_with_a_referenced_clip_clears_document_protection() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("effect-with-clip.aestra.ron");
        let mut session = test_support::session_with_timing_slack();
        let clip = aestra_core::EffectClip::new(
            EffectAssetRef::new(aestra_core::EffectId::from_u128(0xc11d)),
            0.25,
            1.0,
        );
        assert!(session.execute(
            "Add referenced effect clip",
            EffectCommand::AddEffectClip { clip, index: 0 },
            true,
        ));
        assert!(session.dirty);

        session.save_as(&path).unwrap();

        assert!(!session.dirty);
        assert!(!document_action_requires_confirmation(
            &session,
            &EditorSettings::default()
        ));
        assert_eq!(EffectAsset::load_ron(path).unwrap(), session.effect);
    }

    #[test]
    fn dirty_catalog_switch_opens_editor_protection_without_replacing_document() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("catalog-effect.aestra.ron");
        let mut catalog_effect = test_support::effect_with_timing_slack();
        catalog_effect.id = aestra_core::EffectId::from_u128(0xca7a10);
        catalog_effect.name = "Catalog target".into();
        catalog_effect.save_ron(&path).unwrap();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let reference = catalog.entries()[0].reference.unwrap();
        let mut session = test_support::session_with_timing_slack();
        let original = session.effect.id;
        session.adjust_effect_duration(0.1);
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
            .init_resource::<DocumentProtectionState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(execute_document_action);

        app.world_mut()
            .trigger(DocumentAction::OpenCatalog(reference));
        app.update();

        assert_eq!(app.world().resource::<EditorSession>().effect.id, original);
        assert_eq!(
            app.world().resource::<DocumentProtectionState>().pending,
            Some(DocumentAction::OpenCatalog(reference))
        );
        app.world_mut()
            .resource_mut::<DocumentProtectionState>()
            .pending = None;
        app.world_mut().trigger(DocumentAction::OpenProject);
        app.update();
        assert_eq!(app.world().resource::<EditorSession>().effect.id, original);
        assert_eq!(
            app.world().resource::<DocumentProtectionState>().pending,
            Some(DocumentAction::OpenProject)
        );
    }

    #[test]
    fn saved_effect_clip_document_switches_without_an_unsaved_prompt() {
        let temporary = tempfile::tempdir().unwrap();
        let target_path = temporary.path().join("catalog-target.aestra.ron");
        let mut target = test_support::effect_with_timing_slack();
        target.id = aestra_core::EffectId::from_u128(0xca7a10);
        target.name = "Catalog target".into();
        target.save_ron(&target_path).unwrap();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let reference = catalog.entries()[0].reference.unwrap();

        let source_path = temporary.path().join("source.aestra.ron");
        let mut session = test_support::session_with_timing_slack();
        session.effect.id = aestra_core::EffectId::from_u128(0x50a7ce);
        session.save_as(&source_path).unwrap();
        let clip = aestra_core::EffectClip::new(reference, 0.25, 1.0);
        assert!(session.execute(
            "Add referenced effect clip",
            EffectCommand::AddEffectClip { clip, index: 0 },
            true,
        ));
        assert!(session.dirty);

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
            .init_resource::<DocumentProtectionState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(execute_document_action);

        app.world_mut().trigger(DocumentAction::Save);
        app.update();
        crate::project_content::io::drain(app.world_mut());
        assert!(!app.world().resource::<EditorSession>().dirty);

        app.world_mut()
            .trigger(DocumentAction::OpenCatalog(reference));
        app.update();
        crate::project_content::io::drain(app.world_mut());

        assert_eq!(app.world().resource::<EditorSession>().effect.id, target.id);
        assert!(!app.world().resource::<DocumentProtectionState>().is_open());
    }

    #[test]
    fn saving_a_document_clears_its_tracked_recovery_snapshot() {
        let temporary = tempfile::tempdir().unwrap();
        let source_path = temporary.path().join("saved-effect.aestra.ron");
        let mut session = test_support::session_with_timing_slack();
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
    fn continuous_edits_cannot_postpone_recovery_forever() {
        let temporary = tempfile::tempdir().unwrap();
        let mut session = test_support::session_with_timing_slack();
        let settings = EditorSettings::default();
        let interval = u64::from(settings.general.autosave_interval_seconds);
        let localizer = Localizer::new("en-US").unwrap();
        let mut persistence = RecoveryPersistence::for_test(temporary.path().into(), None);
        let mut state = AutosaveState::new(&session, true);
        let start = Instant::now();
        for second in 0..=interval * 4 {
            assert!(session.set_effect_name(format!("Edit {second}")));
            autosave_recovery_at(
                &mut session,
                &settings,
                &mut persistence,
                &mut state,
                start + Duration::from_secs(second),
                &localizer,
            );
        }
        assert!(persistence.has_active());
        assert_eq!(state.written_revision, Some(session.document_revision()));
    }

    #[test]
    fn disabling_autosave_clears_a_snapshot_even_without_a_written_revision_marker() {
        let temporary = tempfile::tempdir().unwrap();
        let mut session = test_support::session_with_timing_slack();
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
        let mut session = test_support::session_with_timing_slack();
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
    fn document_action_activation_dispatches_directly_and_closes_the_menu() {
        let mut app = App::new();
        let menu = MenuState {
            open: Some(MenuKind::File),
            panels_open: true,
            ..default()
        };
        app.insert_resource(menu)
            .insert_resource(test_support::session_with_timing_slack())
            .add_observer(queue_document_action_activation);
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
        assert!(!action.contains::<PendingFeathersActivation>());
        assert_eq!(action.get::<Interaction>(), Some(&Interaction::None));
        let menu = app.world().resource::<MenuState>();
        assert_eq!(menu.open, None);
        assert!(!menu.panels_open);
    }

    #[test]
    fn file_menu_save_activation_persists_the_current_effect() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("menu-save.aestra.ron");
        let mut session = test_support::session_with_timing_slack();
        session.save_as(&path).unwrap();
        session.adjust_effect_duration(0.25);
        let expected_duration = session.effect.duration;
        assert!(session.dirty);
        let autosave = AutosaveState::new(&session, true);

        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(EditorSettings::default())
            .insert_resource(ProjectEffectCatalog::scan(temporary.path()))
            .init_resource::<CurvesState>()
            .init_resource::<MenuState>()
            .insert_resource(RecoveryPersistence::for_test(
                temporary.path().join("recovery"),
                None,
            ))
            .insert_resource(autosave)
            .init_resource::<DocumentProtectionState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(queue_document_action_activation)
            .add_observer(execute_document_action)
            .add_systems(Update, handle_document_action_buttons);
        let save = app
            .world_mut()
            .spawn((
                DocumentAction::Save,
                FeathersActionButton,
                Interaction::None,
                BackgroundColor(theme::PANEL_DARK),
            ))
            .id();

        app.world_mut().trigger(Activate { entity: save });
        app.update();
        crate::project_content::io::drain(app.world_mut());
        app.update();
        crate::project_content::io::drain(app.world_mut());

        assert!(!app.world().resource::<EditorSession>().dirty);
        assert_eq!(
            EffectAsset::load_ron(path).unwrap().duration,
            expected_duration
        );
    }

    #[test]
    fn migration_preserves_the_original_and_atomically_replaces_the_effect() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("legacy.aestra.ron");
        let original = "legacy format source";
        fs::write(&path, original).unwrap();
        let asset = test_support::effect_with_timing_slack();
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
        let mut asset = test_support::effect_with_timing_slack();
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
            asset: test_support::effect_with_timing_slack(),
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
