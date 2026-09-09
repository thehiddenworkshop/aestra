use super::*;
use bevy::input_focus::{FocusCause, InputFocus, tab_navigation::TabGroup};

#[derive(Component)]
pub(super) struct Overlay;
#[derive(Component)]
pub(super) struct Description;

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands, localizer: &Localizer) {
    parent
        .spawn((
            Overlay,
            TabGroup::modal(),
            GlobalZIndex(305),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Node {
                display: Display::None,
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
        .with_children(|parent| {
            parent
                .spawn((
                    Node {
                        width: Val::Px(580.0),
                        max_width: Val::Percent(92.0),
                        padding: UiRect::all(Val::Px(20.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(14.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                ))
                .with_children(|parent| {
                    parent.spawn((
                        Text::new(localizer.text("browser-recovery-title")),
                        TextColor(theme::TEXT),
                        Pickable::IGNORE,
                    ));
                    parent.spawn((
                        Description,
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(12.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        Pickable::IGNORE,
                    ));
                    parent
                        .spawn(Node {
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: Val::Px(8.0),
                            row_gap: Val::Px(8.0),
                            ..default()
                        })
                        .with_children(|parent| {
                            for (key, choice) in [
                                ("browser-recovery-later", Choice::Later),
                                ("browser-recovery-retry", Choice::Retry),
                                ("browser-recovery-restore", Choice::Restore),
                            ] {
                                button(parent, &localizer.text(key), choice);
                            }
                        });
                });
        });
}

fn button(parent: &mut ChildSpawnerCommands, label: &str, choice: Choice) -> Entity {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_button())
        .insert((
            choice,
            FeathersActionButton,
            TabIndex(0),
            AccessibleLabel(label.to_owned()),
            Node {
                height: Val::Px(28.0),
                padding: UiRect::horizontal(Val::Px(10.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_child((Text::new(label), ThemedText, Pickable::IGNORE))
        .id()
}

pub(crate) fn spawn_reopen_button(parent: &mut ChildSpawnerCommands, localizer: &Localizer) {
    let entity = button(
        parent,
        &localizer.text("browser-recovery-title"),
        Choice::Open,
    );
    parent
        .commands()
        .entity(entity)
        .entry::<Node>()
        .and_modify(|mut node| node.display = Display::None);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync(
    mut state: ResMut<RecoveryState>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    mut protection: ResMut<DocumentProtectionState>,
    localizer: Res<Localizer>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut overlays: Query<&mut Node, (With<Overlay>, Without<Choice>)>,
    mut descriptions: Query<&mut Text, With<Description>>,
    mut buttons: Query<(Entity, &Choice, &mut Node), Without<Overlay>>,
    mut focus: Option<ResMut<InputFocus>>,
    mut return_focus: Local<Option<Entity>>,
    mut was_open: Local<bool>,
    tasks: Option<Res<io::ProjectIoTasks>>,
    mut commands: Commands,
) {
    // A queued job can be refused before its worker starts if another action wins.
    if state.busy && io::idle(tasks) {
        state.busy = false;
        state.requested = true;
        state.error = Some(localizer.text("browser-recovery-changed"));
    }
    // Window close can hand focus to the normal Save/Discard/Cancel dialog.
    if !protection.asset_recovery_open {
        state.open = false;
    }
    if state.open && !state.busy && keys.is_some_and(|keys| keys.just_pressed(KeyCode::Escape)) {
        state.open = false;
        protection.asset_recovery_open = false;
    }
    if state.open
        && !*was_open
        && let Some(focus) = focus.as_mut()
    {
        *return_focus = focus.get();
        if let Some((entity, _, _)) = buttons
            .iter()
            .find(|(_, choice, _)| **choice == Choice::Later)
        {
            focus.set(entity, FocusCause::Navigated);
        }
    } else if !state.open
        && *was_open
        && let Some(focus) = focus.as_mut()
    {
        // The previous control can have been rebuilt while the dialog was open.
        if let Some(entity) = return_focus
            .take()
            .filter(|entity| commands.get_entity(*entity).is_ok())
        {
            focus.set(entity, FocusCause::Navigated);
        } else {
            focus.clear();
        }
    }
    *was_open = state.open;
    for mut node in &mut overlays {
        node.display = if state.open {
            Display::Flex
        } else {
            Display::None
        };
    }
    let clear = drafts_clear(&catalog, &session);
    let mut description = localizer.text("browser-recovery-description");
    if let Some(pending) = &state.pending {
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("files", pending.moved_files as i64);
        args.set("folders", pending.moved_folders as i64);
        args.set("path", pending.journal.display().to_string());
        description.push_str(&format!(
            "\n\n{}",
            localizer.text_with("browser-recovery-summary", &args)
        ));
    }
    if !clear {
        description.push_str(&format!(
            "\n\n{}",
            localizer.text("browser-recovery-drafts")
        ));
    }
    if let Some(error) = &state.error {
        description.push_str(&format!("\n\n{error}"));
        if state.pending.is_none() {
            description.push_str(&format!(
                "\n{}",
                catalog
                    .root()
                    .join(".aestra/asset-transactions/active.pending")
                    .display()
            ));
        }
    }
    for mut text in &mut descriptions {
        if text.0 != description {
            text.0.clone_from(&description);
        }
    }
    for (entity, choice, mut node) in &mut buttons {
        if *choice == Choice::Open {
            node.display = if state.available {
                Display::Flex
            } else {
                Display::None
            };
        }
        let disabled =
            state.busy || (*choice == Choice::Restore && (!clear || state.pending.is_none()));
        if disabled {
            commands.entity(entity).insert(InteractionDisabled);
        } else {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
}
