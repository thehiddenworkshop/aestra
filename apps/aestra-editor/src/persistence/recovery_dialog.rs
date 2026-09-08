//! Startup recovery is a retained, non-blocking editor modal, never an OS Yes/No alert.
use super::*;
use bevy::input_focus::{FocusCause, InputFocus, tab_navigation::TabGroup};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Resource, Default)]
pub(crate) struct RecoveryDialogState {
    pub(super) candidate: Option<RecoveryCandidate>,
    error: Option<String>,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Restore,
    Discard,
    Later,
}

#[derive(Component)]
pub(super) struct Overlay;
#[derive(Component)]
pub(super) struct Description;
#[derive(Component)]
pub(super) struct ErrorText;

pub(crate) fn spawn(
    parent: &mut ChildSpawnerCommands,
    protection: &DocumentProtectionState,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Overlay,
            TabGroup::modal(),
            GlobalZIndex(310),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Node {
                display: if protection.recovery_open {
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
                        width: Val::Px(540.0),
                        max_width: Val::Percent(92.0),
                        padding: UiRect::all(Val::Px(22.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(14.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(7.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                    BorderColor::all(theme::BORDER_BRIGHT),
                ))
                .with_children(|dialog| {
                    dialog
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::SpaceBetween,
                            ..default()
                        })
                        .with_children(|header| {
                            header.spawn((
                                Text::new(localizer.text("persistence-dialog-recovery-title")),
                                TextFont {
                                    font_size: FontSize::Px(17.0),
                                    ..default()
                                },
                                TextColor(theme::TEXT),
                                Pickable::IGNORE,
                            ));
                            button(
                                header,
                                "×",
                                &localizer.text("persistence-recovery-later"),
                                Choice::Later,
                            );
                        });
                    dialog.spawn((
                        Description,
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        Pickable::IGNORE,
                    ));
                    dialog.spawn((
                        ErrorText,
                        Node {
                            display: Display::None,
                            ..default()
                        },
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Pickable::IGNORE,
                    ));
                    dialog
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            flex_wrap: FlexWrap::Wrap,
                            justify_content: JustifyContent::End,
                            column_gap: Val::Px(8.0),
                            row_gap: Val::Px(8.0),
                            ..default()
                        })
                        .with_children(|buttons| {
                            for (key, choice) in [
                                ("persistence-recovery-later", Choice::Later),
                                ("persistence-recovery-discard", Choice::Discard),
                                ("persistence-recovery-restore", Choice::Restore),
                            ] {
                                let label = localizer.text(key);
                                button(buttons, &label, &label, choice);
                            }
                        });
                });
        });
}

fn button(parent: &mut ChildSpawnerCommands, label: &str, accessible: &str, choice: Choice) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_button())
        .insert((
            choice,
            FeathersActionButton,
            TabIndex(0),
            AccessibleLabel(accessible.to_owned()),
            Node {
                height: Val::Px(30.0),
                padding: UiRect::horizontal(Val::Px(12.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_child((Text::new(label), ThemedText, Pickable::IGNORE));
}

fn description(candidate: &RecoveryCandidate, localizer: &Localizer, now: SystemTime) -> String {
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(candidate.saved_at_unix_millis() / 1000);
    let (key, count) = match seconds {
        0..60 => ("persistence-recovery-age-now", 0),
        60..3600 => ("persistence-recovery-age-minutes", seconds / 60),
        3600..86400 => ("persistence-recovery-age-hours", seconds / 3600),
        _ => ("persistence-recovery-age-days", seconds / 86400),
    };
    let mut args = FluentArgs::new();
    args.set("count", count.to_string());
    let age = localizer.text_with(key, &args);
    args.set("effect", candidate.effect().name.clone());
    args.set("age", age);
    args.set("count", candidate.material_drafts().count().to_string());
    let context = match candidate.material_target() {
        crate::material_document::MaterialEditingTarget::Function { id, .. } => {
            let name = candidate
                .material_drafts()
                .functions
                .get(id)
                .and_then(|draft| draft.current.as_ref())
                .map_or_else(|| id.to_string(), |function| function.name.clone());
            args.set("function", name);
            localizer.text_with("persistence-recovery-function", &args)
        }
        crate::material_document::MaterialEditingTarget::EffectInstance => {
            localizer.text_with("persistence-recovery-effect", &args)
        }
        crate::material_document::MaterialEditingTarget::Program { id, .. } => {
            let name = candidate
                .material_drafts()
                .programs
                .get(id)
                .and_then(|draft| draft.current.as_ref())
                .map_or_else(|| id.to_string(), |program| program.name.clone());
            args.set("material", name);
            localizer.text_with("persistence-recovery-material", &args)
        }
    };
    format!(
        "{context}\n\n{}",
        localizer.text_with("persistence-recovery-details", &args)
    )
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn sync(
    state: Res<RecoveryDialogState>,
    localizer: Res<Localizer>,
    mut overlays: Query<(Entity, &mut Node), With<Overlay>>,
    mut descriptions: Query<&mut Text, (With<Description>, Without<ErrorText>)>,
    mut errors: Query<(&mut Text, &mut Node), (With<ErrorText>, Without<Overlay>)>,
    choices: Query<(Entity, &Choice)>,
    mut focus: Option<ResMut<InputFocus>>,
    mut focused_overlay: Local<Option<Entity>>,
) {
    let open = state.candidate.is_some();
    for (entity, mut node) in &mut overlays {
        let display = if open { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        if open && *focused_overlay != Some(entity) {
            if let Some(focus) = focus.as_mut()
                && let Some((button, _)) =
                    choices.iter().find(|(_, choice)| **choice == Choice::Later)
            {
                focus.set(button, FocusCause::Navigated);
            }
            *focused_overlay = Some(entity);
        }
    }
    if !open {
        *focused_overlay = None;
    }
    let summary = state
        .candidate
        .as_ref()
        .map(|candidate| description(candidate, &localizer, SystemTime::now()))
        .unwrap_or_default();
    for mut text in &mut descriptions {
        if text.0 != summary {
            text.0.clone_from(&summary);
        }
    }
    for (mut text, mut node) in &mut errors {
        let value = state.error.as_deref().unwrap_or_default();
        if text.0 != value {
            text.0 = value.to_owned();
        }
        let display = if state.error.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

fn finish(
    state: &mut RecoveryDialogState,
    protection: &mut DocumentProtectionState,
    autosave: &mut AutosaveState,
    session: &EditorSession,
    settings: &EditorSettings,
) {
    state.candidate = None;
    state.error = None;
    protection.recovery_open = false;
    *autosave = AutosaveState::new(session, settings.general.autosave_enabled);
}

pub(super) fn escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<RecoveryDialogState>,
    mut protection: ResMut<DocumentProtectionState>,
    mut autosave: ResMut<AutosaveState>,
    session: Res<EditorSession>,
    settings: Res<EditorSettings>,
    mut focus: Option<ResMut<InputFocus>>,
) {
    if state.candidate.is_some() && keys.just_pressed(KeyCode::Escape) {
        finish(
            &mut state,
            &mut protection,
            &mut autosave,
            &session,
            &settings,
        );
        if let Some(focus) = focus.as_mut() {
            focus.clear();
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn activate(
    event: On<Activate>,
    choices: Query<&Choice>,
    mut state: ResMut<RecoveryDialogState>,
    mut protection: ResMut<DocumentProtectionState>,
    mut autosave: ResMut<AutosaveState>,
    mut session: ResMut<EditorSession>,
    settings: Res<EditorSettings>,
    mut persistence: ResMut<RecoveryPersistence>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut layout: ResMut<WorkspaceLayout>,
    localizer: Res<Localizer>,
    mut focus: Option<ResMut<InputFocus>>,
) {
    let Ok(choice) = choices.get(event.entity) else {
        return;
    };
    let Some(candidate) = state.candidate.as_ref() else {
        return;
    };
    let result = match choice {
        Choice::Later => Ok(()),
        Choice::Discard => persistence
            .discard_candidate(candidate)
            .map(|()| {
                set_persistence_status(
                    &mut session,
                    &localizer,
                    PersistenceStatus::RecoveryDiscarded,
                );
            })
            .map_err(|error| {
                localize_persistence_status(
                    PersistenceStatus::RecoveryDiscardFailed(error.to_string()),
                    &localizer,
                )
            }),
        Choice::Restore => recovery_target::restore_candidate(
            &mut session,
            &mut persistence,
            candidate,
            &mut catalog,
        )
        .map(|mut warnings| {
            // Legacy effect-only snapshots can belong to a different project than startup.
            if let Some(path) = session.source_path.as_deref()
                && session.standalone_material().is_none()
                && session.standalone_function().is_none()
                && session.material_drafts.is_empty()
                && !crate::project::contains_source(&catalog, path)
            {
                let project = crate::project::folder_for_source(path)
                    .and_then(|folder| crate::project::catalog_for_folder(&folder))
                    .and_then(|catalog| {
                        catalog
                            .compile_project(&session.effect)
                            .map(|compiled| (catalog, compiled.root))
                    });
                match project {
                    Ok((project, compiled)) => {
                        if session.install_compiled_project_root(compiled).is_ok() {
                            *catalog = project;
                        }
                    }
                    Err(error) => warnings.push(error),
                }
            }
            session.playing = settings.preview.play_on_open;
            if session.standalone_material().is_some() || session.standalone_function().is_some() {
                reveal_dock_panel(&mut layout, &mut session, DockPanel::MaterialGraph);
            }
            let status = if warnings.is_empty() {
                PersistenceStatus::RecoveryRestored(session.effect.name.clone())
            } else {
                PersistenceStatus::RecoveryDiagnostic(warnings.join("; "))
            };
            set_persistence_status(&mut session, &localizer, status);
        }),
    };
    match result {
        Ok(()) => {
            finish(
                &mut state,
                &mut protection,
                &mut autosave,
                &session,
                &settings,
            );
            if let Some(focus) = focus.as_mut() {
                focus.clear();
            }
        }
        Err(error) => {
            state.error = Some(error);
        }
    }
}

#[cfg(test)]
mod tests;
